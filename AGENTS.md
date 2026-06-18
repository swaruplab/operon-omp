# AGENTS.md — Operon-OMP

Contributor and AI-agent guide for **this** repository. Operon-OMP is Operon (a
Tauri 2 desktop IDE for HPC bioinformatics: Rust backend + React/TypeScript frontend)
with its agent engine swapped from OpenCode to **OMP (oh-my-pi)**. This file is short
and factual; for the full picture read `CLAUDE.md` (architecture) and
`OMP_MIGRATION_PLAN.md` (the engine swap).

## What runs the agent

This repo runs **OMP** (`@oh-my-pi/pi-coding-agent`, https://github.com/can1357/oh-my-pi,
MIT) — **not** Claude Code and **not** Codex. OMP is invoked as a **one-shot CLI**:

```
omp --mode json -p [--model provider/model] [--resume <id>]
    [--thinking minimal|low|medium|high|xhigh] [--auto-approve]
    [--append-system-prompt <text>] [--no-tools] ["<prompt>" | @<file>]
```

`--mode json -p` streams `AgentSessionEvent` objects as JSONL on stdout, needs **no
TTY** (works over SSH on a compute node), and exits when done. The first line is the
`session` header whose `.id` is the resume id. This is grounded against omp **v16.0.5**.

## The engine seam — do not reinvent it

There is exactly one extension point for engines: the **`HarnessAdapter` trait** in
`src-tauri/src/harness/mod.rs`, constructed via **`pick(engine)`**:

- `pick("opencode")` → `opencode::OpenCodeAdapter` (legacy, kept for rollback)
- anything else → `omp::OmpAdapter` (`src-tauri/src/harness/omp.rs`, the default)

The engine is chosen by the `agent_engine` setting (`AppSettings.agent_engine` in
`src-tauri/src/commands/settings.rs`, default `"omp"`; mirrored as `settings.agent_engine`
on the frontend, with a dropdown in `SettingsPanel.tsx`).

An adapter owns three things: `build_command()` (produce the shell-ready `omp ...`
string), `normalize_line()` (translate one stdout line to a canonical event), and
`capabilities()`. Engine-specific concerns that live *outside* the trait —
`remote_bin_name()`, `install_hint()`, `ensure_local_config()` — are also trait methods
so the caller (`src-tauri/src/commands/agent.rs`) never hardcodes an engine. **When
adding engine behavior, put it on the adapter; do not branch on engine strings in
`agent.rs`.**

### No translation proxy / gateway

Provider and model routing is owned by **OMP's own config** (`~/.omp/agent/models.yml`
and `config.yml`, written by `ensure_omp_config` on first local run). Do **not** add an
HTTP/SSE/WebSocket gateway, an LLM-translation proxy, or a provider shim between Operon
and the engine — Operon talks to the engine **only** as a one-shot CLI over stdout.
There is no SDK and no socket. To change providers, edit `models.yml`, not Operon code.

## IPC / event model

```
build_command()  →  one-shot `omp --mode json -p ...`
       │                         │ stdout = JSONL (AgentSessionEvent per line)
       │   (local: spawned)      │ (remote: redirected to a .jsonl on the shared FS,
       ▼                         ▼  tailed back over a 2nd SSH hop; .done = complete)
   each raw line ──► adapter.normalize_line() ──► canonical event
                                                       │
                          agent.rs emits  agent-event-{sid}  (Tauri event)
                          and  agent-done-{sid}  on EOF
                                                       ▼
                                   ChatPanel.tsx renders it
```

`normalize_line` maps the OMP stream to **five canonical frontend events** (plus a
`raw` passthrough for anything unrecognized — never silently drop a line):

| Canonical event | Emitted from OMP |
|-----------------|------------------|
| `system` | `session` header line — carries `session_id` (persisted for `--resume`) |
| `assistant` | `message_update` text/thinking deltas, and `tool_execution_start` (as a `tool_use` block) |
| `tool` | `tool_execution_end` — matched to the call by `tool_use_id` |
| `result` | `message_end` — usage tokens, cost, stop reason |
| `error` | streaming `error` sub-events, `message_end` with `error`/`aborted`, or an error-level `notice` |

OMP has **no per-message id**, so the adapter synthesizes a stable one that bumps on
`message_start`. It reads the **cumulative** `AssistantMessage` snapshot from
`message_update.message.content[]` (no manual delta accumulation), which matches the
frontend's "same id replaces, new id appends" dedup model.

## Modes

Operon's modes (`agent` / `plan` / `ask` / `report`) are **Operon conventions**, not
OMP features. `build_command()` encodes them: per-mode system-prompt prefixes,
`--thinking high` for plan/report, `--no-tools` for ask, and a temp prompt file
(`@file`) for report (its prompt is too large for argv; the caller SCPs it on remote
runs). Plan mode still works by scaffolding `implementation_plan.md`.

## Key gotchas (from the codebase — read before editing)

- **Never hold a `std::sync::Mutex` across `.await`.** Extract the data, drop the lock,
  then await. `OmpAdapter` keeps `session_id`/`msg_index` behind short-lived locks only.
- **HPC `/tmp` is node-local.** Agent output goes to a `.jsonl` on the **shared
  filesystem** (the working directory, NFS/GPFS), not `/tmp` — a compute-node `/tmp`
  is invisible from the login node that tails it.
- **Base64-encode complex SSH scripts.** The tail script crosses local shell → SSH →
  remote shell → bash; send it base64-encoded to avoid multi-layer quoting breakage.
- **Run in the user's own shell** (not piped to `bash`) so conda/modules/aliases and
  `~/.local/bin` (where `omp` installs) are on `PATH`.
- **Filter SSH stderr** (post-quantum/connection warnings) so they aren't mistaken for
  agent errors.
- **Config writes are idempotent and non-destructive.** `ensure_omp_config` never
  overwrites a user's hand-tuned `models.yml` / `config.yml` / guardrail hook.

## Local dev

```bash
npm install
cargo tauri dev
```

Do not commit unless asked. New Rust commands need a typed wrapper in `src/lib/*.ts`;
new engine behavior belongs on the adapter, not in `agent.rs` engine branches.

## Status

The core swap compiles (`cargo check` green) and is grounded against real omp v16.0.5.
The **HPC remote path is not yet validated on a cluster** — the gating test is to
confirm the Linux binary's glibc compatibility and a one-shot `omp --mode json` tailed
across the login/compute split. See `OMP_MIGRATION_PLAN.md` and `docs/omp-event-schema.md`.
