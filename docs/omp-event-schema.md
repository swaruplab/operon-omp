# OMP `--mode json` Event Schema

This document describes the event stream that OMP (oh-my-pi, package
`@oh-my-pi/pi-coding-agent`) emits when Operon runs it as a one-shot CLI, and how
Operon's harness adapter (`src-tauri/src/harness/omp.rs`) normalizes each event
into Operon's canonical NDJSON shape.

**Provenance.** The schema below was verified from OMP source on branch `main`
(`packages/coding-agent/src/modes/print-mode.ts`, `packages/agent/src/types.ts`,
`packages/ai/src/types.ts`, `packages/catalog/src/types.ts`) and cross-checked
against the real `omp` v16.0.5 binary. Treat the OMP source as authoritative if
it ever drifts from this document.

> **Status — honest scope.** The core engine swap compiles (`cargo check` is
> green) and is grounded against the real `omp` v16.0.5 binary running locally.
> The **HPC remote path has not yet been tested on a real cluster.** The gating
> test before trusting the remote path is: confirm the Linux binary's glibc
> compatibility on the target login/compute nodes, and run one
> `omp --mode json` job whose JSONL output is tailed across the login/compute
> split (the same shared-filesystem + SSH-tail flow Operon uses for OpenCode).

---

## Wire format

`omp --mode json -p [...]` writes **JSONL** to stdout: one
`JSON.stringify(event)` per line, **no envelope and no framing**. Every line is a
self-contained JSON object with a `type` field. Operon's reader splits on
newlines and hands each line to `OmpAdapter::normalize_line`, which either emits
one canonical line or drops the event (`None`).

Key invariant: the **first** line is always the `session` header, and its `.id`
is the session id Operon surfaces for `--resume`.

A non-JSON line (should not happen in `--mode json`, but defensive) is passed
through untouched as `{"type":"raw","source":"omp","line":"<text>"}`.

---

## Events

### `session` (header — line 1)

```json
{ "type": "session", "version": 3, "id": "0f3c…uuid", "model": "…", "cwd": "…" }
```

- `.id` — the session id. Operon stores it and surfaces it immediately so the UI
  can persist it for `--resume`, and attaches it to the final `result`.
- The adapter also accepts `"session_start"` as an alias for this header.

### `message_start`

Marks the beginning of a new assistant message (a new conversational turn within
the run). It carries no useful payload for rendering.

OMP's stream has **no per-message id**. The adapter therefore synthesizes a
stable id and **bumps it on every `message_start`**, so the frontend's
stream-dedup logic treats the next snapshots as a *new* message (append) rather
than an update to the previous one (replace).

### `message_update` (streaming chunk)

This is the workhorse event. It is emitted repeatedly as the assistant streams.
Two fields matter:

- `.assistantMessageEvent` — the **delta sub-event**: what just changed. Its
  `.type` is one of:
  - `text_start` / `text_delta` / `text_end`
  - `thinking_start` / `thinking_delta` / `thinking_end`
  - `toolcall_start` / `toolcall_delta` / `toolcall_end`
  - `error` — carries `.error.errorMessage`
- `.message` — the **CUMULATIVE `AssistantMessage` snapshot** so far. Its
  `.content[]` array holds typed blocks:
  - `{ "type": "text", "text": "…" }`
  - `{ "type": "thinking", "thinking": "…" }`
  - (tool-call blocks may also appear, but Operon does not render them from here)

Because `.message` is cumulative, the adapter does **not** accumulate deltas
itself. On any `text_*` sub-event it concatenates every `text` block of
`.message.content[]` and emits the full text so far; on any `thinking_*`
sub-event it does the same for `thinking` blocks. The frontend's
same-id-means-replace rule turns these repeated full snapshots into smooth
in-place streaming.

`toolcall_*` sub-events are intentionally **not** rendered from `message_update`
— tool calls are surfaced from the top-level `tool_execution_*` events instead,
which carry a stable `toolCallId` and the resolved args/result.

### `tool_execution_start`

```json
{ "type": "tool_execution_start", "toolCallId": "tc_…", "toolName": "bash", "args": { … } }
```

The canonical "a tool is now running" signal. Operon renders it as a
`tool_use` block keyed by `toolCallId`.

### `tool_execution_end`

```json
{ "type": "tool_execution_end", "toolCallId": "tc_…", "result": <string|object>, "isError": false }
```

The tool result, keyed by the same `toolCallId`. `.result` may be a string or an
arbitrary JSON value (the adapter stringifies non-string results).
`tool_execution_update` (progress) is dropped.

### `message_end`

Closes the current assistant message. Carries the accounting fields on
`.message`:

- `.message.usage` — `{ input, output, cacheRead, cacheWrite, totalTokens, … }`
  (and optionally `reasoningTokens`). Cost is at `.message.usage.cost.total`
  (USD).
- `.message.stopReason` — one of `stop` | `length` | `toolUse` | `error` |
  `aborted`.

