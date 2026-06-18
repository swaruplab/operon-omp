//! OMP (oh-my-pi) adapter — wraps the `omp` CLI in one-shot JSON mode.
//!
//! OMP (https://github.com/can1357/oh-my-pi, package `@oh-my-pi/pi-coding-agent`,
//! MIT) is a multi-provider terminal coding agent with a native Rust core. It
//! replaces OpenCode as Operon's agent "brain" with a goal of more autonomous,
//! more robust behaviour (model-role routing + fallback, subagent swarm,
//! programmable guardrails, hash-anchored edits, persistent Python kernel).
//!
//! ## Why this is a near-drop-in for Operon's harness
//!
//! Operon already drives its engine as a ONE-SHOT CLI emitting line-delimited
//! JSON on stdout, behind the `HarnessAdapter` trait. OMP supports exactly that
//! shape, so this adapter mirrors `opencode.rs`.
//!
//! ## CLI surface (verified against the real binary, omp v16.0.5)
//!
//! ```text
//! omp --mode json -p [--model provider/model] [-r|--resume <id>]
//!     [--thinking minimal|low|medium|high|xhigh] [--auto-approve]
//!     [--append-system-prompt <text>] [--no-tools] [@<file> | "<prompt>"]
//! ```
//!
//! - `--mode json` streams `AgentSessionEvent` objects as JSONL (one
//!   `JSON.stringify(event)` per line, no envelope); `-p` makes the run
//!   non-interactive. Confirmed: needs no TTY, so it works over SSH on HPC.
//! - The FIRST line is `{"type":"session","version":3,"id":"<uuid>",...}` — that
//!   `id` is the session id we surface for `--resume` (verified live).
//!
//! ## Event schema (verified from OMP source, branch `main`)
//!
//! - `session` — header; `.id` is the session id.
//! - `message_update` — streaming; the chunk is in `.assistantMessageEvent`
//!   (`text_delta` / `thinking_delta` / `toolcall_end` / `error`), and `.message`
//!   is the CUMULATIVE `AssistantMessage` snapshot (we read text/thinking from its
//!   `.content[]` blocks — no delta accumulation needed). There is no per-message
//!   id, so we synthesize a stable one (bumped on `message_start`).
//! - `tool_execution_start` — `toolCallId` / `toolName` / `args`.
//! - `tool_execution_end` — `toolCallId` / `result` / `isError`.
//! - `message_end` — `.message.usage` (`input`/`output`/`cacheRead`/`cacheWrite`/
//!   `totalTokens`, cost at `.cost.total`) and `.message.stopReason`.
//! - everything else (`agent_*`, `turn_*`, `auto_*`, `ttsr_triggered`, `notice`,
//!   `todo_*`, …) is structural/housekeeping → dropped or surfaced as `raw`.

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;

use super::{BuildContext, BuildOutput, Capabilities, HarnessAdapter};

/// Behavioural rules appended to OMP's system prompt on EVERY turn. These exist
/// because the run is headless (stdout redirected to a file, tailed over SSH):
/// anything that blocks on a TTY, a display, or an interactive approval prompt
/// would hang the one-shot command. Also encodes the HPC "register, don't poll"
/// rule so the agent registers long SLURM jobs with the watchdog instead of
/// blocking the turn on `squeue`.
const HEADLESS_RULES: &str = "OPERON RUNTIME RULES (non-interactive):\n\
    - OUTPUT LOCATION (critical): write EVERY file you create — scripts, results, figures, data, \
    websites — into the CURRENT WORKING DIRECTORY (the project folder you were launched in; run \
    `pwd` if unsure) using relative paths. NEVER write to /tmp or create your own temporary/scratch \
    directory: the user only sees the project folder, so anything written elsewhere is invisible to \
    them and effectively lost.\n\
    - You run headless: stdout is captured, there is no TTY and no interactive UI. \
    Never wait for keyboard input or a display. Render plots/files to disk, do not open windows.\n\
    - Long-running or batch jobs (e.g. SLURM `sbatch`): submit the job, report the job id, then \
    END YOUR TURN. Do NOT poll `squeue`/`sacct` in a loop — Operon's watchdog tracks completion.\n\
    - Prefer non-interactive flags for every shell tool (`-y`, `--no-input`, `--batch`).\n";

