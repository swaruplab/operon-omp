//! OpenCode adapter — wraps the `opencode` CLI.
//!
//! OpenCode (https://opencode.ai) is a provider-agnostic open-source agent CLI.
//! In Operon Enterprise it is the default harness, configured to talk to a
//! local Ollama (or vLLM / TGI / LM-Studio) server.
//!
//! ## CLI surface (verified from public docs)
//!
//! ```text
//! opencode run [--format json] [--model provider/model]
//!              [--continue | --session <ID>] [--fork] "<prompt>"
//! ```
//!
//! - `--format json` emits raw JSON events on stdout. We normalize each line
//!   into the canonical `AgentEvent` shape the frontend already understands.
//! - `--model provider/model` (e.g. `ollama/qwen2.5-coder:7b`).
//! - `--session <ID>` resumes a specific session, `--continue` resumes the
//!   most recent one.
//!
//! ## What is NOT verified
//!
//! The exact JSON event schema produced by `--format json` is not published
//! in the docs. The translator below handles a reasonable superset and
//! falls through with `type: "raw"` for unknown shapes so we can iterate
//! once we see real output. See `normalize_line` for details.
//!
//! ## OpenCode-specific behavior notes
//!
//! - No `--max-turns` flag. Turn budget is not enforced.
//! - No `--dangerously-skip-permissions`. Permission policy is configured
//!   in `opencode.json`.
//! - No `--mcp-config`; MCP is configured via `opencode.json`.
//! - Plan/ask/agent/report modes are Operon-side conventions baked into the
//!   prompt — OpenCode itself has no notion of them.

use std::path::{Path, PathBuf};

use super::{BuildContext, BuildOutput, Capabilities, HarnessAdapter};

/// Write a default `opencode.json` into `project_path` if one doesn't already
/// exist. Configures the local Ollama provider via `@ai-sdk/openai-compatible`
/// and pins the model. Idempotent — never overwrites a hand-tuned config.
///
/// Returns `Ok(true)` if a config was written, `Ok(false)` if one already
/// existed (and was therefore left alone). The model id is expected in
/// `provider/model` form (e.g. `"ollama/kimi-k2.6:cloud"`); only the model
/// portion is registered under the provider's `models` block.
pub fn ensure_default_config(project_path: &str, model: &str) -> Result<bool, String> {
    let config_path = Path::new(project_path).join("opencode.json");
    if config_path.exists() {
        return Ok(false);
    }

    let (provider, model_id) = match model.split_once('/') {
        Some((p, m)) if !p.is_empty() && !m.is_empty() => (p, m),
        _ => ("ollama", model),
    };

    // Only emit a known-good template for the local Ollama case. For other
    // providers we still write a stub but the user will need to fill in
    // baseURL / auth — log a hint via eprintln so it shows up in dev console.
    let base_url = if provider == "ollama" {
        "http://localhost:11434/v1"
    } else {
        eprintln!(
            "[opencode] unknown provider '{}', writing stub config — edit opencode.json to set baseURL/auth",
            provider
        );
        "http://localhost:11434/v1"
    };

    let config = serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "provider": {
            provider: {
                "npm": "@ai-sdk/openai-compatible",
                "options": { "baseURL": base_url },
                "models": {
                    model_id: { "tools": true }
                }
            }
        },
        "model": format!("{}/{}", provider, model_id)
    });

    let body = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("serialize opencode.json: {}", e))?;
    std::fs::write(&config_path, body)
        .map_err(|e| format!("write {}: {}", config_path.display(), e))?;
    eprintln!(
        "[opencode] wrote default config at {} (model={}/{})",
        config_path.display(),
        provider,
        model_id
    );
    Ok(true)
}

#[derive(Default)]
pub struct OpenCodeAdapter;

