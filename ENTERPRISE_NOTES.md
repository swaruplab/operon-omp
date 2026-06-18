# Operon Enterprise — Project Notes

This is a fork of `swaruplab/operon` for an enterprise edition that replaces the
Claude Code harness with open-source / local-GPU-friendly alternatives. Strategic
and architectural context lives here so future sessions can pick up without
reconstructing it from scratch.

---

## Repo Setup

- **Origin (this repo):** https://github.com/swaruplab/operon-enterprise (private)
- **Upstream (base project):** https://github.com/swaruplab/operon (public)
- **Active branch:** `enterprise-harness`
- **Sibling working copy of base project:** `../operon_crossplatform`
- Cherry-pick fixes from upstream with: `git fetch upstream && git cherry-pick <sha>`

The base project is **untouched** — all enterprise experimentation happens here.

---

## Why this fork exists

Lab/enterprise users (pharma, research IT, HPC operators) are asking whether the
Claude Code harness can be replaced with an open-source agent that runs against
their own GPUs (Gemma, Qwen-Coder, Llama-3.3, DeepSeek-Coder served via vLLM /
Ollama / TGI). Goals:

1. Make the harness pluggable, not Claude-locked.
2. Support OpenAI-compatible endpoints pointing at on-prem inference.
3. Ship as a separate binary (different bundle id) so it installs alongside the
   open-source Operon.
4. Position as a paid enterprise SKU without burning the OSS community version.

---

## Harness Refactor Plan

The Claude Code coupling lives in three places:

1. `src-tauri/src/commands/claude.rs` — spawns
   `claude -p --verbose --output-format stream-json`, parses NDJSON, emits
   Tauri events.
2. `src/types/chat.ts` — `ClaudeEvent`, tool/thinking blocks.
3. Lifecycle conventions — `--resume`, `--max-turns`,
   `--dangerously-skip-permissions`, `implementation_plan.md`, session-id
   captured from `system` event.

Everything else (terminal, SSH, files, editor, settings) is harness-agnostic.

### Plan

Introduce a **`HarnessAdapter`** trait in Rust:

```rust
trait HarnessAdapter {
    fn spawn_cmd(&self, prompt, cwd, resume_id, model) -> String;
    fn parse_line(&self, raw: &str) -> Option<NormalizedEvent>;
    fn capabilities(&self) -> Caps; // resume? plan mode? tool format?
}
```

- Keep `ClaudeEvent` as the canonical internal event format.
- Each adapter translates its CLI's output into it.
- Frontend stays unchanged.
- Settings gain `harness: "claude" | "opencode" | ...` plus
  `endpoint_url`, `api_key`, `model_name` for BYO inference servers.

### Adapter Roadmap

1. `harness/claude.rs` — move existing logic, validate the seam.
2. `harness/opencode.rs` — primary enterprise harness (closest Claude Code clone,
   provider-agnostic).
3. (later) `harness/codex.rs` — OpenAI Codex CLI; note `.codex/` already
   appearing in upstream working tree suggests prior exploration.
4. (later) `harness/goose.rs` — Block's agent, multi-provider.

### Bundle Rename

For side-by-side install with base Operon:
- `tauri.conf.json` → change `productName`, `identifier`, app icon.
- `package.json` → rename.
- Different `identifier` is what lets both versions live on disk simultaneously.

### Known Hard Parts

- HPC tmux + tail-jsonl flow in `claude.rs` is shaped around Claude's NDJSON.
  Each adapter needs its own base64-encoded tail script. Per-harness, not hard.
- MCP support is not 1:1 across harnesses. OpenCode + Goose have equivalents.
- Plan-mode (`implementation_plan.md`) is just a file convention — ports
  trivially.
- `--resume` exists for OpenCode and Codex CLI; Aider is different.

---

## Model Reality Check

Tool-use quality on open models is not yet at Claude / GPT-4 class. For agentic
coding workflows recommend:

- **Qwen2.5-Coder-32B** — strongest open coder for tool use today.
- **DeepSeek-Coder-V2** — competitive, larger.
- **Llama-3.3-70B-Instruct** — solid generalist with usable function calling.
- **Gemma** — set expectations: weaker on multi-step tool use. Document this for
  customers who lead with "we have Gemma running."

Inference servers customers will deploy: vLLM (best throughput), Ollama (easiest
single-node), TGI (HuggingFace, batched), llama.cpp server (fallback).

---

## Licensing & Business Strategy (working position)

Recommendation: **open core + source-available enterprise add-ons**.

- **Operon (base, MIT/Apache-2.0)** — current app, BYO Claude Code.
- **Operon Enterprise (source-available, FSL or BSL)** — OpenCode/local-GPU
  harness, SSO/SAML, audit logging, multi-user workspace, admin console,
  support. Source published and auditable. License blocks competing SaaS use,
  converts to Apache after 2-4 years (FSL pattern).

### Why not fully closed

The target audience is research / HPC / pharma:
- Reproducibility culture — closed tools are viewed with suspicion.
- Procurement loves OSS-with-paid-support (Red Hat model).
- They run on-prem / air-gapped — they need to read the code anyway.
- Tiny competitive cloning threat in this niche.

### What enterprises actually pay for

Not source secrecy. They pay for:
- Support / SLA
- Liability transfer + indemnification
- Compliance attestations (SOC2, HIPAA)
- Roadmap influence
- Integration help (their auth, vault, scheduler)

A solo dev/small lab can't deliver most of these alone. Pre-revenue blockers:
need an LLC at minimum (ideally Delaware C-corp) — pharma legal won't sign
contracts with an individual.

### Pricing tiers (sketch, not committed)

- Free: base Operon, BYO key.
- Per-seat license for enterprise binary.
- Support contracts on top (the real revenue).
- Custom integration / deployment services.

---

## Open Decisions

- [ ] Confirm OpenCode as adapter #2 (vs Codex CLI as primary — `.codex/` hint
      in upstream).
- [x] Final license choice: **FSL-1.1-MIT** (Functional Source License,
      MIT Future License — converts to MIT after 2 years). Picked at v0.1.0.
- [ ] What goes in free tier vs paid tier (SSO, audit log, multi-user are
      paid-tier candidates).
- [ ] Entity formation timing — needed before first paid contract.
- [ ] App rename: `Operon Enterprise`? `Operon Pro`? something else?

---

## Working Notes

- Base project's `CLAUDE.md` still applies for architecture, gotchas, and
  HPC-specific patterns. Do not duplicate it here — read both.
- The graphify knowledge graph in `graphify-out/` was inherited from upstream
  and is stale relative to enterprise-harness changes. Re-run `graphify update .`
  after the harness refactor lands.