/// A bundled pre-tool-call guardrail hook (TypeScript). Written to
/// `~/.omp/hooks/pre/operon-guardrails.ts` by `ensure_omp_config`. Hard-blocks
/// destructive operations on data filesystems. Because the run is headless we
/// BLOCK rather than confirm.
const GUARDRAILS_HOOK_TS: &str = r#"// Operon guardrails — auto-installed. Hard-blocks destructive ops headless.
// Loaded by OMP's hook system (pre tool_call). Regenerated on first run if absent.
export default function (omp) {
  omp.on("tool_call", async (e) => {
    const name = String(e.toolName ?? e.tool ?? "");
    const input = (e.input ?? e.args ?? e.params ?? {});
    const cmd = String(input.command ?? input.cmd ?? "");
    if (name === "bash" || name === "shell") {
      // 1. Refuse rm on shared/data filesystems.
      if (/\brm\s+(-[a-z]*\s+)*[^|;&]*\/(dfs\d+|data|gpfs|scratch|nfs|projects?)\b/i.test(cmd)) {
        return { block: true, reason: "Operon guardrail: refusing rm on a shared/data filesystem path." };
      }
      // 2. Refuse recursive delete of a top-level path.
      if (/\brm\s+-[a-z]*r[a-z]*f?[a-z]*\s+(\/|~|\$HOME)\s*($|\s)/.test(cmd)) {
        return { block: true, reason: "Operon guardrail: refusing recursive delete of a top-level path." };
      }
    }
    return undefined;
  });
}
"#;

/// Write OMP's global config (`~/.omp/agent/models.yml` + `config.yml`) and the
/// guardrail hook IF they are missing. Idempotent and non-destructive — never
/// overwrites a user's hand-tuned config. Mirrors `opencode::ensure_default_config`
/// but targets OMP's config layout and wires in the autonomy features: model
/// roles (default/smol/slow/plan/commit), provider fallback chains, and the
/// guardrail hook.
///
/// `model` is expected in `provider/model` form (e.g. `ollama/qwen3:32b`).
/// Returns `Ok(true)` if anything was written, `Ok(false)` if everything existed.
pub fn ensure_omp_config(model: &str) -> Result<bool, String> {
    let home = dirs::home_dir().ok_or_else(|| "cannot resolve home dir".to_string())?;
    let agent_dir = home.join(".omp").join("agent");
    let hooks_dir = home.join(".omp").join("hooks").join("pre");

    let (provider, model_id) = match model.split_once('/') {
        Some((p, m)) if !p.is_empty() && !m.is_empty() => (p.to_string(), m.to_string()),
        _ => ("ollama".to_string(), model.to_string()),
    };
    let base_url = match provider.as_str() {
        "lmstudio" => "http://localhost:1234/v1",
        "vllm" => "http://localhost:8000/v1",
        _ => "http://localhost:11434/v1", // ollama + default
    };

    let mut wrote = false;
    std::fs::create_dir_all(&agent_dir)
        .map_err(|e| format!("create {}: {}", agent_dir.display(), e))?;
    std::fs::create_dir_all(&hooks_dir)
        .map_err(|e| format!("create {}: {}", hooks_dir.display(), e))?;

    // --- models.yml: provider definitions (OpenAI-compatible custom baseURL) ---
    let models_path = agent_dir.join("models.yml");
    if !models_path.exists() {
        let models_yml = format!(
            "# Auto-generated by Operon (OMP engine). Edit freely — not overwritten.\n\
             providers:\n\
             \x20 {provider}:\n\
             \x20   baseUrl: {base_url}\n\
             \x20   api: openai-completions\n\
             \x20   auth: none   # local engine; set apiKey/env for cloud providers\n\
             \x20   models:\n\
             \x20     - id: {model_id}\n\
             \x20       name: {model_id}\n",
        );
        std::fs::write(&models_path, models_yml)
            .map_err(|e| format!("write {}: {}", models_path.display(), e))?;
        wrote = true;
    }

    // --- config.yml: model roles + fallback chains + approval + autonomy ---
    let config_path = agent_dir.join("config.yml");
    if !config_path.exists() {
        let full_model = format!("{}/{}", provider, model_id);
        let config_yml = format!(
            "# Auto-generated by Operon (OMP engine). Edit freely — not overwritten.\n\
             # Model roles let the agent route sub-tasks to the right model.\n\
             modelRoles:\n\
             \x20 default: {full_model}\n\
             \x20 plan: {full_model}\n\
             \x20 smol: {full_model}\n\
             \x20 slow: {full_model}\n\
             \x20 commit: {full_model}\n\
             # Robustness: fall back automatically when a model/provider fails.\n\
             retry:\n\
             \x20 enabled: true\n\
             \x20 fallbackChains:\n\
             \x20   default: [{full_model}]\n\
             tools:\n\
             \x20 approvalMode: write\n",
        );
        std::fs::write(&config_path, config_yml)
            .map_err(|e| format!("write {}: {}", config_path.display(), e))?;
        wrote = true;
    }

    // --- guardrail hook ---
    let hook_path = hooks_dir.join("operon-guardrails.ts");
    if !hook_path.exists() {
        std::fs::write(&hook_path, GUARDRAILS_HOOK_TS)
            .map_err(|e| format!("write {}: {}", hook_path.display(), e))?;
        wrote = true;
    }

    if wrote {
        eprintln!(
            "[omp] wrote default config under {} (model={})",
            agent_dir.display(),
            model
        );
    }
    Ok(wrote)
}

