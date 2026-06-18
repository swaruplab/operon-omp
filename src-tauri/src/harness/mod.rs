//! Harness adapter abstraction.
//!
//! An *adapter* wraps a coding-agent CLI (currently OpenCode against local
//! Ollama) and is responsible for:
//!
//! 1. Building the shell-ready CLI invocation for a given user request
//!    (`build_command`).
//! 2. Translating each line of the agent's stdout into the canonical
//!    NDJSON event shape the frontend already understands
//!    (`normalize_line`).
//! 3. Declaring which optional features it supports (`capabilities`) so the
//!    UI can grey out unsupported toggles.
//!
//! Operon-OMP ships OMP (oh-my-pi) as the default engine, with OpenCode kept
//! behind the `agent_engine` setting for one-flag rollback. The `pick()` helper
//! is the single construction point so adapters plug in without touching
//! `start_agent_session`; engine-specific concerns that live OUTSIDE the trait
//! (remote binary name, install hint, local config bootstrap) are exposed as
//! trait methods so the caller never hardcodes an engine.

pub mod omp;
pub mod opencode;

/// Pick a harness adapter for the configured engine. Defaults to OMP (oh-my-pi);
/// `"opencode"` selects the legacy OpenCode adapter, kept for one-flag rollback.
pub fn pick(engine: &str) -> Box<dyn HarnessAdapter> {
    match engine {
        "opencode" => Box::new(opencode::OpenCodeAdapter::new()),
        _ => Box::new(omp::OmpAdapter::new()),
    }
}

use std::path::PathBuf;

/// Inputs the caller has already resolved (settings read, plan content
/// loaded, MCP config generated, timestamp computed) and now hands to the
/// adapter so it can build a CLI invocation.
///
/// All paths are caller-side; the adapter does not touch settings or SSH.
pub struct BuildContext<'a> {
    /// User's raw prompt text (not yet shell-escaped).
    pub prompt: &'a str,
    /// Working directory the agent will run in (local or remote).
    pub project_path: &'a str,
    /// Frontend-supplied session UUID (used to name temp prompt files etc.).
    pub session_id: &'a str,
    /// `"agent" | "plan" | "ask" | "report"`.
    pub mode: &'a str,
    /// Optional model override.
    pub model: Option<&'a str>,
    /// Optional turn budget.
    pub max_turns: Option<u32>,
    /// Agent CLI session id to resume, if any.
    pub resume_session: Option<&'a str>,
    /// `"full_auto" | "safe_mode" | "supervised"` — caller pre-resolved
    /// from settings.
    pub permission_mode: &'a str,
    /// Contents of the project's `implementation_plan.md` if it exists, else
    /// empty. Adapters may inject this as context.
    pub existing_plan: &'a str,
    /// Human-readable timestamp for plan-mode headers (e.g.
    /// `"2026-05-05 14:23 UTC"`).
    pub now_timestamp: &'a str,
    /// Path to an MCP config file the caller has already generated, or
    /// `None` if the user has no MCP servers configured.
    pub mcp_config_path: Option<&'a str>,
}

/// What the adapter produces.
pub struct BuildOutput {
    /// Shell-ready command string. The caller is responsible for spawning
    /// this through a login shell (locally or via SSH).
    pub command: String,
    /// Path to a temp file the adapter created that must travel with the
    /// command (currently used for report mode, where the prompt is too
    /// large to fit on the command line and is piped via stdin). The caller
    /// is responsible for SCP'ing this to the remote in remote sessions and
    /// cleaning it up after the run.
    pub prompt_file: Option<PathBuf>,
}

/// What an adapter supports. Frontend can use this to disable irrelevant
/// toggles (e.g. resume button on adapters that don't support it).
#[derive(Debug, Clone, Copy)]
pub struct Capabilities {
    pub resume: bool,
    pub plan_mode: bool,
    pub max_turns: bool,
    pub mcp: bool,
}

pub trait HarnessAdapter: Send + Sync {
    /// Stable identifier (`"opencode"`, …). Used in logs and session
    /// metadata.
    fn id(&self) -> &'static str;

    /// Build the CLI command for a single agent run.
    fn build_command(&self, ctx: &BuildContext<'_>) -> Result<BuildOutput, String>;

    /// Translate one raw stdout line into a canonical NDJSON event line, or
    /// `None` to drop it.
    fn normalize_line(&self, raw: &str) -> Option<String>;

    fn capabilities(&self) -> Capabilities;

    /// The binary name to probe/resolve on a remote host, and the leading bare
    /// token of `build_command`'s output (the caller absolutizes it for remote
    /// runs). Defaults to `id()`.
    fn remote_bin_name(&self) -> &'static str {
        self.id()
    }

    /// Human-facing hint for installing the engine on a remote host when the
    /// binary can't be found.
    fn install_hint(&self) -> &'static str;

    /// Write a default local engine config (provider/model, roles, guardrails)
    /// into the appropriate location if one doesn't already exist. Returns
    /// `Ok(true)` if anything was written, `Ok(false)` if it already existed.
    /// Called only for local sessions (remote config is pre-placed on the host).
    fn ensure_local_config(&self, project_path: &str, model: &str) -> Result<bool, String>;
}
