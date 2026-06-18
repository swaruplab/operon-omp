# Operon Enterprise v0.1.0 — first release

**Repo:** `swaruplab/operon-enterprise`
**Branch:** `enterprise-harness` → `main`
**Diff vs base Operon (v0.6.0):** 28 files changed, +538 / −8068
**Theme:** First cut of the enterprise fork. Replaces the built-in proprietary agent runtime that ships with base Operon with the open-source OpenCode CLI. Strips everything that was specific to the old runtime (vendor-API translation proxy, vendor env vars, vendor-shaped streaming events). Introduces a trait-based harness abstraction so additional adapters can slot in later without touching the UI.

This is `v0.1.0` because it is the first tagged release of the enterprise SKU. The base project (`swaruplab/operon`) continues its own version line independently.

---

## Backend (Rust)

### Harness layer (new)

- `src-tauri/src/harness/mod.rs` — defines the `HarnessAdapter` trait + the canonical `AgentEvent` shape used internally. The trait carries `spawn_cmd()`, `parse_line()`, and a `Caps` descriptor (resume support, plan mode, tool format).
- `src-tauri/src/harness/opencode.rs` (470 lines) — first concrete adapter. Translates OpenCode's `--format stream-json` line-protocol into the shared `AgentEvent` shape so the rest of the app does not need to know which CLI is in use. Includes session-id capture, tool-call/tool-result mapping, and plan-mode hand-off.

### Commands

- **Renamed** `src-tauri/src/commands/claude.rs` (3550 lines) → `src-tauri/src/commands/agent.rs` (1769 lines).
  - Bulk-renamed `ClaudeSession` / `claude_shell` / `claude_cmd` / `find_claude_cmd` / `claude_resolve` → `AgentSession` / `agent_shell` / `agent_cmd` / `find_agent_cmd` / `agent_resolve`.
  - Replaced the SSH-side bash script that hunts for the agent binary on remote HPC nodes — it now scans `$HOME/.local/bin/opencode`, `$HOME/.opencode/bin/opencode`, `.npm-global/bin/opencode`, NVM paths, and `which opencode`. Alias-detection logic dropped (no longer needed since the new runtime resolves to a regular binary).
  - Updated install error message to point at `https://opencode.ai/install`.
  - Comment sweep: every reference to the old runtime in inline comments updated.
- **Deleted** `src-tauri/src/commands/proxy.rs` (206 lines) — the in-app translation-proxy commands. The new runtime speaks OpenAI-compatible natively; no sidecar required.
- **Deleted** dead `claude_mcp_add()` / `claude_mcp_remove()` / `sync_mcp_servers_to_claude()` from `commands/mcp.rs` (and 5 in-tree callers).
- **New** `src-tauri/src/commands/setup.rs` (210 lines) — first-run setup wizard backend. Three Tauri commands probe the host:
  - `check_opencode` → `opencode --version` + `command -v`
  - `check_ollama`   → `ollama --version` + `command -v`
  - `check_vllm`     → `python -c 'import vllm; print(vllm.__version__)'`
  - Two installer commands: `install_opencode` (curl from `opencode.ai/install`), `install_ollama` (Homebrew first on macOS, curl fallback on macOS/Linux, manual instruction on Windows).
  - `complete_setup` flips `settings.setup_completed = true` and persists.
- `commands/files.rs` — `generate_protocol` and `generate_protocol_from_files` rewritten to shell out to `opencode run --format text --model '{model}'` with the prompt piped via base64 + stdin. Both now take `tauri::State<SettingsManager>` so they can read the configured model.
- `commands/watchdog.rs` — comment sweep, no behavioural change.

### Settings

- `commands/settings.rs` — added `setup_completed: bool` (default `false`, `#[serde(default)]` for old config files). Removed BYOE provider fields and translation-proxy state. Kept `model` migration logic that resets stale stored model ids to the new default `ollama/kimi-k2.6:cloud` so existing user installs don't show an unrunnable model in the dropdown.

### Platform layer

- `platform/mod.rs` — deleted dead wrapper functions `install_node_platform` / `install_claude_platform` / `install_git_platform` / `refresh_path` / `persist_git_bash_env`. The setup-wizard backend in `commands/setup.rs` is now the single install entry point.
- `platform/macos.rs` — deleted `install_claude_platform`, `install_node_tarball`, `install_node_platform`, `find_npm`. Switched `extra_tool_paths()` from `~/.claude/local/bin` to `~/.opencode/bin`.
- `platform/linux.rs` — deleted `install_claude_platform`. Same path swap.
- `platform/windows.rs` — deleted `persist_git_bash_env` and `install_claude_platform`. Removed all `CLAUDE_CODE_GIT_BASH_PATH` env-var injection from `open_terminal_with_command()`. Path swap.

### Wiring

- `src-tauri/src/lib.rs` — re-sorted the `commands::{...}` use block, registered the six new setup commands in `tauri::generate_handler![...]`. Removed translation-proxy command registrations.
- `src-tauri/src/commands/mod.rs` — added `pub mod setup;` and re-exports.

---

## Frontend (React/TypeScript)

### App gating (new behaviour)

- `src/App.tsx` — first-run gate. On mount, fetch settings; if `setup_completed === false`, render `<SetupWizard onComplete={...} />` instead of the main shell. Falls open on settings read errors so the user is never locked out.

### Setup wizard (rewritten)

- `src/components/setup/SetupWizard.tsx` — now a focused 3-tool checklist: OpenCode (required), Ollama (recommended), vLLM (advisory). Layout features:
  - Per-tool status icon (loader / green check / yellow warn / gray circle), version string when installed.
  - One-click Install buttons for OpenCode and Ollama.
  - vLLM is intentionally not installable from the wizard — it's a GPU-server Python package. The card shows a manual hint `pip install vllm && vllm serve <model>` and a docs link.
  - "Re-check", "Skip for now", and "Finish setup" footer. Finish is disabled until OpenCode resolves; the other tools are non-blocking.