/// Adapter state. OMP has no per-message id in its stream, so we synthesize a
/// stable one that bumps on each `message_start`, and we remember the session id
/// from the header line to attach it to the final `result`. The adapter instance
/// lives for the whole stream (`pick()` is called once per stream task in
/// `agent.rs`), so interior-mutable state is correct.
pub struct OmpAdapter {
    session_id: Mutex<String>,
    msg_index: Mutex<u64>,
}

impl Default for OmpAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl OmpAdapter {
    pub fn new() -> Self {
        Self {
            session_id: Mutex::new(String::new()),
            msg_index: Mutex::new(0),
        }
    }

    fn current_msg_id(&self) -> String {
        format!("omp-msg-{}", *self.msg_index.lock().unwrap())
    }
}

/// Shell single-quote escape for embedding a value inside `'...'`.
fn shq(s: &str) -> String {
    s.replace('\'', "'\\''")
}

/// Concatenate the text of every `content[]` block of the given `block_type`,
/// reading the `field` key from each (e.g. type "text" / field "text", or type
/// "thinking" / field "thinking").
fn join_blocks(message: Option<&Value>, block_type: &str, field: &str) -> String {
    let mut out = String::new();
    if let Some(arr) = message.and_then(|m| m.get("content")).and_then(|c| c.as_array()) {
        for block in arr {
            if block.get("type").and_then(|t| t.as_str()) == Some(block_type) {
                if let Some(s) = block.get(field).and_then(|v| v.as_str()) {
                    out.push_str(s);
                }
            }
        }
    }
    out
}