If `stopReason` is `error` or `aborted`, the adapter emits a canonical `error`
(reading `.message.errorMessage`); otherwise it emits the canonical `result`
with usage + cost + the remembered session id.

### Housekeeping / lifecycle events

These structural events are emitted by OMP to describe its own progress. They
are **not** rendered as chat content — most are dropped, the rest surface as
`raw` for visibility during bring-up:

| Event prefix / name | Meaning | Operon treatment |
|---|---|---|
| `agent_start` / `agent_end` | top-level agent run boundary | dropped |
| `turn_start` / `turn_end` | a model turn boundary | dropped |
| `tool_execution_update` | tool progress tick | dropped |
| `auto_compaction_start` / `auto_compaction_end` | context window auto-compaction | `raw` |
| `auto_retry_*` | a transient call is being retried | `raw` |
| `retry_fallback_*` | provider/model fallback chain engaged | `raw` |
| `ttsr_triggered` | a time-traveling streamed rule fired (see autonomy doc) | `raw` |
| `todo_*` | the agent's internal todo list changed | `raw` |
| `notice` | out-of-band session notice with a `level` | error → `error`, else dropped |
| *(any other `type`)* | unknown / new in a future OMP build | logged to stderr + `raw` |

`notice` is the one housekeeping event with rendering significance: a
`{"type":"notice","level":"error","message":"…"}` is surfaced as a canonical
`error`; `info`/`warning` notices are dropped.

---

## Mapping table: OMP event → Operon canonical event

The right column is the `type` of the line `normalize_line` emits (or *(drop)*
when it returns `None`). Operon's canonical events are `system`,
`assistant` (with a `tool_use` / `text` / `thinking` content block), `tool`,
`result`, `error`, and `raw`.

| OMP event | OMP sub-discriminator | Operon canonical event |
|---|---|---|
| `session` / `session_start` | — | `system` (`{session_id}`) |
| `message_start` | — | *(drop; bumps synthetic msg id)* |
| `message_update` | `assistantMessageEvent.type` = `text_*` | `assistant` → `content:[{type:text,text}]` |
| `message_update` | `assistantMessageEvent.type` = `thinking_*` | `assistant` → `content:[{type:thinking,thinking}]` |
| `message_update` | `assistantMessageEvent.type` = `toolcall_*` | *(drop; tool surfaced via `tool_execution_*`)* |
| `message_update` | `assistantMessageEvent.type` = `start` / `done` | *(drop)* |
| `message_update` | `assistantMessageEvent.type` = `error` | `error` |
| `tool_execution_start` | — | `assistant` → `content:[{type:tool_use,id,name,input}]` |
| `tool_execution_end` | — | `tool` (`{tool_use_id,content}`) |
| `tool_execution_update` | — | *(drop)* |
| `message_end` | `stopReason` ∈ `stop`/`length`/`toolUse` | `result` (`{session_id,subtype,usage,total_cost_usd}`) |
| `message_end` | `stopReason` ∈ `error`/`aborted` | `error` |
| `notice` | `level` = `error` | `error` |
| `notice` | `level` ≠ `error` | *(drop)* |
| `agent_start` / `agent_end` / `turn_start` / `turn_end` | — | *(drop)* |
| `auto_compaction_*` / `auto_retry_*` / `retry_fallback_*` / `ttsr_triggered` / `todo_*` | — | `raw` |
| *(unrecognized `type`)* | — | `raw` (+ stderr log) |
| *(non-JSON line)* | — | `raw` (`{source:"omp",line}`) |

### Canonical output shapes (for reference)

```jsonc
// system  (from session header)
{ "type": "system", "session_id": "0f3c…" }

// assistant text  (from message_update / text_*)
{ "type": "assistant", "message": { "id": "omp-msg-3", "role": "assistant",
  "content": [ { "type": "text", "text": "…cumulative text…" } ] } }

// assistant thinking  (from message_update / thinking_*)
{ "type": "assistant", "message": { "id": "omp-msg-3-thinking", "role": "assistant",
  "content": [ { "type": "thinking", "thinking": "…cumulative reasoning…" } ] } }

// assistant tool_use  (from tool_execution_start)
{ "type": "assistant", "message": { "id": "omp-tool-tc_42", "role": "assistant",
  "content": [ { "type": "tool_use", "id": "tc_42", "name": "bash", "input": { … } } ] } }

// tool result  (from tool_execution_end)
{ "type": "tool", "tool_use_id": "tc_42", "content": "…stdout/result…" }

// result  (from message_end)
{ "type": "result", "session_id": "0f3c…", "subtype": "stop",
  "usage": { "input": 1234, "output": 567, "total": 1801,
             "cacheRead": 0, "cacheWrite": 0, "reasoning": null },
  "total_cost_usd": 0.0 }

// error  (from message_update/error, message_end error/aborted, or notice error)
{ "type": "error", "error": { "message": "…" } }
```

These shapes are identical to what the OpenCode adapter emits, which is why the
frontend (`ChatPanel.tsx`) renders OMP sessions with no changes: the engine swap
is fully contained inside the harness adapter.