- Old wizard (1963 lines, vendor-API key entry, OAuth flow, multi-step welcome) was deleted in favour of this leaner version.

### Wrappers

- **New** `src/lib/agent.ts` — typed Tauri command wrappers for the renamed harness.
- **Deleted** `src/lib/claude.ts`.
- `src/lib/settings.ts` — `AppSettings` interface gains `setup_completed: boolean` (default `false`). Removed translation-proxy and vendor-API fields.
- `src/types/chat.ts` — renamed `ClaudeEvent` → `AgentEvent`, kept the structure identical so message-rendering code in `ChatPanel` was a near-mechanical update.

### Panels (simplified)

- `src/components/chat/ChatPanel.tsx` — −960 lines. Cost-tracking UI dropped (the new runtime handles billing externally), vendor-API auth flow dropped, mode/model dropdown simplified.
- `src/components/settings/SettingsPanel.tsx` — −553 lines. Translation-proxy panel removed, vendor-API key entry removed, BYOE provider toggle removed. The "AI provider" section is now strictly OpenAI-compatible base URL + key + detect-models.
- `src/components/help/HelpPanel.tsx` — surgical edit. Kept the OpenRouter / LiteLLM / Ollama / LM Studio / vLLM / Custom-endpoint walkthroughs intact (still accurate). Stripped:
  - The "vendor-API vs OpenAI-compatible" overview framing.
  - The whole "Why the Translation Proxy exists" item.
  - "Translation proxy ON/OFF" guidance from every per-backend walkthrough.
  - The remote troubleshooting `*_BASE_URL / *_AUTH_TOKEN` env-var step (replaced with an OpenCode binary check).
  - Footer tagline updated to "Powered by OpenCode".

### Misc

- `TopBar.tsx`, `AppShell.tsx`, `TerminalArea.tsx`, `TerminalInstance.tsx`, `ProtocolsView.tsx`, `HelpView.tsx`, `ReportPhasePanel.tsx` — light edits, mostly string sweeps where the old runtime name appeared in tooltips or empty states.

---

## Architecture notes

- **Why a trait?** The base project hard-couples the agent runtime, the streaming-event shape, and the chat UI. The trait-based `HarnessAdapter` keeps the chat UI honest about what events it knows how to render (`AgentEvent`) and pushes per-CLI quirks (resume flag spelling, tool-call JSON shape, plan-mode marker file) into the adapter. Adding a second adapter later (Codex CLI, Goose, Aider) is now isolated to one new file under `harness/` plus a settings entry.
- **Why drop the translation proxy?** The previous runtime spoke a proprietary `/v1/messages` shape; we shipped a Rust sidecar that translated to/from OpenAI Chat Completions. The new runtime emits OpenAI Chat Completions natively, so the sidecar is dead weight. Scripts and CI workflow entries that build the proxy binary are still in-tree (`scripts/fetch-anthropic-proxy.sh`, `.github/workflows/*.yml`); they can be removed in a follow-up but are harmless.
- **HPC flow unchanged.** The `tmux + jsonl tail` pattern documented in `CLAUDE.md` (now misnamed but still architecturally accurate) carried over verbatim — it's runtime-agnostic. Only the binary spawned inside the tmux session changed.
- **Default model.** `ollama/kimi-k2.6:cloud` — OpenCode-style provider/model id, served by Ollama Cloud through a locally-running daemon. Existing installs with stale stored ids are silently migrated to this default on settings load.

---

## Verification

- `cargo check` clean on `src-tauri/`.
- `tsc --noEmit` clean on the frontend.
- Dev launch (`npm run tauri dev`) compiles in ~17 s and the window comes up.

---

## Files at a glance

```
deleted:    src-tauri/src/commands/claude.rs              (-3550)
deleted:    src-tauri/src/commands/proxy.rs               (-206)
deleted:    src/lib/claude.ts                             (-66)
new:        src-tauri/src/commands/agent.rs               (+1769)
new:        src-tauri/src/commands/setup.rs               (+210)
new:        src-tauri/src/harness/mod.rs                  (+97)
new:        src-tauri/src/harness/opencode.rs             (+470)
new:        src/lib/agent.ts                              (+33)
modified:   src-tauri/src/{lib,commands/{mcp,settings,files,watchdog,mod}}.rs
modified:   src-tauri/src/platform/{mod,macos,linux,windows}.rs
modified:   src/App.tsx
modified:   src/components/{chat,settings,help,setup,layout,sidebar,terminal,report}/...
modified:   src/{lib/settings,types/chat}.ts
```

Net: −8068 / +538. The big wins are deletions — three large vendor-coupled modules replaced with a ~2.6 KLOC trait-based equivalent that keeps the surface area thin.

---

## Suggested commit shape

If squashed into one commit:

```
v0.1.0: first Operon Enterprise release — OpenCode harness

- Introduce HarnessAdapter trait + opencode.rs adapter
- Rename ClaudeSession/claude_cmd/claude_shell -> Agent*
- Replace SSH remote-binary discovery for opencode
- Recreate first-run wizard (OpenCode + Ollama + vLLM)
- Remove translation proxy (no longer needed)
- Drop dead platform installers; switch ~/.claude paths to ~/.opencode
- Strip vendor-API framing from Help panel; keep external-provider docs
- Simplify ChatPanel and SettingsPanel
- Migrate stale model ids in user settings to ollama/kimi-k2.6:cloud
```

Alternative: split into a stack — `harness-trait`, `setup-wizard`, `help-docs`, `platform-cleanup`, `chat-settings-trim` — each lands cleanly on its own.