fn jstr(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

impl HarnessAdapter for OmpAdapter {
    fn id(&self) -> &'static str {
        "omp"
    }

    fn remote_bin_name(&self) -> &'static str {
        "omp"
    }

    fn install_hint(&self) -> &'static str {
        "curl -fsSL https://omp.sh/install | sh   (self-contained binary to ~/.local/bin, no root)"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            resume: true,     // -r/--resume <id> (verified)
            plan_mode: true,  // Operon plan scaffolding + write tool
            max_turns: false, // OMP uses --max-time (seconds), not a turn count
            mcp: true,        // first-class MCP (stdio + http)
        }
    }

    fn ensure_local_config(&self, _project_path: &str, model: &str) -> Result<bool, String> {
        ensure_omp_config(model)
    }

    fn build_command(&self, ctx: &BuildContext<'_>) -> Result<BuildOutput, String> {
        // --- Behavioural guidance -> system prompt (kept OUT of the user msg) ---
        let mut sys = String::new();
        match ctx.permission_mode {
            "safe_mode" => sys.push_str(
                "SAFE MODE: you may read/search/browse freely, but before any file write/edit or \
                 state-changing shell command, describe the action and ask the user for explicit \
                 confirmation in chat, then end your turn. Read-only commands are always allowed.\n",
            ),
            "supervised" => sys.push_str(
                "SUPERVISED MODE: prefer reversible actions; summarise any destructive step before doing it.\n",
            ),
            _ => {}
        }
        if !ctx.existing_plan.is_empty() && ctx.mode != "plan" {
            sys.push_str(&format!(
                "CONTEXT: an implementation_plan.md exists in this directory:\n---\n{}\n---\n\
                 Follow it if the request relates to it; otherwise ignore it. As you complete plan \
                 steps, mark them [x] in implementation_plan.md.\n",
                ctx.existing_plan
            ));
        }
        sys.push_str(HEADLESS_RULES);

        // --- Per-mode user message construction ---
        let mut prompt_file: Option<PathBuf> = None;
        let user_message = match ctx.mode {
            "plan" => {
                let prior = if ctx.existing_plan.is_empty() {
                    String::new()
                } else {
                    format!("\n\n<previous_plan>\n{}\n</previous_plan>", ctx.existing_plan)
                };
                format!(
                    "You are in PLAN mode. Write a single file 'implementation_plan.md' in the \
                     current directory and nothing else. Structure: # Implementation Plan: <title>; \
                     **Date:** {}; then 1) Overview 2) Step-by-step implementation 3) Files to create/modify \
                     4) Dependencies 5) Testing strategy 6) Risks; and a '## Status' section listing each \
                     step as [ ] so Agent mode can track progress.{}\n\nThe user's request: {}",
                    ctx.now_timestamp, prior, ctx.prompt
                )
            }
            "ask" => format!(
                "You are in ASK mode — a scientific Q&A assistant. Answer from your knowledge and any \
                 literature provided. If <pubmed_literature> tags are present, cite them [1], [2], … and \
                 end with a References section.\n\n{}",
                ctx.prompt
            ),
            "report" => {
                let report_prompt = format!(
                    "You are in REPORT mode — a scientific report generator for bioinformatics analyses. \
                     All needed file contents are inline below. Produce a professional analysis report. \
                     RULES: formal scientific prose; cite PubMed as [N]; Methods lists tools with versions; \
                     interpret results biologically; Discussion connects to literature. Output markdown \
                     sections (# Title, ## Abstract, ## Introduction, ## Results, ## Discussion, ## Methods, \
                     ## References).\n\n{}",
                    ctx.prompt
                );
                let path =
                    std::env::temp_dir().join(format!("operon-report-prompt-{}.txt", ctx.session_id));
                std::fs::write(&path, &report_prompt)
                    .map_err(|e| format!("write report prompt file: {}", e))?;
                eprintln!(
                    "[omp] report prompt written to {} ({} bytes)",
                    path.display(),
                    report_prompt.len()
                );
                prompt_file = Some(path);
                String::new() // message supplied via @file below
            }
            _ => ctx.prompt.to_string(), // agent (default)
        };

        // --- Assemble the CLI invocation ---
        // -p + --mode json => non-interactive JSONL event stream, then exit.
        // --auto-approve => never block on an approval prompt (headless); the
        //   permission tier is enforced via the system prompt + guardrail hooks.
        // --allow-home stops omp from auto-relocating to a temp dir when the
        // project folder is (or resolves to) $HOME — outputs must land in the
        // user's working folder (the launch cwd), which is the only place the
        // Operon file explorer shows them.
        let mut cmd = String::from("omp --mode json -p --auto-approve --allow-home");
        if let Some(m) = ctx.model {
            cmd.push_str(&format!(" --model '{}'", shq(m)));
        }
        if let Some(resume) = ctx.resume_session {
            cmd.push_str(&format!(" --resume '{}'", shq(resume)));
        }
        let thinking = match ctx.mode {
            "plan" | "report" => "high",
            _ => "medium",
        };
        cmd.push_str(&format!(" --thinking {}", thinking));
        if ctx.mode == "ask" {
            cmd.push_str(" --no-tools");
        }
        if !sys.is_empty() {
            cmd.push_str(&format!(" --append-system-prompt '{}'", shq(&sys)));
        }

        let final_cmd = match (&prompt_file, ctx.mode) {
            (Some(path), "report") => format!("{} @'{}'", cmd, shq(&path.to_string_lossy())),
            _ => format!("{} '{}'", cmd, shq(&user_message)),
        };

        // OMP configures MCP via its own config + universal discovery, and uses
        // --max-time (not a turn count), so those caller fields aren't flags here.
        let _ = ctx.mcp_config_path;
        let _ = ctx.max_turns;

        Ok(BuildOutput {
            command: final_cmd,
            prompt_file,
        })
    }

    /// Translate one OMP `--mode json` event line into the canonical NDJSON shape
    /// the frontend understands (`system`, `assistant{text|thinking|tool_use}`,
    /// `tool`, `result`, `error`, or `raw`).
    fn normalize_line(&self, raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        let value: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => {
                return Some(format!(
                    "{{\"type\":\"raw\",\"source\":\"omp\",\"line\":{}}}",
                    jstr(trimmed)
                ));
            }
        };

        let event_type = value.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match event_type {
            // Header (line 1). Remember + surface the session id for resume.
            "session" | "session_start" => {
                let sid = value.get("id").and_then(|v| v.as_str()).unwrap_or("");
                *self.session_id.lock().unwrap() = sid.to_string();
                Some(format!("{{\"type\":\"system\",\"session_id\":{}}}", jstr(sid)))
            }

            // New assistant message — bump the synthetic id so the frontend treats
            // it as a new turn (append rather than replace).
            "message_start" => {
                *self.msg_index.lock().unwrap() += 1;
                None
            }

            // Streaming chunk. The sub-event tells us what kind; `.message` is the
            // cumulative snapshot we render from.
            "message_update" => {
                let ame_type = value
                    .get("assistantMessageEvent")
                    .and_then(|a| a.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                let message = value.get("message");
                match ame_type {
                    "text_start" | "text_delta" | "text_end" => {
                        let text = join_blocks(message, "text", "text");
                        Some(format!(
                            "{{\"type\":\"assistant\",\"message\":{{\"id\":{},\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":{}}}]}}}}",
                            jstr(&self.current_msg_id()),
                            jstr(&text)
                        ))
                    }
                    "thinking_start" | "thinking_delta" | "thinking_end" => {
                        let think = join_blocks(message, "thinking", "thinking");
                        Some(format!(
                            "{{\"type\":\"assistant\",\"message\":{{\"id\":{},\"role\":\"assistant\",\"content\":[{{\"type\":\"thinking\",\"thinking\":{}}}]}}}}",
                            jstr(&format!("{}-thinking", self.current_msg_id())),
                            jstr(&think)
                        ))
                    }
                    // The streaming error sub-event carries an AssistantMessage.
                    "error" => {
                        let msg = value
                            .get("assistantMessageEvent")
                            .and_then(|a| a.get("error"))
                            .and_then(|e| e.get("errorMessage"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("OMP error");
                        Some(format!(
                            "{{\"type\":\"error\",\"error\":{{\"message\":{}}}}}",
                            jstr(msg)
                        ))
                    }
                    // start / done / toolcall_* — no separate render (tool calls
                    // are surfaced via the top-level tool_execution_* events).
                    _ => None,
                }
            }

            // Tool call begins (canonical "running" signal).
            "tool_execution_start" => {
                let tool_id = value.get("toolCallId").and_then(|v| v.as_str()).unwrap_or("omp-tool");
                let tool_name = value.get("toolName").and_then(|v| v.as_str()).unwrap_or("tool");
                let input = value.get("args").cloned().unwrap_or_else(|| serde_json::json!({}));
                Some(format!(
                    "{{\"type\":\"assistant\",\"message\":{{\"id\":{},\"role\":\"assistant\",\"content\":[{{\"type\":\"tool_use\",\"id\":{},\"name\":{},\"input\":{}}}]}}}}",
                    jstr(&format!("omp-tool-{}", tool_id)),
                    jstr(tool_id),
                    jstr(tool_name),
                    serde_json::to_string(&input).unwrap_or_else(|_| "{}".into())
                ))
            }

            // Tool result.
            "tool_execution_end" => {
                let tool_id = value.get("toolCallId").and_then(|v| v.as_str()).unwrap_or("");
                let content = match value.get("result") {
                    Some(Value::String(s)) => s.clone(),
                    Some(other) => other.to_string(),
                    None => String::new(),
                };
                Some(format!(
                    "{{\"type\":\"tool\",\"tool_use_id\":{},\"content\":{}}}",
                    jstr(tool_id),
                    jstr(&content)
                ))
            }

            // Message complete — usage + stop reason live on `.message`.
            "message_end" => {
                let message = value.get("message");
                let stop = message
                    .and_then(|m| m.get("stopReason"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("stop");
                if stop == "error" || stop == "aborted" {
                    let em = message
                        .and_then(|m| m.get("errorMessage"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("OMP error");
                    return Some(format!(
                        "{{\"type\":\"error\",\"error\":{{\"message\":{}}}}}",
                        jstr(em)
                    ));
                }
                let usage = message.and_then(|m| m.get("usage"));
                let get_u = |k: &str| usage.and_then(|u| u.get(k)).and_then(|v| v.as_i64()).unwrap_or(0);
                let cost = usage
                    .and_then(|u| u.get("cost"))
                    .and_then(|c| c.get("total"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let usage_out = serde_json::json!({
                    "input": get_u("input"),
                    "output": get_u("output"),
                    "total": get_u("totalTokens"),
                    "cacheRead": get_u("cacheRead"),
                    "cacheWrite": get_u("cacheWrite"),
                    "reasoning": usage.and_then(|u| u.get("reasoningTokens")).and_then(|v| v.as_i64()),
                });
                let sid = self.session_id.lock().unwrap().clone();
                Some(format!(
                    "{{\"type\":\"result\",\"session_id\":{},\"subtype\":{},\"usage\":{},\"total_cost_usd\":{}}}",
                    jstr(&sid),
                    jstr(stop),
                    serde_json::to_string(&usage_out).unwrap_or_else(|_| "{}".into()),
                    cost
                ))
            }

            // Out-of-band session notice (surface errors; ignore info/warning).
            "notice" => {
                if value.get("level").and_then(|v| v.as_str()) == Some("error") {
                    let msg = value.get("message").and_then(|v| v.as_str()).unwrap_or("OMP error");
                    Some(format!(
                        "{{\"type\":\"error\",\"error\":{{\"message\":{}}}}}",
                        jstr(msg)
                    ))
                } else {
                    None
                }
            }

            // Structural lifecycle — no render (frontend gets agent-done on EOF).
            "agent_start" | "turn_start" | "turn_end" | "agent_end"
            | "tool_execution_update" => None,

            // Housekeeping that's worth seeing during bring-up but not fatal.
            _ => {
                eprintln!("[omp:unknown] {}", trimmed);
                Some(format!(
                    "{{\"type\":\"raw\",\"source\":\"omp\",\"event\":{}}}",
                    trimmed
                ))
            }
        }
    }
}
