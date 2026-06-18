# OMP Migration Plan — OpenCode → oh-my-pi (OMP)

> **Decision record / engineering analysis — Status: PLAN ONLY, nothing implemented.**
> Produced 2026-06-17 by a verified multi-agent analysis: ground-truth of the `enterprise-harness`
> working tree + due-diligence on https://github.com/can1357/oh-my-pi, in response to the migration
> runbook. Every file:line reference was checked against the working tree.
> Companion artifact: `port-decision/` (feature-port decision engine).

---

# Migration Plan: Swap Operon Enterprise's Agent Engine from OpenCode to OMP (oh-my-pi)

**Status: PLAN ONLY — no implementation proposed.** All file/line references verified against the working tree on branch `enterprise-harness`. Source lives at the git root `operon-enterprise/`, not the `port-decision/` analysis directory.

---

## 1. Executive reframe

The runbook is written for an architecture Operon does not have. **There is no `@opencode-ai/sdk`, no HTTP, no SSE, no WebSocket** to the engine (verified: the only `reqwest` callers are `knowledge.rs`/`extensions.rs`, unrelated). OpenCode is already a **one-shot CLI emitting line-delimited JSON to stdout**, invoked behind an **existing abstraction** — the `HarnessAdapter` trait.

What this means for scope:

- **The abstraction already exists.** `src-tauri/src/harness/mod.rs:84-97` defines `HarnessAdapter { id, build_command, normalize_line, capabilities }`. The module doc (mod.rs:14-16) explicitly says `pick()` is the single seam "so future adapters … plug in without touching `start_agent_session`." You do **not** introduce an `AgentEngine` layer — you implement the one that's there.
- **The transport is already stdio NDJSON.** The runbook's "central task = replace SSE/SDK with stdio RPC" is moot. Spawn/SSH/tail/shared-FS plumbing in `agent.rs` is engine-agnostic.
- **The RPC sidecar the runbook recommends is the wrong shape.** Operon's marquee path is a remote compute-node one-shot command whose shared-FS `.jsonl` is tailed over a *second* SSH hop, with a `.done` sentinel for completion. A persistent local `omp --mode rpc` process cannot be tailed across the login/compute split. **One-shot `omp --mode json` is the correct mode.**

**Net scope:** The migration is overwhelmingly "add `src-tauri/src/harness/omp.rs` implementing `HarnessAdapter`, register it in `pick()`, and generalize ~6 hardcoded-`"opencode"` couplings in `agent.rs` that live *outside* the trait." The defining constraint is **remote HPC execution**, not a local sidecar. The single largest *new* risk the runbook omits entirely is **getting the `omp` binary onto a no-admin Linux compute node** — see §4.

---

## 2. The linchpin decision: does OMP support a one-shot JSON-streaming run?

**Answer: YES (high confidence).** `omp --mode json "<prompt>"` executes a single prompt, streams the full `AgentSessionEvent` stream to stdout as newline-delimited JSON, then exits. It requires **no TTY** (only interactive mode does), so it works over SSH/pipes on an HPC node. This is **near-drop-in** for Operon.

**Why this fits the HPC model exactly.** The caller wraps the adapter's command verbatim (agent.rs:529-537) as:

```
{REMOTE_PATH_PREFIX}cd '{remote_path}' && {agent_cmd} > '{output_file}' 2>&1; echo $? > '{done_file}'{cleanup}
```

`omp --mode json` satisfies every hard constraint this imposes:
- **Single shell command** — yes, one invocation.
- **No TTY / no stdin block** — confirmed; remote SSH is spawned without `-tt` (agent.rs:916) and stdin is `< /dev/null` for the non-terminal path (agent.rs:903).
- **Line-delimited JSON to a redirected file, tailed by `stdbuf -oL tail -f`** — yes, JSONL output. *(Must verify per-line flushing on the real binary; block-buffering is the known "thinking but not responding" failure.)*
- **Completion via `.done` sentinel** (no exit-code channel over the tail) — `omp` just needs to terminate cleanly so `; echo $?` runs.
- **Runs in the user's own shell** to preserve aliases/conda/modules (terminal mode injects via `source '{script}'`, agent.rs:585-602) — fine for a binary on PATH.

