# Operon Lab TTSR Rules (TEMPLATE — adapt per project)

<!--
  TTSR = Time-Traveling Streamed Rules.

  HOW IT WORKS
  ------------
  OMP (oh-my-pi) watches the agent's streaming context. The moment text matching
  a rule's `trigger` regex streams by, OMP injects that rule's `inject` text back
  into the agent's context *at that point* (hence "time-traveling": the reminder
  arrives exactly when it becomes relevant, not at the start of the run). When a
  rule fires, OMP emits a `ttsr_triggered` event, which Operon surfaces as a
  `raw` event (see docs/omp-event-schema.md).

  These rules are a lightweight, declarative complement to the hard-block
  guardrail hook (~/.omp/hooks/pre/operon-guardrails.ts). Use the hook to BLOCK
  dangerous actions; use TTSR to NUDGE the agent's reasoning.

  FILE FORMAT
  -----------
  Each rule is a fenced ```ttsr block containing simple `key: value` lines:
    trigger: <regex>      — case-insensitive regex matched against streamed text
    inject:  <text>       — reminder injected when the trigger matches
    once:    true|false   — (optional) fire at most once per session (default true)

  This is a TEMPLATE. Copy it to your project's `.omp/rules/` directory and edit
  the regexes/messages for your lab's data-governance and HPC policies. Keep
  triggers specific to avoid noisy injections.
-->

## Rule 1 — Controlled-access / sensitive data stays on local models

Reminds the agent that anything touching controlled-access or patient-identifying
data must be handled with **local** models only — never sent to a cloud provider.
Pair this with provider routing in `~/.omp/agent/config.yml` (keep the relevant
`modelRoles` / `fallbackChains` pointed at local endpoints).

```ttsr
trigger: (dbGaP|controlled-access|PHI|patient.identifi)
inject: |
  DATA GOVERNANCE: this context references controlled-access / patient-identifying
  data (dbGaP, PHI, etc.). Keep all reasoning and tool use on LOCAL models only —
  do NOT route this content to any cloud provider, and do not copy raw identifiers
  into prompts, logs, or files outside the approved data mount. If a task would
  require a cloud model, stop and ask the user instead.
once: true
```

## Rule 2 — SLURM: submit and end the turn (do not poll)

Reinforces Operon's HPC rule: after `sbatch`, report the job id and END THE TURN.
Operon's watchdog tracks completion — polling `squeue`/`sacct` in a loop would
block the one-shot headless run.

```ttsr
trigger: sbatch
inject: |
  HPC BATCH RULE: you are submitting a SLURM job. Submit it, capture and report the
  job id, then END YOUR TURN. Do NOT poll `squeue`/`sacct` in a loop and do NOT
  block waiting for the job — Operon's watchdog tracks completion and will resume
  the session when the job finishes.
once: true
```
