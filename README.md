<div align="center">

# Operon-OMP

**A desktop IDE for HPC bioinformatics — with its agent engine swapped from OpenCode to OMP (oh-my-pi).**

[![Tauri](https://img.shields.io/badge/Tauri_2-Rust_%2B_React-orange.svg)](https://tauri.app)
[![Engine](https://img.shields.io/badge/agent-OMP_(oh--my--pi)-purple.svg)](https://github.com/can1357/oh-my-pi)

Operon is a native desktop application (Tauri 2 — Rust backend + React/TypeScript
frontend) for computational biologists: an integrated terminal, Monaco code editor,
file browser, SSH/HPC remote access, and an AI chat panel in one workspace. It is
built for scientists who run pipelines on shared HPC clusters — Operon can run an
AI agent **directly on a remote compute node** over SSH + tmux, with sessions that
persist across app restarts.

**Operon-OMP** is the same Operon with one thing changed: the agent "brain" is now
[**OMP (oh-my-pi)**](https://github.com/can1357/oh-my-pi) instead of OpenCode, for a
more autonomous and more robust agent. OpenCode is kept behind a setting for
one-flag rollback.

</div>

---

## What changed vs. stock Operon

The engine swap is deliberately surgical. Operon already drives its agent as a
**one-shot CLI that streams line-delimited JSON to stdout**, behind a small Rust
abstraction — the `HarnessAdapter` trait. OMP supports exactly that shape, so the
migration is "add an adapter and select it," not "rewrite the transport."

There is **no HTTP, no SSE, no WebSocket, and no translation proxy/gateway** between
Operon and the engine. Provider/model routing is owned entirely by OMP's own config
(`~/.omp/agent/models.yml`), not by Operon.

## Architecture (one paragraph)

The engine seam is the `HarnessAdapter` trait in `src-tauri/src/harness/mod.rs`;
`pick(engine)` is the single construction point that returns either the OMP adapter
(`src-tauri/src/harness/omp.rs`, default) or the legacy OpenCode adapter
(`opencode.rs`). For each turn, `src-tauri/src/commands/agent.rs` asks the adapter to
`build_command()` a shell-ready `omp --mode json -p ...` invocation, runs it as a
**one-shot CLI** (locally, or on an HPC compute node where stdout is redirected to a
`.jsonl` file on the **shared filesystem** and tailed back over a second SSH hop with
a `.done` sentinel for completion), and feeds every output line through the adapter's
`normalize_line()`. That translates OMP's `AgentSessionEvent` stream into the five
canonical events the React `ChatPanel` already understands (`system`, `assistant`,
`tool`, `result`, `error`; anything unrecognized passes through as `raw`). The OMP
adapter also bootstraps `~/.omp/agent/{models,config}.yml` and a guardrail hook on
first local run (`ensure_local_config`), and exposes the remote binary name and
install hint so the caller never hardcodes an engine.

## Autonomy capabilities

Switching to OMP unlocks four agent capabilities Operon wires in:

1. **Model-role routing + fallback chains** — `~/.omp/agent/config.yml` defines roles
   (`default` / `plan` / `smol` / `slow` / `commit`) so sub-tasks route to the right
   model, with automatic provider/model fallback on failure.
2. **Programmable guardrails (hooks + TTSR)** — a bundled pre-tool-call hook
   (`~/.omp/hooks/pre/operon-guardrails.ts`) hard-blocks destructive operations
   (e.g. `rm` on shared/data filesystems, recursive deletes of a top-level path).
   Because runs are headless, guardrails **block** rather than prompt. TTSR
   (time-traveling stream rules) is available for soft, mid-generation policy nudges.
3. **Subagent swarm** — OMP can fan a task out to coordinated subagents.
4. **Hashline edits + persistent Python kernel** — hash-anchored edits for robust file
   modification, and a persistent Python kernel for stateful analysis.

## Switching engines

The active engine is the `agent_engine` setting (Rust: `AppSettings.agent_engine` in
`src-tauri/src/commands/settings.rs`; TS: `settings.agent_engine`). Valid values:

| Value | Engine |
|-------|--------|
| `omp` (default) | OMP / oh-my-pi |
| `opencode` | Legacy OpenCode adapter — kept for one-flag rollback |

In the app, open **Settings → Agent Settings → Agent Engine** and choose
*OMP (oh-my-pi)* or *OpenCode (legacy)*. The change takes effect on the next chat turn;
`pick(engine)` selects the adapter, and there is nothing else to reconfigure.

## Installing OMP

OMP ships as a single self-contained binary (~150 MB) — no Bun/Node at runtime, no root:

```bash
curl -fsSL https://omp.sh/install | sh
```

This installs `omp` to `~/.local/bin` (already on Operon's remote `PATH` and in its
binary resolver). It supports macOS (arm64/x64), Linux (x64/arm64), and Windows (x64).
On a no-admin HPC node, run the same installer in your home directory, or pre-download
the matching `omp-linux-<arch>` release asset and `chmod +x` it.

OMP is `@oh-my-pi/pi-coding-agent` (https://github.com/can1357/oh-my-pi, MIT).

## Running the app

```bash
# from the repo root
npm install
cargo tauri dev      # development with hot-reload

# production build (macOS)
cargo tauri build
# Output: src-tauri/target/release/bundle/macos/Operon.app (+ .dmg)
```

Prerequisites for building from source: Xcode Command Line Tools, Rust (rustup),
and Node.js 18+. Operon's first-run setup wizard handles agent/runtime dependencies.

## Status (honest)

- **Core engine swap: working.** The OMP adapter (`src-tauri/src/harness/omp.rs`) is
  implemented behind the `HarnessAdapter` trait, selected by `pick()` from the
  `agent_engine` setting, and the backend **compiles clean** (`cargo check` green).
- **Grounded against the real binary.** `build_command` and `normalize_line` are
  written against the actual **omp v16.0.5** CLI surface and event schema (`--mode json -p`,
  the `session` header line, `message_update` / `tool_execution_*` / `message_end`
  events, `--resume`, `--thinking`, `--append-system-prompt`, `--auto-approve`).
- **HPC remote path: NOT yet validated on a real cluster.** The one-shot
  `omp --mode json` design fits Operon's SSH-tail model on paper, but the gating test
  has not been run. That test is: on the **actual HPC cluster**, copy `omp-linux-<arch>`,
  `chmod +x`, confirm the Linux binary's **glibc compatibility** (`ldd ./omp` / execute
  it on the target node — HPC nodes often run old RHEL/CentOS glibc), then hand-run
  `cd '<shared-fs path>' && omp --mode json -p '<prompt>' > out.jsonl 2>&1; echo $? > out.done`
  from a **compute node** while `tail -f`-ing `out.jsonl` from the **login node** and
  confirming per-line flushing. Until that passes, treat remote/HPC sessions as
  unverified.

## Further reading

- [`OMP_MIGRATION_PLAN.md`](OMP_MIGRATION_PLAN.md) — the full OpenCode → OMP migration
  analysis: scope, the adapter design, the non-trait couplings generalized in
  `agent.rs`, the HPC install strategy, risk register, and phased plan.
- [`docs/omp-event-schema.md`](docs/omp-event-schema.md) — the OMP `AgentSessionEvent`
  stream and how `normalize_line` maps each event to Operon's canonical frontend events.
- [`AGENTS.md`](AGENTS.md) — contributor/agent guide for working in this repo.

## License

This repository is MIT-licensed (see [`LICENSE`](LICENSE)). OMP / oh-my-pi is a
separate MIT project (© Mario Zechner and Can Bölük); Operon consumes its `omp`
binary, it is not vendored into this source tree.