**The two contingency points (NOT blockers, but must be resolved before relying on resume):**

1. **Resume flag for `--mode json` is undocumented.** `docs/session.md` documents only internal `resolveResumableSession(sessionArg, cwd)` / `continueRecent()` APIs and explicitly does **not** name a CLI flag (no `--resume`/`--continue`/`--session`/`-c` confirmed). **Contingency:** confirm the exact resume syntax from OMP's CLI arg-parser source in the Phase 0 spike. If no clean per-turn resume flag exists for `--mode json`, fall back options in priority order: (a) positional session-id arg if supported; (b) `--no-session` + Operon re-feeds prior transcript as context (degraded but workable); (c) **keep OMP local-only, OpenCode remote** as a transitional split (the `pick()` seam supports per-context selection).

2. **`--mode rpc` is explicitly rejected** for the remote path: it is a *persistent* stdin/stdout server (`{"type":"ready"}` on start, exits on stdin close), which has no per-turn spawn model and cannot be file-tailed across the login/compute split. It could in theory back a *local-only* fast path later, but that is out of scope for this migration and not the recommendation.

**Verdict on the linchpin: GREEN for one-shot streaming; YELLOW only on the resume flag, which is a spike-resolvable unknown, not an architecture problem.**

---

## 3. Target design: the `OmpAdapter`

New file: `src-tauri/src/harness/omp.rs`, modeled on `src-tauri/src/harness/opencode.rs` (624 lines, the reference implementation). Register in `src-tauri/src/harness/mod.rs:21-23` (`pick()`), today hardcoded to `Box::new(opencode::OpenCodeAdapter::new())`.

### 3a. `build_command(ctx: &BuildContext) -> BuildOutput`

OpenCode builds `"opencode run --format json --thinking"` (opencode.rs:378), appending `--model {m}` (opencode.rs:385) and `--session {resume}` (opencode.rs:388). The OMP analog:

- **Base invocation:** `omp --mode json` with `--no-session` for fresh turns (per the canonical automation example), `--model <provider>/<modelId>` passing `ctx.model` through unchanged (format is compatible — both use `provider/model`). The leading bare token must be `omp ` so the resolver can absolutize it (see §3e).
- **Resume:** when `ctx.resume_session` is `Some`, append the **(to-be-confirmed)** OMP resume flag/positional instead of `--no-session`. **This is the one field that cannot be finalized from docs** — resolve in Phase 0.
- **Mode → OMP role/flags mapping** (modes are *Operon conventions*, not engine features; `build_command` owns all mode semantics today — safety prefixes opencode.rs:214-224, plan formatting :242-268, report rules :269-308, headless-plotting rules :325-362):

  | Operon mode | OMP mapping |
  |---|---|
  | `agent` | default role (no role flag); keep headless/non-interactive prompt rules so nothing blocks on stdin/display |
  | `plan` | `--plan` (plan model role) + Operon's `implementation_plan.md` prompt scaffolding injected as prompt text (plan mode is a model-role swap in OMP — verify it actually restricts writes; if not, Operon's plan semantics stay prompt-driven as today) |
  | `ask` | structured prompt; OMP's `ask` tool (structured picker) exists but is agent-invoked mid-turn, not a launch flag — keep Operon's ask semantics in the prompt prefix |
  | `report` | `commit` role (OMP's changelog/commit role) + report rules; prompt-via-temp-file path preserved (see below) |

- **Hard requirements carried over:** single shell command; line-delimited JSON to stdout; prompt single-quote-escaped (`.replace('\'', "'\\''")`, opencode.rs idiom); `BuildOutput.prompt_file` for report mode reconstructed deterministically from `session_id` (e.g. `temp_dir/operon-report-prompt-{session_id}.txt`) so the caller can SCP it (agent.rs:442-449, :838-845).
- **Headless safety:** keep the non-interactive/headless-plotting prompt rules (opencode.rs:325-362) — they exist precisely because anything that blocks on a TTY/display hangs the redirected one-shot run.

### 3b. `normalize_line(raw) -> Option<String>` — the real contract surface

This is the single most engine-specific method. It maps OMP's `AgentSessionEvent` stream → Operon's **exactly 5 canonical shapes** (`ChatPanel.tsx` understands only these, plus `raw`/`heartbeat` passthrough). Proposed mapping from OMP events:

| OMP event | Operon canonical event |
|---|---|
| `agent_start` (carries run/session id) | `system` → `{"type":"system","session_id":"<id>"}` — frontend persists this for resume (ChatPanel 1674-1681). **Must locate where OMP surfaces its own session id** (agent_start payload or first message) — spike item. |
| `message_update` w/ `assistantMessageEvent.type == "text_delta"` (`.delta`) | `assistant`/text → `{"type":"assistant","message":{"id":"<msgId>","role":"assistant","content":[{"type":"text","text":"..."}]}}`. **Delta-accumulation gap:** OMP streams *deltas*; Operon's dedup expects **cumulative** content per `msgId` (same id replaces, new id appends — ChatPanel seenMsgIds). The adapter must accumulate deltas per OMP message id and emit the running total, OR emit per-delta with a stable id and rely on replace semantics. Choose accumulation (closer to OpenCode's cumulative model). |
| `message_update` w/ `thinking_delta` (`.delta`) | `assistant`/thinking → same shape with `id: "<msgId>-thinking"` and `content:[{"type":"thinking","thinking":"..."}]` (distinct suffix so it doesn't collide with text). |
| `tool_execution_start` (`toolName`, params) / `toolCall` | `assistant`/tool_use → `{"type":"assistant","message":{"id":"<msgId>","role":"assistant","content":[{"type":"tool_use","id":"<toolId>","name":"<tool>","input":{...}}]}}` |
| `tool_execution_end` (`isError`, result) | `tool` → `{"type":"tool","tool_use_id":"<toolId>","content":"<output>"}` (matched by `tool_use_id`, sets status complete). |
| `message_end` (`usage`, `stopReason`) / `turn_end` / `agent_end` | `result` → `{"type":"result","session_id":"...","subtype":"<stopReason>","usage":{...},"total_cost_usd":<n>}`. Map OMP `stopReason` (`stop`/`toolUse`/`length`/`error`/`aborted`) → `subtype`. **Exact `usage` field names (inputTokens/outputTokens/cacheRead) are unverified** — resolve from a captured stream. |
| error frames / `auto_retry_*` failures | `error` → `{"type":"error","error":{"message":"..."}}` |
| `auto_compaction_*`, `ttsr_triggered`, `todo_reminder`, unknown | passthrough `raw` → `{"type":"raw","source":"omp","event":<verbatim>}` and `eprintln!("[omp:unknown] …")` for iteration (opencode.rs:612-621 idiom). **Never drop.** |
| non-JSON lines | `{"type":"raw","source":"omp","line":<escaped>}` (opencode.rs:456-459 idiom). |

`heartbeat` is **not** emitted by the adapter — it is injected by the SSH tail script every 30s (agent.rs:642). No change needed.

### 3c. `capabilities()`

OpenCode declares `{resume:true, plan_mode:true, max_turns:false, mcp:false}` (opencode.rs:196-201). OMP proposal: `{resume: <true iff resume flag confirmed>, plan_mode:true, max_turns:false, mcp:true}` — OMP has first-class MCP (§5), so the MCP toggle can light up. Set `resume:false` until the flag is confirmed; the frontend greys the resume toggle accordingly, which is the honest UX until Phase 0 resolves it.

### 3d. Config generation — the `ensure_default_config` analog

`agent.rs:316-330` calls `opencode::ensure_default_config(&project_path, &model)` + `ensure_operon_plan_agent(&project_path)` **by name, behind `if remote.is_none()`** (local-only). These write `opencode.json` (provider/baseURL for OpenAI-compatible Ollama/vLLM). OMP needs an analog that writes **`~/.omp/agent/models.yml`** (and/or project `.omp/`):

- A `providers:` entry — `local-openai: {baseUrl: http://127.0.0.1:8000/v1, auth: none, api: openai-completions, models: [...]}` for vLLM/local-GPU, or rely on **keyless auto-discovery** for Ollama (`:11434`)/llama.cpp/LM Studio/LiteLLM.
- `settings.modelRoles` (default/smol/slow/plan/commit) to back the mode→role mapping in §3a.
- **Provider config does NOT auto-carry from `opencode.json`** — OMP ingests opencode *skills/MCP*, not provider/baseURL blocks. This must be authored fresh.

**Decision:** rather than hardcoding a second by-name call in `agent.rs:316-330`, add a trait method (e.g. `ensure_local_config(project_path, model) -> Result<()>`) so the bootstrap is driven off the adapter, not the literal engine. This is the cleanest place to also tackle the **remote-config TODO** (agent.rs:311-315): remote sessions today assume pre-placed config; OMP remote sessions will need `models.yml` pushed to the node (a new remote-bootstrap step) unless keyless local-model discovery covers the cluster's setup.

### 3e. `pick()` / feature flag for parallel run + rollback

- **Selection:** extend `harness::pick()` (mod.rs:21-23) to choose `OmpAdapter` vs `OpenCodeAdapter` from a setting (e.g. `AppSettings.agent_engine: "opencode" | "omp"`, default `opencode`). This is the rollback switch and the shadow-mode lever. `pick()` is the *only* construction point, so this is a one-function change for selection itself.
- **Caveat:** `pick()` returns the trait object but the **non-trait couplings in `agent.rs` must also branch on `adapter.id()`** (see §3f), or selection alone silently breaks remote/config paths. Generalizing those is the real wiring work.

### 3f. Non-trait couplings to generalize (the part the runbook misses)

These hardcode the literal string `"opencode"` and must be driven off `adapter.id()`:

- `agent.rs:316-330` — local config bootstrap (calls `opencode::ensure_default_config` / `ensure_operon_plan_agent` by name). → trait method (§3d).
- `agent.rs:77-78` — `REMOTE_PATH_PREFIX` exports `$HOME/.opencode/bin:$HOME/.local/bin:$HOME/.npm-global/bin:$PATH`. → add `$HOME/.local/bin` already present (good — that's where omp.sh installs), but make the prefix engine-aware if omp uses a different dir.
- `agent.rs:786-820` — remote resolver `find_agent_cmd` probes `…/opencode` at 8 fixed locations + NVM glob + `which opencode`. → generalize the probed binary name to `adapter.id()` (omp installs to `~/.local/bin/omp`, which the loop already covers structurally).
- `agent.rs:827-831` — install hint `curl -fsSL https://opencode.ai/install | bash`. → engine-specific hint: `curl -fsSL https://omp.sh/install | sh`.
- `agent.rs:897` — `agent_cmd.replacen("opencode ", &format!("{} ", agent_invoke), 1)`. → replace the leading `adapter.id() + " "` token.

---

## 4. Remote/HPC install strategy — flag as the highest practical risk

**This is the section the runbook omits and the single biggest unsolved risk.** OMP must land on a **no-admin Linux compute node** as a non-interactive CLI with **no Bun/Node runtime present**.

**Good news (verified):**
- OMP ships as a **single self-contained binary per platform** (`bun build --compile`, embeds Bun + the N-API addon — **no Bun/Node at runtime**). Linux assets: `omp-linux-x64` (~158MB, `bun-linux-x64-baseline` = no AVX2, ideal for old/heterogeneous HPC CPUs) and `omp-linux-arm64` (~137MB, for Grace/Graviton/Ampere).
- **No-admin install:** `curl -fsSL https://omp.sh/install | sh` detects os/arch, downloads **one** binary to `~/.local/bin` (override `PI_INSTALL_DIR`), `chmod +x`. No root, no Bun. `~/.local/bin` is already in `REMOTE_PATH_PREFIX` (agent.rs:78) **and** already a probed resolver location (agent.rs:791) — so a binary named `omp` there is found with zero resolver change beyond the binary name.
- **Apple-Silicon sidecar (desktop):** bundle `omp-darwin-arm64` (~115MB) / `omp-darwin-x64` as a Tauri sidecar — self-contained, no Rosetta, ship matching arch.
- **Air-gapped:** pre-download the exact `omp-<os>-<arch>` asset from a pinned release tag on a connected machine, copy to the node, `chmod +x`. It is the **sole artifact** (no companion downloads). Installer does **no checksum** — verify integrity manually.
- **This eliminates the npx/npm-alias approach** entirely; the `claude`/`opencode → npx …` alias pattern is replaced by one ~150MB binary on the shared filesystem.

**The unresolved risk (HIGHEST PRACTICAL RISK):**
- The prebuilt linux binaries **declare no libc field**; whether they are **glibc-only (vs musl/Alpine) and the minimum glibc floor are UNVERIFIED.** HPC login/compute nodes often run old RHEL/CentOS glibc. **If the binary won't run, the entire remote path is dead.** Mitigation: **Phase 0 must `ldd ./omp-linux-x64` / just execute it on the actual target cluster** before any further work. This is the gating de-risk test.
- `--external mupdf` at build time means PDF tooling may expect mupdf externally; **likely affects only PDF features, not the core agent** — record but don't block.
- No Windows-arm64 binary exists (x64 only) — irrelevant for HPC, note for desktop.

**Avoid on HPC:** `bun install -g @oh-my-pi/pi-coding-agent` / `--ref` source mode (needs Bun ≥1.3.14 present, pulls a per-platform `pi-natives` ~233MB-unpacked N-API package). **`pi-natives` is an N-API addon, not a crates.io crate — it cannot be a Cargo dependency in Operon's Tauri backend.** Consume the `omp` binary exactly as `opencode` is consumed today.

---

## 5. Config, MCP, protocols, guardrails mapping

**Carries over with little/no work:**
- **MCP** (near-drop-in): same `{mcpServers:{...}}` JSON shape (stdio command/args/env; http/sse url/headers). OMP's universal config discovery explicitly ingests MCP from `.claude/.cursor/.vscode/opencode.json` — **no migration script.** Operon's existing MCP servers carry over. Capabilities can flip `mcp:true`.
- **Skills:** `.claude/skills` (priority 80) and opencode skills (priority 55) are both ingested. OMP `SKILL.md` uses YAML frontmatter (name/description/globs/alwaysApply). Operon's bioinformatics protocol prompt-text can become `SKILL.md` skills.
- **`AGENTS.md`:** read by OMP's workspace walker (`collectAgentsMd`). Operon's `AGENTS.md`/`CLAUDE.md` carry as workspace context. *(Caveat: this repo's `AGENTS.md` is architecture documentation, "reusable as-is" is trivially true.)*

**Must verify before relying on it:**
- **`.claude` RULE files** (vs skills/MCP): OMP's own docs **conflict** — README claims broad `.claude` inheritance, but `rulebook-matching-pipeline.md` lists rule providers **without** `.claude` and notes `.claude`/`AGENTS.md`/`CLAUDE.md` "appear absent" from the rule pipeline. **Verify from source**; if rules don't carry, re-author lab rules as `.omp/rules/*.md` or `instructions/`.
- **CLAUDE.md ingestion** asserted only by a secondary (DeepWiki) source — verify.

**Must be rebuilt (but primitives are stronger):**
- **Guardrails (`rm -rf` / `sbatch` gates).** OpenCode/Operon have **no** native programmable tool-blocking. OMP gives two new primitives, both net-new code but directly enabling the lab guardrails:
  - **Hooks** (TS modules, `pi.on('tool_call', …)` returning `{block:true, reason}`): block `bash` containing `rm -rf`; gate `sbatch` via `ctx.ui.confirm`. `tool_result` hooks can redact output. Discovered via `.omp/hooks/pre/*.ts` or `--hook`/`--extension`.
  - **TTSR** (declarative time-traveling stream rules): regex/ast-grep conditions that interrupt mid-generation and re-inject — good for *soft* guardrails (style/policy nudges).
  - **Recommendation:** adopt **hooks for hard blocks** (`rm -rf`, destructive paths, `sbatch` confirmation) and **TTSR for soft policy**. Note hooks are TS files that must be **deployed to the remote `.omp/`** — a new artifact in the remote-bootstrap step.
- **Provider config** (`models.yml`) — re-authored, not imported (§3d).
- **Read-only mode:** OMP has **no first-class read-only agent mode.** Reconstruct from `approvalMode: always-ask` + per-tool `tools.approval.<tool>=deny` on write/exec, or a read-only hook. Operon's `permission_mode` (`full_auto`/`safe_mode`/`supervised`) maps onto OMP's `yolo`/`write`/`always-ask` tiers.

---

## 6. Phased plan (adapted to the existing architecture)

**Phase 0 — De-risk spike (local + cluster smoke test).** Goal: prove the linchpin and kill the top risks before writing the adapter.
- Run `omp --mode json "<prompt>"` locally; capture the raw `AgentSessionEvent` stream to a fixture file. Confirm: per-line flushing, JSONL (no pretty-print), exit code semantics, where the session/run id appears.
- Resolve the **resume flag** for `--mode json` from OMP CLI source.
- Capture verbatim `usage` and `tool_execution_end` field names.
- **On the actual HPC cluster:** copy `omp-linux-<arch>`, `chmod +x`, `ldd`/run it → confirm glibc compatibility. **Hand-run** `cd '<nfs path>' && omp --mode json '<prompt>' > out.jsonl 2>&1; echo $? > out.done` from a compute node and `tail -f` from the login node.
- **Exit criteria:** OMP one-shot JSONL stream captured locally; resume syntax known; `omp` binary confirmed executable on the target cluster and tail-streamable over the login/compute split; usage/tool field names documented. **If glibc fails or no resume flag exists → STOP and re-evaluate (no-go or local-only split).**

**Phase 1 — `OmpAdapter` behind `pick()` flag (local only).** Implement `src-tauri/src/harness/omp.rs` (`build_command`, `normalize_line`, `capabilities`, `ensure_local_config` trait method). Add `agent_engine` setting; branch `pick()`. Generalize the non-trait `agent.rs` couplings off `adapter.id()` (§3f). Drive `models.yml` generation for local Ollama/vLLM.
- **Exit criteria:** with `agent_engine="omp"`, a **local** agent/plan/ask/report turn renders correctly in `ChatPanel` (all 5 canonical event types: text, thinking, tool_use, tool_result, result); switching back to `"opencode"` is a clean rollback; no `[omp:unknown]` events for common turns.

**Phase 2 — Remote/HPC path.** Wire the generalized resolver + install hint + `REMOTE_PATH_PREFIX` for `omp`; add remote `models.yml`/hooks bootstrap (closing the agent.rs:311-315 TODO for OMP). Validate the full inject-into-terminal + SSH-tail + `.done`-sentinel flow on the cluster.
- **Exit criteria:** a remote HPC agent turn streams end-to-end via the tail; resume (re-tail running, hydrate completed via `read_session_output`) works across an app restart; ControlMaster/Duo-MFA resilience and 30s heartbeats unaffected; guardrail hooks block `rm -rf` and gate `sbatch` on the remote.

**Phase 3 — Parity matrix + shadow.** Build an explicit OpenCode-vs-OMP parity matrix (modes × resume × MCP × plan × guardrails × cost/usage rendering). Run shadow comparisons on representative bioinformatics tasks (same prompt, both engines, diff the transcripts/outputs).
- **Exit criteria:** documented parity table with no P0 gaps; any gap has an owner + workaround; cost/usage numbers render correctly; protocol/skill/MCP carryover verified empirically (esp. `.claude` rule ingestion).

**Phase 4 — Cutover + soak.** Flip default `agent_engine` to `omp` for opt-in users; keep `opencode` selectable for rollback. Pin a specific `omp` binary release (download asset from a fixed tag — `--ref` switches to source mode, so pin by URL, not flag). Soak on real lab workloads.
- **Exit criteria:** N days of soak with zero P0 regressions; rollback exercised at least once; binary version pinned + vendored; runbook updated.

---

## 7. Risk register

| # | Risk | Sev | Source | Mitigation |
|---|---|---|---|---|
| R1 | **Linux binary glibc/musl floor undeclared** — may not run on old HPC nodes | **Critical** | Operon-specific (runbook omits) | Phase 0 `ldd`/exec on actual cluster; if fails, no-go for remote or use a node with newer glibc / build-from-source path |
| R2 | **Resume flag for `--mode json` undocumented** | High | Operon-specific | Phase 0 source check; fallback to positional/`--no-session`+context replay; ship `resume:false` capability until confirmed |
| R3 | **Single-maintainer hard fork, ~6mo old, v16.x, multiple releases/day** — bus-factor + API instability | High | Health findings | Pin/vendor a fixed binary release; budget in-house maintenance; keep OpenCode (and the `pick()` seam) as a live fallback; migration path to upstream Pi |
| R4 | **Delta-vs-cumulative stream mismatch** — OMP streams deltas, Operon dedup expects cumulative per msgId | Medium | Adapter design | Accumulate deltas per OMP message id in `normalize_line`; cover in Phase 1 exit criteria |
| R5 | **Block-buffering** ("thinking but not responding") if `omp` doesn't flush per line | Medium | HPC constraint | Verify per-line flush in Phase 0; `stdbuf` already in tail; if needed wrap with `stdbuf -oL` on the omp side |
| R6 | **Remote config not auto-bootstrapped** (agent.rs:311-315 TODO; provider config doesn't carry from opencode.json) | Medium | Ground truth | New remote `models.yml`/hooks push step in Phase 2 |
| R7 | **`.claude` rule-file ingestion contradicted in OMP docs** | Medium | Config findings | Verify from source in Phase 3; re-author as `.omp/rules` if absent |
| R8 | **Non-trait `"opencode"` couplings missed** → silent remote/config breakage | Medium | Ground truth (runbook omits) | §3f checklist; gate Phase 2 on all 6 generalized |
| R9 | **No TTY/Bun on compute node** | Low (resolved) | Distribution findings | `--mode json` needs no TTY; binary is self-contained — confirmed, just verify in Phase 0 |
| R10 | **No first-class read-only/plan write-restriction** | Low | Config findings | Reconstruct from `approvalMode`+per-tool deny / read-only hook; keep plan semantics prompt-driven |
| R11 | **MIT compliance** — two copyright holders | Low | Health findings | Preserve both "Mario Zechner" and "Can Bölük" notices when vendoring |

---

## 8. Interaction with the in-flight feature-port roadmap

The `port-decision/` engine (`engine.py`, `candidates.json`, `DECISIONS.md`) scores feature ports that are **mostly engine-agnostic** — they live in `agent.rs`/`ChatPanel.tsx`/SSH plumbing, which survive the swap. Specifically:

- **Engine-agnostic ports survive unchanged:** anything touching the SSH/tail/shared-FS plumbing, session metadata, reconnect, FileViewer, terminal, etc. The `HarnessAdapter` boundary is exactly why these are insulated.
- **Provider-gateway** port is already marked **Skip** — consistent with this plan (OMP handles providers via `models.yml`, not a gateway).
- **SLURM "register, don't poll" prompt injection** is the one port that *touches the engine*: today it would be a prompt prefix in `opencode.rs`. Under OMP it moves to either the **OMP system-prompt/instructions** (`.omp/instructions` / `AGENTS.md`) or a **TTSR rule** that nudges the agent away from blocking `squeue` polling. Re-target that port's insertion point from "opencode.rs prefix" to "OMP system-prompt or TTSR."
- Any port that assumed an OpenCode-specific flag must be re-expressed against OMP roles/flags (§3a) — flag those in `DECISIONS.md` as engine-coupled.

Recommend tagging each candidate in `candidates.json` with an `engine_coupled: bool` so the swap's blast radius on the roadmap is explicit.

---

## 9. Open questions (runbook §15 resolved where code now answers it)

**Resolved by the code:**
- *"Where does the OpenCode client live?"* → There is no client/SDK. It's the `HarnessAdapter` trait (`src-tauri/src/harness/mod.rs`) + `opencode.rs` adapter, driven by `start_agent_session` in `src-tauri/src/commands/agent.rs`. Frontend parser is `src/components/chat/ChatPanel.tsx`.
- *"Replace SSE/SDK transport with stdio RPC?"* → No transport to replace; it's already one-shot CLI → stdout NDJSON. Write an adapter, not a transport.
- *"Introduce an engine abstraction?"* → Already exists; implement it.
- *"Depend on OMP Rust crates / pi-natives?"* → Infeasible; pi-natives is an N-API addon, not a Cargo crate. Consume the `omp` binary as a sidecar/CLI.
- *"RPC sidecar?"* → Wrong fit for the remote tail model; use `--mode json` one-shot.
- *"Remote/multi-client HTTP needed?"* → **No.** Ground truth: no HTTP anywhere in the engine path; the remote model is one-shot-command + second-SSH-tail + shared-FS `.jsonl`. No HTTP server should be introduced.
- *"Are model strings identical?"* → `provider/model` format is compatible (pass-through), but the **CLI flag and provider config differ** — `build_command` is rewritten and `models.yml` is re-authored.

**Still needs a human decision:**
1. **Resume CLI syntax for `--mode json`** — must be read from OMP source (Phase 0). Blocks the resume capability flag.
2. **Glibc/musl compatibility on the specific cluster(s)** — must be tested on the real node (Phase 0). Gating.
3. **Default engine policy** — opt-in flag indefinitely, or hard cutover? (recommend opt-in → soak → default).
4. **Guardrail depth** — adopt hooks/TTSR now, or ship parity-only first and add guardrails as a fast-follow? (recommend hooks for `rm -rf`/`sbatch` in Phase 2 since OMP makes it cheap and the lab wants it).
5. **`.claude` rule carryover** — verify, then decide whether to migrate lab rules into `.omp/rules` proactively.
6. **Version-pinning policy** — given multiple releases/day, pick a cadence for re-pinning the vendored binary.

---

## 10. Recommendation

**CONDITIONAL GO.** The architecture strongly favors this migration: the abstraction seam already exists, the transport is already the right shape, OMP's `--mode json` is a near-drop-in one-shot JSONL engine that needs no TTY, the install story is a single self-contained no-admin binary that lands exactly where Operon's resolver already looks (`~/.local/bin`), and MCP/skills carry over with no migration script. OMP is also a *superset* on providers and uniquely unlocks the lab's `rm -rf`/`sbatch` guardrails via hooks/TTSR — something OpenCode cannot do.

The "go" is conditional on **two cluster-testable unknowns** (binary glibc compatibility, and the `--mode json` resume flag) plus accepting a real **single-maintainer bus-factor risk** mitigated by binary pinning, the live OpenCode fallback through `pick()`, and an in-house maintenance budget.

**Single biggest de-risking step to do first:** On the **actual HPC cluster**, copy `omp-linux-<arch>`, `chmod +x`, run `ldd`/execute it, and hand-run `cd '<nfs-path>' && omp --mode json '<prompt>' > out.jsonl 2>&1; echo $? > out.done` from a **compute node** while `tail -f`-ing `out.jsonl` from the **login node**. If that one experiment works, every other risk is engineering work behind a feature flag; if it fails (glibc/musl), the remote path — Operon's entire reason for existing — is blocked and the decision changes to local-only or no-go.

---

**Key file references for implementation (all absolute):**
- `src-tauri/src/harness/mod.rs` — trait + `pick()` (lines 21-23, 84-97)
- `src-tauri/src/harness/opencode.rs` — reference adapter (command at :378, capabilities :196-201) → model the new `omp.rs` here
- `src-tauri/src/commands/agent.rs` — non-trait couplings to generalize: `REMOTE_PATH_PREFIX` (:77-78), local config bootstrap (:316-330), remote resolver + install hint (:786-831), `replacen("opencode ", …)` (:897)
- `src/components/chat/ChatPanel.tsx` — canonical-event consumer (the `normalize_line` contract surface)
- `port-decision/{engine.py,candidates.json,DECISIONS.md}` — feature-port roadmap to tag with `engine_coupled`