impl OpenCodeAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl HarnessAdapter for OpenCodeAdapter {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            resume: true,
            plan_mode: false,
            max_turns: false,
            mcp: false,
        }
    }

    fn install_hint(&self) -> &'static str {
        "curl -fsSL https://opencode.ai/install | bash"
    }

    fn ensure_local_config(&self, project_path: &str, model: &str) -> Result<bool, String> {
        ensure_default_config(project_path, model)
    }

    fn build_command(&self, ctx: &BuildContext<'_>) -> Result<BuildOutput, String> {
        let escaped_prompt = ctx.prompt.replace('\'', "'\\''");

        // Operon-side mode conventions, expressed as prompt prefixes.
        // OpenCode has no native concept of plan/ask/agent/report — we
        // shape the agent's behaviour entirely through the prompt.
        let safety_prefix = if ctx.permission_mode == "safe_mode" {
            "IMPORTANT SAFETY CONSTRAINT: You are in SAFE MODE. You may freely read files, search, \
             and browse, but you MUST ask the user for explicit confirmation before: \
             (1) writing or editing any file, (2) running any bash command that modifies state \
             (installs, deletes, moves, or overwrites), (3) creating new files. \
             For any such action, describe what you plan to do and wait for the user to say 'yes' or 'go ahead' \
             before executing. Read-only commands (cat, ls, grep, find, head, etc.) are always safe to run.\n\n"
                .to_string()
        } else {
            String::new()
        };

        let plan_context = if !ctx.existing_plan.is_empty() && ctx.mode != "plan" {
            format!(
                "CONTEXT: There is an existing implementation_plan.md in this directory from a previous planning session. \
                 Here is its content:\n\n---\n{}\n---\n\n\
                 Use this plan as context for your work. If the user's request relates to this plan, follow it. \
                 If the request is unrelated, you can ignore the plan.\n\n",
                ctx.existing_plan
            )
        } else {
            String::new()
        };
        let context_prefix = format!("{}{}", safety_prefix, plan_context);

        let mut prompt_file: Option<PathBuf> = None;

        let final_prompt = match ctx.mode {
            "plan" => {
                let prior = if !ctx.existing_plan.is_empty() {
                    format!(
                        "\n\nCONTEXT: The previous implementation plan (now archived) is shown below for reference. \
                         Use it to understand what has already been planned or completed. \
                         You may reference, build upon, or supersede it — but write your plan as a \
                         fresh, self-contained document.\n\n\
                         <previous_plan>\n{}\n</previous_plan>",
                        ctx.existing_plan
                    )
                } else {
                    String::new()
                };
                format!(
                    "{}You are in PLAN mode.\n\n\
                     CRITICAL INSTRUCTION: Your ONLY action is to write a file called 'implementation_plan.md' \
                     in the current directory. Do NOT run shell commands, read files, or use search tools — \
                     you already have all the context you need in this prompt.\n\n\
                     FORMATTING RULES:\
                     \n- Start with: # Implementation Plan: <short title>\
                     \n- Add: **Date:** {}\
                     \n- Then: 1) Overview, 2) Step-by-step implementation, 3) Files to create or modify, \
                     4) Dependencies, 5) Testing strategy, 6) Risks.\
                     \n- Include a '## Status' section with each step as [ ] (pending) so Agent mode can \
                     mark progress.{}\
                     \n\nThe user's request: {}",
                    safety_prefix, ctx.now_timestamp, prior, escaped_prompt
                )
            }
            "report" => {
                // Big-prompt-via-file pattern. OpenCode's `run`
                // accepts the prompt as a positional argument, so we read the
                // file and pass its content. The caller still SCPs the file
                // for remote runs (path is reconstructed from session_id) —
                // we keep the file for that purpose.
                let tool_instruction =
                    "CRITICAL: All file contents are already provided inline in this prompt inside <file> tags. \
                     Do NOT use any tools — no file reads, writes, or shell commands. \
                     Write the entire report directly from the provided content.";
                let report_prompt = format!(
                    "You are in REPORT mode — a scientific report generator for bioinformatics analyses. \
                     Produce a professional analysis report from the provided files and context.\n\n\
                     {}\n\n\
                     RULES:\n\
                     1. Formal scientific prose suitable for a research report.\n\
                     2. Cite PubMed references using [N] notation for biological claims.\n\
                     3. Methods section lists tools with version numbers (omit infrastructure details).\n\
                     4. Interpret results biologically — explain what plots mean.\n\
                     5. Discussion connects findings to broader literature.\n\
                     6. Use implementation_plan.md (if available) for analysis context.\n\n\
                     Output as markdown sections (# Title, ## Abstract, ## Introduction, ## Results, \
                     ## Discussion, ## Methods, ## References).\n\n\
                     {}{}",
                    tool_instruction, context_prefix, ctx.prompt
                );

                let path = std::env::temp_dir()
                    .join(format!("operon-report-prompt-{}.txt", ctx.session_id));
                std::fs::write(&path, &report_prompt)
                    .map_err(|e| format!("Failed to write report prompt file: {}", e))?;
                eprintln!(
                    "[operon] OpenCode report prompt written to {} ({} bytes)",
                    path.to_string_lossy(),
                    report_prompt.len()
                );
                prompt_file = Some(path);

                report_prompt
            }
            "ask" => {
                format!(
                    "{}You are in ASK mode — a scientific Q&A assistant. \
                     Do NOT use any tools (no file reads, writes, or shell commands). \
                     Answer using your knowledge and any literature provided in the prompt. \
                     If <pubmed_literature> tags are present, cite them by number [1], [2], … \
                     and end with a References section.\n\n{}",
                    context_prefix, escaped_prompt
                )
            }
            _ => {
                // Agent (default).
                if !ctx.existing_plan.is_empty() {
                    format!(
                        "{}IMPORTANT: As you complete steps from the implementation plan, \
                         update implementation_plan.md to mark completed steps with [x] \
                         so progress is tracked.\n\n{}",
                        context_prefix, escaped_prompt
                    )
                } else {
                    format!("{}{}", context_prefix, escaped_prompt)
                }
            }
        };

        // Build the CLI invocation.
        let mut cmd = String::from("opencode run --format json");
        if let Some(m) = ctx.model {
            cmd.push_str(&format!(" --model {}", m));
        }
        if let Some(resume) = ctx.resume_session {
            cmd.push_str(&format!(" --session {}", resume));
        }

        // For report mode, the prompt is huge — pipe via stdin to avoid
        // ARG_MAX issues. `opencode run` reads positional prompt from
        // stdin when the trailing arg is absent (verified for OpenCode;
        // assumed similar for OpenCode — falls back to inline arg if not).
        // We use the same `cat file | opencode run …` shape.
        let final_cmd = if ctx.mode == "report" {
            if let Some(ref path) = prompt_file {
                let path_str = path.to_string_lossy().to_string();
                #[cfg(target_os = "windows")]
                let pipe = format!("type \"{}\" | {}", path_str, cmd);
                #[cfg(not(target_os = "windows"))]
                let pipe = format!("cat '{}' | {}", path_str, cmd);
                pipe
            } else {
                format!("{} '{}'", cmd, final_prompt.replace('\'', "'\\''"))
            }
        } else {
            format!("{} '{}'", cmd, final_prompt.replace('\'', "'\\''"))
        };

        // `--max-turns` and `--mcp-config` are intentionally NOT emitted
        // (OpenCode doesn't support them). The caller's mcp_config_path is
        // ignored — users configure MCP via `opencode.json` instead.
        let _ = ctx.max_turns;
        let _ = ctx.mcp_config_path;

        Ok(BuildOutput {
            command: final_cmd,
            prompt_file,
        })
    }

    /// Translate one OpenCode JSON event line into an `AgentEvent`-shaped
    /// line that the frontend already knows how to render.
    ///
    /// Real OpenCode event schema (observed from `opencode run --format json`
    /// against Ollama, kimi-k2.6:cloud, opencode 1.14.39):
    ///
    /// ```json
    /// {"type":"step_start","timestamp":...,"sessionID":"ses_...",
    ///  "part":{"id":"prt_...","messageID":"msg_...","sessionID":"ses_...","type":"step-start"}}
    /// {"type":"text","timestamp":...,"sessionID":"ses_...",
    ///  "part":{"id":"prt_...","messageID":"msg_...","sessionID":"ses_...",
    ///          "type":"text","text":"...","time":{"start":...,"end":...}}}
    /// {"type":"step_finish","timestamp":...,"sessionID":"ses_...",
    ///  "part":{"id":"prt_...","reason":"stop","messageID":"msg_...","sessionID":"ses_...",
    ///          "type":"step-finish",
    ///          "tokens":{"total":...,"input":...,"output":...,"reasoning":...,
    ///                    "cache":{"write":...,"read":...}},
    ///          "cost":0}}
    /// ```
    ///
    /// Tool events follow the same envelope (`{type, sessionID, part:{...}}`)
    /// — schema for those still inferred until we run a tool-using prompt.
    fn normalize_line(&self, raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        let value: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => {
                // Not JSON — wrap as a raw text event so it still surfaces
                // in the chat (with provenance) instead of being dropped.
                return Some(format!(
                    "{{\"type\":\"raw\",\"source\":\"opencode\",\"line\":{}}}",
                    serde_json::to_string(trimmed).unwrap_or_else(|_| "\"\"".into())
                ));
            }
        };

        let event_type = value
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        // Top-level sessionID (camelCase D, OpenCode quirk).
        let session_id = value
            .get("sessionID")
            .or_else(|| value.get("sessionId"))
            .or_else(|| value.get("session_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Most events nest the payload under `part`.
        let part = value.get("part");

        match event_type.as_str() {
            // Turn begins. Frontend's "system" event carries the session id
            // — emit it once per turn so resume metadata stays in sync.
            "step_start" => Some(format!(
                "{{\"type\":\"system\",\"session_id\":{}}}",
                serde_json::to_string(session_id).unwrap_or_else(|_| "\"\"".into())
            )),

            // Assistant text. AgentEvent shape: assistant message with a
            // single text content block.
            "text" => {
                let text = part
                    .and_then(|p| p.get("text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let msg_id = part
                    .and_then(|p| p.get("messageID"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("opencode-msg");
                Some(format!(
                    "{{\"type\":\"assistant\",\"message\":{{\"id\":{},\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":{}}}]}}}}",
                    serde_json::to_string(msg_id).unwrap_or_else(|_| "\"opencode-msg\"".into()),
                    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
                ))
            }

            // Turn complete with token usage. AgentEvent's "result" carries
            // session_id + optional usage so the cost meter can update.
            "step_finish" => {
                let usage = part
                    .and_then(|p| p.get("tokens"))
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                let cost = part
                    .and_then(|p| p.get("cost"))
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!(0));
                let reason = part
                    .and_then(|p| p.get("reason"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("stop");
                Some(format!(
                    "{{\"type\":\"result\",\"session_id\":{},\"subtype\":{},\"usage\":{},\"total_cost_usd\":{}}}",
                    serde_json::to_string(session_id).unwrap_or_else(|_| "\"\"".into()),
                    serde_json::to_string(reason).unwrap_or_else(|_| "\"stop\"".into()),
                    serde_json::to_string(&usage).unwrap_or_else(|_| "{}".into()),
                    serde_json::to_string(&cost).unwrap_or_else(|_| "0".into()),
                ))
            }

            // Tool call. Schema not yet observed — assume `part.tool`,
            // `part.input` follow OpenCode's part-wrapper convention. If the
            // real shape differs, the [opencode:unknown] log will catch it.
            "tool_call" | "tool_use" | "tool" => {
                let tool_id = part
                    .and_then(|p| p.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("opencode-tool");
                let tool_name = part
                    .and_then(|p| p.get("tool"))
                    .or_else(|| part.and_then(|p| p.get("name")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("tool");
                let input = part
                    .and_then(|p| p.get("input"))
                    .or_else(|| part.and_then(|p| p.get("arguments")))
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                Some(format!(
                    "{{\"type\":\"assistant\",\"message\":{{\"id\":\"opencode-tool-msg\",\"role\":\"assistant\",\"content\":[{{\"type\":\"tool_use\",\"id\":{},\"name\":{},\"input\":{}}}]}}}}",
                    serde_json::to_string(tool_id).unwrap_or_else(|_| "\"opencode-tool\"".into()),
                    serde_json::to_string(tool_name).unwrap_or_else(|_| "\"tool\"".into()),
                    serde_json::to_string(&input).unwrap_or_else(|_| "{}".into())
                ))
            }

            "tool_result" => {
                let tool_id = part
                    .and_then(|p| p.get("toolID"))
                    .or_else(|| part.and_then(|p| p.get("id")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let content = part
                    .and_then(|p| p.get("output"))
                    .or_else(|| part.and_then(|p| p.get("content")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                Some(format!(
                    "{{\"type\":\"tool\",\"tool_use_id\":{},\"content\":{}}}",
                    serde_json::to_string(tool_id).unwrap_or_else(|_| "\"\"".into()),
                    serde_json::to_string(content).unwrap_or_else(|_| "\"\"".into())
                ))
            }

            "error" | "exception" => {
                let msg = value
                    .get("message")
                    .or_else(|| value.get("error"))
                    .and_then(|v| {
                        v.as_str()
                            .map(String::from)
                            .or_else(|| Some(v.to_string()))
                    })
                    .unwrap_or_else(|| "OpenCode error".to_string());
                Some(format!(
                    "{{\"type\":\"error\",\"error\":{{\"message\":{}}}}}",
                    serde_json::to_string(&msg).unwrap_or_else(|_| "\"OpenCode error\"".into())
                ))
            }

            _ => {
                // Unknown event — surface it as a raw passthrough so we can
                // see what OpenCode is actually emitting and refine this
                // translator. Logged with [opencode:unknown] for grepping.
                eprintln!("[opencode:unknown] {}", trimmed);
                Some(format!(
                    "{{\"type\":\"raw\",\"source\":\"opencode\",\"event\":{}}}",
                    trimmed
                ))
            }
        }
    }
}
