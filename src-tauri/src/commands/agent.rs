use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as AsyncCommand;

/// Suppress console window creation on Windows for std::process::Command.
#[cfg(windows)]
fn hide_window(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x08000000)
}
#[cfg(not(windows))]
fn hide_window(cmd: &mut std::process::Command) -> &mut std::process::Command {
    cmd
}

/// Suppress console window creation on Windows for tokio::process::Command.
#[cfg(windows)]
fn hide_window_async(cmd: &mut AsyncCommand) -> &mut AsyncCommand {
    cmd.creation_flags(0x08000000)
}
#[cfg(not(windows))]
fn hide_window_async(cmd: &mut AsyncCommand) -> &mut AsyncCommand {
    cmd
}

/// The shell used to launch agent sessions with `-l -c` flags.
/// On Windows, Git Bash is required because cmd.exe doesn't support `-l`/`-c`
/// and the OpenCode CLI itself needs a POSIX environment.
fn agent_shell() -> String {
    #[cfg(target_os = "windows")]
    {
        crate::platform::find_git_bash_path().unwrap_or_else(|| crate::platform::default_shell())
    }
    #[cfg(not(target_os = "windows"))]
    {
        crate::platform::default_shell()
    }
}

// --- Types ---

/// Default engine for session files written before `agent_engine` existed.
fn default_session_engine() -> String {
    "omp".to_string()
}

/// Persistent metadata about an agent session, saved to ~/.operon/sessions/
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionMetadata {
    pub session_id: String,                // Our frontend UUID
    pub agent_session_id: Option<String>, // Agent CLI's internal session ID (for --resume)
    pub project_path: String,              // Local or remote working directory
    pub profile_id: Option<String>,        // SSH profile ID if remote
    pub remote_path: Option<String>,       // Remote path if remote
    pub mode: String,                      // "agent", "plan", "ask"
    pub model: Option<String>,
    pub created_at: u64,             // Unix timestamp ms
    pub last_activity: u64,          // Unix timestamp ms
    pub status: String,              // "running", "completed", "failed"
    pub use_terminal: bool,          // Whether this used terminal mode
    pub terminal_id: Option<String>, // Terminal ID if terminal mode
    #[serde(default)]
    pub name: Option<String>, // Human-readable session name (from first prompt)
    /// Which engine drove this session ("omp" / "opencode"), so reconnect uses
    /// the matching adapter even across app restarts.
    #[serde(default = "default_session_engine")]
    pub agent_engine: String,
}

/// Status of a session's output files on the filesystem
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionFileStatus {
    pub session_id: String,
    pub output_exists: bool,
    pub done_exists: bool,
    pub is_running: bool,   // output exists but done doesn't
    pub is_completed: bool, // both exist
}

/// PATH prefix applied to every remote SSH command that invokes the agent.
/// Covers known install locations so the engine binary (`omp` / `opencode`) is
/// found regardless of shell or rc files. `~/.local/bin` is where `omp.sh`
/// installs the self-contained OMP binary.
const REMOTE_PATH_PREFIX: &str =
    r#"export PATH="$HOME/.local/bin:$HOME/.omp/bin:$HOME/.opencode/bin:$HOME/.npm-global/bin:$PATH"; "#;

pub struct AgentSession {
    pub child: tokio::process::Child,
}

pub struct AgentManager {
    pub sessions: Mutex<HashMap<String, AgentSession>>,
}

impl AgentManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

// --- Session Metadata Persistence ---

fn sessions_dir() -> Result<std::path::PathBuf, String> {
    crate::platform::sessions_dir()
}

fn save_session_to_disk(meta: &SessionMetadata) -> Result<(), String> {
    let dir = sessions_dir()?;
    let path = dir.join(format!("{}.json", meta.session_id));
    let data = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| format!("Failed to save session: {}", e))
}

fn load_session_from_disk(session_id: &str) -> Result<Option<SessionMetadata>, String> {
    let dir = sessions_dir()?;
    let path = dir.join(format!("{}.json", session_id));
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let meta: SessionMetadata = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    Ok(Some(meta))
}

fn load_all_sessions_from_disk() -> Vec<SessionMetadata> {
    let dir = match sessions_dir() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut sessions = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                if let Ok(data) = std::fs::read_to_string(&path) {
                    if let Ok(meta) = serde_json::from_str::<SessionMetadata>(&data) {
                        sessions.push(meta);
                    }
                }
            }
        }
    }
    // Sort by last_activity descending (most recent first)
    sessions.sort_by_key(|s| std::cmp::Reverse(s.last_activity));
    sessions
}

// --- Agent Session ---

/// Optional SSH context for running the agent on a remote server
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RemoteContext {
    pub profile_id: String,
    pub remote_path: String,
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn start_agent_session(
    state: tauri::State<'_, AgentManager>,
    terminal_state: tauri::State<'_, super::terminal::TerminalManager>,
    ssh_state: tauri::State<'_, super::ssh::SSHManager>,
    settings_state: tauri::State<'_, super::settings::SettingsManager>,
    app: tauri::AppHandle,
    session_id: String,
    prompt: String,
    project_path: String,
    model: Option<String>,
    max_turns: Option<u32>,
    resume_session: Option<String>,
    mode: Option<String>,
    remote: Option<RemoteContext>,
    use_terminal: Option<bool>,
    terminal_id: Option<String>,
) -> Result<(), String> {
    let mode = mode.unwrap_or_else(|| "agent".to_string());
    eprintln!(
        "[operon] start_agent_session: mode='{}', resume={:?}, max_turns={:?}",
        mode, resume_session, max_turns
    );

    // --- Check for existing plan files in the target directory ---
    // This gives the agent context about previous planning sessions in this folder.
    let existing_plan = if let Some(ref ctx) = remote {
        // Remote: read implementation_plan.md via SSH
        let profile = {
            let profiles = ssh_state.profiles.lock().map_err(|e| e.to_string())?;
            profiles.iter().find(|p| p.id == ctx.profile_id).cloned()
        };
        if let Some(prof) = profile {
            let check_cmd = format!(
                "cat '{}'/implementation_plan.md 2>/dev/null || echo ''",
                ctx.remote_path.replace('\'', "'\\''")
            );
            super::ssh::ssh_exec(&prof, &check_cmd).unwrap_or_default()
        } else {
            String::new()
        }
    } else {
        // Local: read implementation_plan.md from project path
        let plan_path = std::path::Path::new(&project_path).join("implementation_plan.md");
        std::fs::read_to_string(&plan_path).unwrap_or_default()
    };
    let existing_plan = existing_plan.trim().to_string();

    // Permission mode is consumed by the harness adapter when shaping the
    // CLI invocation (see src/harness/opencode.rs).
    let permission_mode = {
        let settings = settings_state.settings.lock().map_err(|e| e.to_string())?;
        settings.permission_mode.clone()
    };

    // Generate a human-readable timestamp for plan sections
    let now_timestamp = {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Format as YYYY-MM-DD HH:MM (UTC)
        let days = secs / 86400;
        let time_of_day = secs % 86400;
        let hours = time_of_day / 3600;
        let minutes = (time_of_day % 3600) / 60;
        // Compute year/month/day from epoch days
        let mut y = 1970i64;
        let mut remaining = days as i64;
        loop {
            let days_in_year = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                366
            } else {
                365
            };
            if remaining < days_in_year {
                break;
            }
            remaining -= days_in_year;
            y += 1;
        }
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let month_days = [
            31,
            if leap { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ];
        let mut m = 0usize;
        for &md in &month_days {
            if remaining < md as i64 {
                break;
            }
            remaining -= md as i64;
            m += 1;
        }
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02} UTC",
            y,
            m + 1,
            remaining + 1,
            hours,
            minutes
        )
    };
    // Also compute a filename-safe version for archiving
    let now_filename = now_timestamp.replace(' ', "_").replace(':', "");

    // --- Plan mode: archive existing plan before writing a new one ---
    // This keeps implementation_plan.md clean (always ONE active plan) while
    // preserving full history in .operon/plan_history/ for reference.
    if mode == "plan" && !existing_plan.is_empty() {
        if let Some(ref ctx) = remote {
            // Remote: archive via SSH
            let profile = {
                let profiles = ssh_state.profiles.lock().map_err(|e| e.to_string())?;
                profiles.iter().find(|p| p.id == ctx.profile_id).cloned()
            };
            if let Some(prof) = profile {
                let archive_cmd = format!(
                    "mkdir -p '{base}/.operon/plan_history' && \
                     cp '{base}/implementation_plan.md' '{base}/.operon/plan_history/plan_{ts}.md' 2>/dev/null || true",
                    base = ctx.remote_path.replace('\'', "'\\''"),
                    ts = now_filename
                );
                let _ = super::ssh::ssh_exec(&prof, &archive_cmd);
            }
        } else {
            // Local: archive to .operon/plan_history/
            let history_dir = std::path::Path::new(&project_path)
                .join(".operon")
                .join("plan_history");
            let _ = std::fs::create_dir_all(&history_dir);
            let archive_name = format!("plan_{}.md", now_filename);
            let plan_path = std::path::Path::new(&project_path).join("implementation_plan.md");
            let _ = std::fs::copy(&plan_path, history_dir.join(&archive_name));
        }
    }

    let (mcp_servers, agent_engine) = {
        let settings = settings_state.settings.lock().map_err(|e| e.to_string())?;
        (settings.mcp_servers.clone(), settings.agent_engine.clone())
    };

    // Generate mcp-config.json — passed to the adapter as `--mcp-config`
    // and also used later (terminal mode) when the local path needs to be
    // swapped for a remote one. `None` means the user has no MCP servers.
    let mcp_config_path = super::mcp::generate_mcp_config(&mcp_servers)?;

    // --- Build the harness adapter for the configured engine ---
    let adapter = crate::harness::pick(&agent_engine);

    // --- Drop a default engine config if missing (local sessions only) ---
    // The adapter writes its engine's native config (OMP's
    // ~/.omp/agent/{models,config}.yml + guardrail hook, or opencode.json),
    // pinning the local provider + model on first run. Remote sessions expect a
    // pre-placed config on the host (TODO: push it for remote too).
    if remote.is_none() {
        let model_for_config = model
            .clone()
            .unwrap_or_else(|| "ollama/kimi-k2.6:cloud".to_string());
        match adapter.ensure_local_config(&project_path, &model_for_config) {
            Ok(true) => eprintln!("[operon] generated {} config for first-run", adapter.id()),
            Ok(false) => {}
            Err(e) => eprintln!("[operon] could not write {} config: {}", adapter.id(), e),
        }
    }

    // --- Build the CLI invocation via the harness adapter ---
    let build_out = adapter.build_command(&crate::harness::BuildContext {
        prompt: &prompt,
        project_path: &project_path,
        session_id: &session_id,
        mode: &mode,
        model: model.as_deref(),
        max_turns,
        resume_session: resume_session.as_deref(),
        permission_mode: &permission_mode,
        existing_plan: &existing_plan,
        now_timestamp: &now_timestamp,
        mcp_config_path: mcp_config_path.as_deref(),
    })?;
    let mut agent_cmd = build_out.command;
    // `build_out.prompt_file` is `Some(...)` for report mode. The caller
    // reconstructs the same path from `session_id` later when SCP'ing it
    // to a remote — no need to thread it through here.
    let _ = build_out.prompt_file;

    eprintln!(
        "[operon] Final agent command (first 200 chars): {}",
        &agent_cmd[..agent_cmd.len().min(200)]
    );

    let shell = agent_shell();

    let use_terminal = use_terminal.unwrap_or(false);

    // --- Persist session metadata so it survives app restarts ---
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    // Derive session name from first ~50 chars of prompt
    let session_name = {
        let trimmed = prompt.trim();
        if trimmed.len() > 50 {
            format!(
                "{}...",
                &trimmed[..trimmed
                    .char_indices()
                    .nth(50)
                    .map(|(i, _)| i)
                    .unwrap_or(trimmed.len())]
            )
        } else {
            trimmed.to_string()
        }
    };

    let meta = SessionMetadata {
        session_id: session_id.clone(),
        agent_session_id: resume_session.clone(),
        project_path: project_path.clone(),
        profile_id: remote.as_ref().map(|r| r.profile_id.clone()),
        remote_path: remote.as_ref().map(|r| r.remote_path.clone()),
        mode: mode.clone(),
        model: model.clone(),
        created_at: now,
        last_activity: now,
        status: "running".to_string(),
        use_terminal,
        terminal_id: terminal_id.clone(),
        name: Some(session_name),
        agent_engine: agent_engine.clone(),
    };
    let _ = save_session_to_disk(&meta);

    // --- TERMINAL MODE: run the agent inside the user's existing terminal session ---
    // This reuses their tmux/compute node/conda environment
    if use_terminal {
        if let (Some(ref ctx), Some(ref tid)) = (&remote, &terminal_id) {
            let profile = {
                let profiles = ssh_state.profiles.lock().map_err(|e| e.to_string())?;
                profiles
                    .iter()
                    .find(|p| p.id == ctx.profile_id)
                    .cloned()
                    .ok_or_else(|| format!("SSH profile {} not found", ctx.profile_id))?
            };

            // For HPC terminal mode, write MCP config to the remote shared filesystem
            // so the agent process on the compute node can access it.
            if let Some(mcp_json) = super::mcp::generate_mcp_config_json(&mcp_servers)? {
                let mcp_config_remote = format!("{}/.operon-mcp-config.json", ctx.remote_path);
                let encoded_json =
                    base64::engine::general_purpose::STANDARD.encode(mcp_json.as_bytes());
                let write_cmd = format!(
                    "echo '{}' | base64 -d > '{}'",
                    encoded_json,
                    mcp_config_remote.replace('\'', "'\\''")
                );
                let _ = super::ssh::ssh_exec(&profile, &write_cmd);
                // Replace the local config path in agent_cmd with the remote path
                if let Some(local_path) = super::mcp::generate_mcp_config(&mcp_servers)? {
                    agent_cmd = agent_cmd.replace(
                        &format!("--mcp-config '{}'", local_path),
                        &format!(
                            "--mcp-config '{}'",
                            mcp_config_remote.replace('\'', "'\\''")
                        ),
                    );
                }
            }

            // For report mode, upload the local prompt file to the remote shared filesystem
            // so the `cat prompt | opencode` command works on the compute node.
            // Uses SCP (with ControlMaster reuse) — reliable for any file size, no encoding issues.
            if mode == "report" {
                let local_prompt_file = std::env::temp_dir()
                    .join(format!("operon-report-prompt-{}.txt", session_id))
                    .to_string_lossy()
                    .to_string();
                let remote_prompt_file = format!(
                    "{}/.operon-report-prompt-{}.txt",
                    ctx.remote_path, session_id
                );
                if std::path::Path::new(&local_prompt_file).exists() {
                    let host_str = format!("{}@{}", profile.user, profile.host);
                    let mut scp_args: Vec<String> = vec![
                        "-o".to_string(),
                        "BatchMode=yes".to_string(),
                        "-o".to_string(),
                        "ConnectTimeout=10".to_string(),
                    ];
                    // Reuse ControlMaster socket if available
                    let sock = crate::platform::ssh_socket_path(
                        &profile.host,
                        profile.port,
                        &profile.user,
                    );
                    if sock.exists() {
                        scp_args.push("-o".to_string());
                        scp_args.push(format!("ControlPath={}", sock.to_string_lossy()));
                    }
                    if profile.port != 22 {
                        scp_args.push("-P".to_string());
                        scp_args.push(profile.port.to_string());
                    }
                    if let Some(key) = &profile.key_file {
                        if std::path::Path::new(key).exists() {
                            scp_args.push("-i".to_string());
                            scp_args.push(key.clone());
                        }
                    }
                    scp_args.push(local_prompt_file.clone());
                    scp_args.push(format!("{}:{}", host_str, remote_prompt_file));

                    let scp_result =
                        hide_window(std::process::Command::new("scp").args(&scp_args)).output();
                    match scp_result {
                        Ok(output) if output.status.success() => {
                            let file_size = std::fs::metadata(&local_prompt_file)
                                .map(|m| m.len())
                                .unwrap_or(0);
                            eprintln!(
                                "[operon] SCP uploaded report prompt to remote: {} ({} bytes)",
                                remote_prompt_file, file_size
                            );
                        }
                        Ok(output) => {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            eprintln!("[operon] SCP upload failed: {}", stderr);
                        }
                        Err(e) => {
                            eprintln!("[operon] SCP command failed: {}", e);
                        }
                    }
                    // Replace the local path in agent_cmd with the remote path
                    agent_cmd = agent_cmd.replace(&local_prompt_file, &remote_prompt_file);
                }
            }

            // Create a unique output file path on the SHARED filesystem (not /tmp which is node-local).
            // On HPC systems, /tmp is local to each node — the compute node writes the file but
            // the tail SSH connects to the login node, which can't see compute-node /tmp.
            // Use the remote working directory which is on a shared NFS/GPFS filesystem.
            let output_file = format!("{}/.operon-{}.jsonl", ctx.remote_path, session_id);
            let done_file = format!("{}/.operon-{}.done", ctx.remote_path, session_id);

            // Write the agent command to a temp script, then `source` it.
            // This keeps the terminal clean (only "source /path/.cf-run.sh" is visible)
            // while preserving the user's shell aliases (unlike piping to `bash`).
            let script_file = format!("{}/.operon-run-{}.sh", ctx.remote_path, session_id);
            // Clean up the report prompt file after the agent finishes (if it exists)
            let prompt_cleanup = if mode == "report" {
                format!(
                    "; rm -f '{}/.operon-report-prompt-{}.txt'",
                    ctx.remote_path.replace('\'', "'\\''"),
                    session_id
                )
            } else {
                String::new()
            };
            // OpenCode reads its config from `opencode.json` in the cwd, so
            // no API-key env vars are needed on the remote.
            let script_content = format!(
                "{}cd '{}' && {} > '{}' 2>&1; echo $? > '{}'{}",
                REMOTE_PATH_PREFIX,
                ctx.remote_path.replace('\'', "'\\''"),
                agent_cmd,
                output_file.replace('\'', "'\\''"),
                done_file.replace('\'', "'\\''"),
                prompt_cleanup,
            );

            // Upload the script to the remote via ssh_exec + base64.
            // Uses chunked transfer to avoid ControlMaster socket message size
            // limits (~256KB). Each chunk is appended to a temp b64 file on the
            // remote, then decoded in one shot.
            {
                let b64_script =
                    base64::engine::general_purpose::STANDARD.encode(script_content.as_bytes());
                let escaped_script = script_file.replace('"', "\\\"");
                let tmp_b64 = format!("{}.__b64__", escaped_script);
                const CHUNK_SIZE: usize = 100_000;

                if b64_script.len() <= CHUNK_SIZE {
                    // Small script — single command
                    let write_cmd = format!(
                        "printf %s {} | base64 -d > \"{}\"",
                        b64_script, escaped_script,
                    );
                    crate::commands::ssh::ssh_exec(&profile, &write_cmd)
                        .map_err(|e| format!("Failed to create run script on remote: {}", e))?;
                } else {
                    // Large script — write base64 in chunks, then decode
                    let mut offset = 0;
                    let mut first = true;
                    while offset < b64_script.len() {
                        let end = std::cmp::min(offset + CHUNK_SIZE, b64_script.len());
                        let chunk = &b64_script[offset..end];
                        let redirect = if first { ">" } else { ">>" };
                        let cmd = format!("printf %s {} {} \"{}\"", chunk, redirect, tmp_b64,);
                        crate::commands::ssh::ssh_exec(&profile, &cmd).map_err(|e| {
                            format!("Failed to upload script chunk to remote: {}", e)
                        })?;
                        first = false;
                        offset = end;
                    }
                    // Decode the assembled base64 and clean up
                    let decode_cmd = format!(
                        "base64 -d \"{}\" > \"{}\" && rm -f \"{}\"",
                        tmp_b64, escaped_script, tmp_b64,
                    );
                    crate::commands::ssh::ssh_exec(&profile, &decode_cmd)
                        .map_err(|e| format!("Failed to decode run script on remote: {}", e))?;
                }
            }

            // Send a short source command to the terminal (the script is already on the remote)
            // The leading space prevents it from appearing in shell history.
            let terminal_cmd = format!(
                " clear; source '{}'; rm -f '{}'\n",
                script_file.replace('\'', "'\\''"),
                script_file.replace('\'', "'\\''"),
            );

            // Write the command into the existing terminal
            let encoded = terminal_cmd.as_bytes().to_vec();
            {
                let terminals = terminal_state.terminals.lock().map_err(|e| e.to_string())?;
                let handle = terminals
                    .get(tid)
                    .ok_or_else(|| format!("Terminal {} not found", tid))?;
                let mut writer = handle.writer.lock().map_err(|e| e.to_string())?;
                use std::io::Write;
                writer.write_all(&encoded).map_err(|e| e.to_string())?;
                writer.flush().map_err(|e| e.to_string())?;
            }

            // Now tail the output file via a separate SSH connection to stream results back.
            // Reuse ControlMaster socket if available — avoids re-authentication (critical
            // for HPC clusters with Duo MFA where a second auth would block/fail).
            let mut ssh_tail_args = format!(
                "ssh -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -o TCPKeepAlive=yes {}@{} -p {}",
                profile.user, profile.host, profile.port
            );
            // Reuse ControlMaster socket if one exists from the main terminal connection
            let ctrl_sock =
                crate::platform::ssh_socket_path(&profile.host, profile.port, &profile.user);
            if ctrl_sock.exists() {
                ssh_tail_args.push_str(&format!(
                    " -o \"ControlPath={}\"",
                    ctrl_sock.to_string_lossy()
                ));
            }
            if let Some(key) = &profile.key_file {
                ssh_tail_args.push_str(&format!(" -i {}", key));
            }
            // Wait for the output file to appear, then tail -f it.
            // Use base64 encoding to completely avoid all shell quoting/expansion issues
            // across the local shell → SSH → remote shell → bash -c chain.
            // The tail script streams the JSONL output file back to the local machine.
            // Key fixes for reliability:
            //   1. Use stdbuf/unbuffer to force line-buffered output through SSH pipe.
            //      Without this, SSH block-buffers stdout (4KB) so small JSON lines
            //      accumulate silently, causing the "thinking but not responding" symptom.
            //   2. Use tail --pid or a polling loop so tail exits promptly when done.
            //   3. Read any remaining lines after tail exits (tail -f may miss the last write).
            let tail_script = format!(
                "i=0; while [ ! -f '{}' ] && [ \"$i\" -lt 1500 ]; do sleep 0.2; i=$((i+1)); done; \
                 if [ ! -f '{}' ]; then echo '{{\"type\":\"error\",\"error\":{{\"message\":\"Output file did not appear after 5 minutes. The command may have failed to start — check the terminal.\"}}}}'; exit 1; fi; \
                 if command -v stdbuf >/dev/null 2>&1; then \
                   TAIL_CMD=\"stdbuf -oL tail -f '{}'\"; \
                 else \
                   TAIL_CMD=\"tail -f '{}'\"; \
                 fi; \
                 eval $TAIL_CMD & TAIL_PID=$!; \
                 ( while [ ! -f '{}' ]; do sleep 30; printf '{{\"type\":\"heartbeat\"}}\\n'; done ) & HB_PID=$!; \
                 while [ ! -f '{}' ]; do sleep 0.5; done; \
                 sleep 0.5; kill $TAIL_PID $HB_PID 2>/dev/null; wait $TAIL_PID $HB_PID 2>/dev/null; \
                 cat '{}'; \
                 rm -f '{}' '{}'",
                output_file, output_file,
                output_file.replace('\'', "'\\''"),
                output_file.replace('\'', "'\\''"),
                done_file,
                done_file,
                output_file.replace('\'', "'\\''"),
                output_file, done_file,
            );
            // Base64-encode the script and have the REMOTE shell decode+execute it.
            // This avoids ALL quoting issues: local shell sees only safe base64 chars.
            let b64_tail = base64::engine::general_purpose::STANDARD.encode(tail_script.as_bytes());
            // The remote command: echo <b64> | base64 -d | bash
            // We pass this directly to SSH (no -- bash -c wrapper needed).
            // SSH sends its args as a single command string to the remote shell.
            ssh_tail_args.push_str(&format!(" \"echo {} | base64 -d | bash\"", b64_tail));

            let mut tail_cmd = AsyncCommand::new(&shell);
            tail_cmd.arg("-l").arg("-c").arg(&ssh_tail_args);
            tail_cmd.stdout(std::process::Stdio::piped());
            tail_cmd.stderr(std::process::Stdio::piped());
            hide_window_async(&mut tail_cmd);

            let mut child = tail_cmd
                .spawn()
                .map_err(|e| format!("Failed to start tail: {}", e))?;
            let stdout = child.stdout.take().ok_or("Failed to capture tail stdout")?;
            let stderr = child.stderr.take();

            // Store as a session so it can be stopped
            state
                .sessions
                .lock()
                .map_err(|e| e.to_string())?
                .insert(session_id.clone(), AgentSession { child });

            // Stream stdout (JSON lines from the output file)
            let app_handle = app.clone();
            let sid = session_id.clone();
            let engine = agent_engine.clone();
                    tokio::spawn(async move {
                let adapter = crate::harness::pick(&engine);
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let Some(canonical) = adapter.normalize_line(&line) else {
                        continue;
                    };
                    let _ = app_handle.emit(
                        &format!("agent-event-{}", sid),
                        serde_json::json!({ "line": canonical }),
                    );
                }
                let _ = app_handle.emit(&format!("agent-done-{}", sid), serde_json::json!({}));
            });

            // Handle stderr (suppress SSH warnings)
            if let Some(stderr) = stderr {
                let app_handle2 = app.clone();
                let sid2 = session_id.clone();
                tokio::spawn(async move {
                    let reader = BufReader::new(stderr);
                    let mut lines = reader.lines();
                    let mut error_buf = String::new();
                    while let Ok(Some(line)) = lines.next_line().await {
                        if !line.trim().is_empty() {
                            error_buf.push_str(&line);
                            error_buf.push('\n');
                        }
                    }
                    let trimmed = error_buf.trim();
                    if !trimmed.is_empty() {
                        let is_just_warning = trimmed.lines().all(|l| {
                            let lt = l.trim().trim_start_matches('*').trim();
                            lt.is_empty()
                                || lt.contains("WARNING")
                                || lt.contains("Warning")
                                || lt.contains("warning")
                                || lt.contains("sntrup")
                                || lt.contains("mlkem")
                                || lt.contains("post-quantum")
                                || lt.contains("quantum")
                                || lt.contains("vulnerable")
                                || lt.contains("decrypt later")
                                || lt.contains("upgraded")
                                || lt.contains("openssh.com")
                                || lt.contains("store now")
                                || lt.contains("key exchange")
                                || lt.contains("no stdin data")
                                || lt.contains("redirect stdin")
                                || lt.contains("piping from")
                                || lt.contains("/dev/null")
                                || lt.contains("wait longer")
                                || lt.contains("proceeding without")
                                || lt.contains("Connection to")
                                || lt.contains("Killed by signal")
                                || lt.contains("Transferred:")
                                || lt.contains("kex_exchange")
                                || lt.contains("banner")
                                || lt.starts_with("debug")
                                || lt.contains("file truncated")
                                || lt.contains("tail:")
                        });
                        if !is_just_warning {
                            let _ = app_handle2.emit(
                                &format!("agent-event-{}", sid2),
                                serde_json::json!({
                                    "line": format!(
                                        "{{\"type\":\"error\",\"error\":{{\"message\":\"{}\"}}}}",
                                        trimmed.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
                                    )
                                }),
                            );
                        }
                    }
                });
            }

            return Ok(());
        } else {
            return Err(
                "Terminal mode requires a remote connection and an active terminal".to_string(),
            );
        }
    }

    // Decide: local or remote execution
    let mut cmd = if let Some(ref ctx) = remote {
        // --- REMOTE: run opencode via SSH on the remote server ---
        let profile = {
            let profiles = ssh_state.profiles.lock().map_err(|e| e.to_string())?;
            profiles
                .iter()
                .find(|p| p.id == ctx.profile_id)
                .cloned()
                .ok_or_else(|| format!("SSH profile {} not found", ctx.profile_id))?
        };

        // Step 1: Figure out how to invoke opencode on the remote server.
        // It might be a binary in PATH or installed at a common location.
        let bin = adapter.remote_bin_name();
        let find_agent_cmd = format!(
            r#"
            # 1. Check for a real binary at common install locations
            for p in \
                "$HOME/.local/bin/{bin}" \
                "$HOME/.omp/bin/{bin}" \
                "$HOME/.opencode/bin/{bin}" \
                "$HOME/.npm-global/bin/{bin}" \
                "$HOME/.npm/bin/{bin}" \
                "$HOME/bin/{bin}" \
                "$HOME/.yarn/bin/{bin}" \
                "$HOME/.bun/bin/{bin}" \
                /usr/local/bin/{bin}; do
                [ -x "$p" ] && echo "$p" && exit 0
            done
            # Check NVM paths
            for p in "$HOME"/.nvm/versions/node/*/bin/{bin}; do
                [ -x "$p" ] && echo "$p" && exit 0
            done

            # 2. Source profile files to get full PATH
            # Set PS1 to trick .bashrc into thinking this is interactive
            # (most .bashrc files have: [ -z "$PS1" ] && return)
            export PS1=x
            . "$HOME/.profile" 2>/dev/null
            . "$HOME/.bash_profile" 2>/dev/null
            . "$HOME/.bashrc" 2>/dev/null
            . "$HOME/.nvm/nvm.sh" 2>/dev/null

            # 3. Check if it's a real binary via which
            w=$(which {bin} 2>/dev/null)
            if [ -n "$w" ] && [ -x "$w" ]; then
                echo "$w"
                exit 0
            fi

            echo ""
        "#,
            bin = bin
        );
        let agent_resolve = super::ssh::ssh_exec(&profile, &find_agent_cmd).unwrap_or_default();
        let agent_resolve = agent_resolve.trim().to_string();

        if agent_resolve.is_empty() || agent_resolve.contains("not found") {
            return Err(format!(
                "{} CLI not found on the remote server. Install it with: {}",
                adapter.remote_bin_name(),
                adapter.install_hint()
            ));
        }

        // Step 2: Replace `opencode` with the resolved absolute path
        let agent_invoke = agent_resolve.clone();

        // For report mode, upload the prompt file to the remote server via SCP
        if mode == "report" {
            let local_prompt_file = std::env::temp_dir()
                .join(format!("operon-report-prompt-{}.txt", session_id))
                .to_string_lossy()
                .to_string();
            let remote_prompt_file = format!(
                "{}/.operon-report-prompt-{}.txt",
                ctx.remote_path, session_id
            );
            if std::path::Path::new(&local_prompt_file).exists() {
                let host_str = format!("{}@{}", profile.user, profile.host);
                let mut scp_args: Vec<String> = vec![
                    "-o".to_string(),
                    "BatchMode=yes".to_string(),
                    "-o".to_string(),
                    "ConnectTimeout=10".to_string(),
                ];
                let sock =
                    crate::platform::ssh_socket_path(&profile.host, profile.port, &profile.user);
                if sock.exists() {
                    scp_args.push("-o".to_string());
                    scp_args.push(format!("ControlPath={}", sock.to_string_lossy()));
                }
                if profile.port != 22 {
                    scp_args.push("-P".to_string());
                    scp_args.push(profile.port.to_string());
                }
                if let Some(key) = &profile.key_file {
                    if std::path::Path::new(key).exists() {
                        scp_args.push("-i".to_string());
                        scp_args.push(key.clone());
                    }
                }
                scp_args.push(local_prompt_file.clone());
                scp_args.push(format!("{}:{}", host_str, remote_prompt_file));

                match hide_window(std::process::Command::new("scp").args(&scp_args)).output() {
                    Ok(output) if output.status.success() => {
                        let file_size = std::fs::metadata(&local_prompt_file)
                            .map(|m| m.len())
                            .unwrap_or(0);
                        eprintln!(
                            "[operon] SCP uploaded report prompt: {} ({} bytes)",
                            remote_prompt_file, file_size
                        );
                    }
                    Ok(output) => {
                        eprintln!(
                            "[operon] SCP upload failed: {}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }
                    Err(e) => {
                        eprintln!("[operon] SCP command failed: {}", e);
                    }
                }
                agent_cmd = agent_cmd.replace(&local_prompt_file, &remote_prompt_file);
            }
        }

        let agent_cmd_abs = agent_cmd.replacen(
            &format!("{} ", adapter.remote_bin_name()),
            &format!("{} ", agent_invoke),
            1,
        );

        // Step 3: Build the remote command — source profile for PATH
        // then cd to the working directory and run opencode.
        // For report mode, the command is `cat file | opencode ...` — don't redirect stdin from /dev/null.
        // For other modes, redirect stdin to prevent the agent from hanging waiting for input.
        let stdin_redirect = if mode == "report" { "" } else { " < /dev/null" };
        // OpenCode reads its provider config from `opencode.json` in the
        // remote cwd, so no env-var forwarding is needed.
        let remote_cmd = format!(
            "export PS1=x; . \"$HOME/.profile\" 2>/dev/null; . \"$HOME/.bash_profile\" 2>/dev/null; . \"$HOME/.bashrc\" 2>/dev/null; . \"$HOME/.nvm/nvm.sh\" 2>/dev/null; cd '{}' && {}{}",
            ctx.remote_path.replace('\'', "'\\''"),
            agent_cmd_abs,
            stdin_redirect
        );

        // Base64-encode to avoid nested quoting issues
        let encoded_cmd = base64::engine::general_purpose::STANDARD.encode(remote_cmd.as_bytes());

        // No -tt flag! We need clean stdout for JSON parsing, not a PTY.
        let mut ssh_args = format!(
            "ssh -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -o TCPKeepAlive=yes {}@{} -p {}",
            profile.user, profile.host, profile.port
        );
        // Reuse ControlMaster socket if available (avoids re-auth on Duo MFA clusters)
        let ctrl_sock =
            crate::platform::ssh_socket_path(&profile.host, profile.port, &profile.user);
        if ctrl_sock.exists() {
            ssh_args.push_str(&format!(
                " -o \"ControlPath={}\"",
                ctrl_sock.to_string_lossy()
            ));
        }
        if let Some(key) = &profile.key_file {
            ssh_args.push_str(&format!(" -i {}", key));
        }
        // Decode and execute on the remote side
        ssh_args.push_str(&format!(
            " -- bash -c \"$(echo {} | base64 -d)\"",
            encoded_cmd
        ));

        let mut c = AsyncCommand::new(&shell);
        c.arg("-l").arg("-c").arg(&ssh_args);
        c
    } else {
        // --- LOCAL: run opencode directly ---
        let mut c = AsyncCommand::new(&shell);
        c.arg("-l").arg("-c").arg(&agent_cmd);
        c.current_dir(&project_path);
        c
    };

    // On Windows the agent needs Git Bash (POSIX environment).
    if let Some(git_bash_path) = crate::platform::find_git_bash_path() {
        cmd.env("CLAUDE_CODE_GIT_BASH_PATH", &git_bash_path);
    }

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    hide_window_async(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start agent: {}", e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture stdout".to_string())?;

    let stderr = child.stderr.take();

    // Store session
    state
        .sessions
        .lock()
        .map_err(|e| e.to_string())?
        .insert(session_id.clone(), AgentSession { child });

    // Spawn stdout reader task
    let app_handle = app.clone();
    let sid = session_id.clone();
    // Persist output to .jsonl file so sessions can be resumed/reconnected.
    // For local sessions this was previously missing — output was only streamed live.
    let output_jsonl_path = format!("{}/.operon-{}.jsonl", project_path, session_id);
    let done_marker_path = format!("{}/.operon-{}.done", project_path, session_id);

    let engine = agent_engine.clone();
    tokio::spawn(async move {
        let adapter = crate::harness::pick(&engine);
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        // Open the output file for appending (create if needed)
        let mut output_file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&output_jsonl_path)
            .await
            .ok();

        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }

            // Translate to canonical AgentEvent shape (identity for OpenCode
            // adapter).
            let Some(canonical) = adapter.normalize_line(&line) else {
                continue;
            };

            // Emit the canonical JSON line to frontend for parsing
            let _ = app_handle.emit(
                &format!("agent-event-{}", sid),
                serde_json::json!({ "line": canonical }),
            );

            // Persist canonical form to disk so resume hydrates the same view
            if let Some(ref mut f) = output_file {
                use tokio::io::AsyncWriteExt;
                let _ = f.write_all(canonical.as_bytes()).await;
                let _ = f.write_all(b"\n").await;
            }
        }

        // Stream ended — write done marker and emit event
        let _ = tokio::fs::write(&done_marker_path, "done").await;
        let _ = app_handle.emit(&format!("agent-done-{}", sid), serde_json::json!({}));
    });

    // Spawn stderr reader task — surface SSH/remote errors to the frontend
    if let Some(stderr) = stderr {
        let app_handle2 = app.clone();
        let sid2 = session_id.clone();

        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            let mut error_buf = String::new();

            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    error_buf.push_str(&line);
                    error_buf.push('\n');
                }
            }

            // If there was meaningful stderr output, send it as an error event
            let trimmed = error_buf.trim();
            if !trimmed.is_empty() {
                // Filter out common SSH warnings (post-quantum key exchange, etc.)
                let is_just_warning = trimmed.lines().all(|l| {
                    let lt = l.trim().trim_start_matches('*').trim();
                    lt.is_empty()
                        || lt.contains("WARNING")
                        || lt.contains("Warning")
                        || lt.contains("warning")
                        || lt.contains("sntrup")
                        || lt.contains("mlkem")
                        || lt.contains("post-quantum")
                        || lt.contains("quantum")
                        || lt.contains("vulnerable")
                        || lt.contains("decrypt later")
                        || lt.contains("upgraded")
                        || lt.contains("openssh.com")
                        || lt.contains("store now")
                        || lt.contains("key exchange")
                        || lt.contains("no stdin data")
                        || lt.contains("redirect stdin")
                        || lt.contains("piping from")
                        || lt.contains("/dev/null")
                        || lt.contains("wait longer")
                        || lt.contains("proceeding without")
                        || lt.contains("file truncated")
                        || lt.contains("tail:")
                });

                if !is_just_warning {
                    let _ = app_handle2.emit(
                        &format!("agent-event-{}", sid2),
                        serde_json::json!({
                            "line": format!(
                                "{{\"type\":\"error\",\"error\":{{\"message\":\"{}\"}}}}",
                                trimmed.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
                            )
                        }),
                    );
                }
            }
        });
    }

    Ok(())
}

#[tauri::command]
pub async fn stop_agent_session(
    state: tauri::State<'_, AgentManager>,
    session_id: String,
) -> Result<(), String> {
    // Extract session from lock first, then await kill — never hold Mutex across .await
    let session = {
        let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        sessions.remove(&session_id)
    };

    if let Some(mut session) = session {
        let _ = session.child.kill().await;
    }

    Ok(())
}

/// Check if an implementation_plan.md exists in the given directory (local or remote).
/// Returns the plan content if found, or an empty string if not.
#[tauri::command]
pub async fn check_existing_plan(
    ssh_state: tauri::State<'_, super::ssh::SSHManager>,
    project_path: String,
    remote: Option<RemoteContext>,
) -> Result<String, String> {
    if let Some(ctx) = remote {
        // Remote: check via SSH
        let profile = {
            let profiles = ssh_state.profiles.lock().map_err(|e| e.to_string())?;
            profiles
                .iter()
                .find(|p| p.id == ctx.profile_id)
                .cloned()
                .ok_or_else(|| format!("SSH profile {} not found", ctx.profile_id))?
        };
        let check_cmd = format!(
            "cat '{}'/implementation_plan.md 2>/dev/null || echo ''",
            ctx.remote_path.replace('\'', "'\\''")
        );
        let content = super::ssh::ssh_exec(&profile, &check_cmd).unwrap_or_default();
        Ok(content.trim().to_string())
    } else {
        // Local
        let plan_path = std::path::Path::new(&project_path).join("implementation_plan.md");
        let content = std::fs::read_to_string(&plan_path).unwrap_or_default();
        Ok(content.trim().to_string())
    }
}

/// Archive the current implementation_plan.md to .operon/plan_history/ before a new plan is written.
/// Called by the frontend before starting a plan session, so archival happens regardless of
/// what mode string the backend receives.
/// Returns Ok(true) if a plan was archived, Ok(false) if there was no plan to archive.
#[tauri::command]
pub async fn archive_current_plan(
    ssh_state: tauri::State<'_, super::ssh::SSHManager>,
    project_path: String,
    remote: Option<RemoteContext>,
) -> Result<bool, String> {
    // Generate timestamp for the archive filename
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    let mut y = 1970i64;
    let mut remaining = days as i64;
    loop {
        let days_in_year = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
            366
        } else {
            365
        };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0usize;
    for &md in &month_days {
        if remaining < md as i64 {
            break;
        }
        remaining -= md as i64;
        m += 1;
    }
    let ts = format!(
        "{:04}-{:02}-{:02}_{:02}{:02}{:02}_UTC",
        y,
        m + 1,
        remaining + 1,
        hours,
        minutes,
        seconds
    );

    if let Some(ctx) = remote {
        let profile = {
            let profiles = ssh_state.profiles.lock().map_err(|e| e.to_string())?;
            profiles.iter().find(|p| p.id == ctx.profile_id).cloned()
        };
        if let Some(prof) = profile {
            let base = ctx.remote_path.replace('\'', "'\\''");
            // Check if plan exists, archive it, then return
            let cmd = format!(
                "if [ -f '{base}/implementation_plan.md' ]; then \
                     mkdir -p '{base}/.operon/plan_history' && \
                     cp '{base}/implementation_plan.md' '{base}/.operon/plan_history/plan_{ts}.md' && \
                     echo 'ARCHIVED'; \
                 else echo 'NO_PLAN'; fi"
            );
            let result = super::ssh::ssh_exec(&prof, &cmd).unwrap_or_default();
            return Ok(result.contains("ARCHIVED"));
        }
        Ok(false)
    } else {
        let plan_path = std::path::Path::new(&project_path).join("implementation_plan.md");
        if plan_path.is_file() {
            let history_dir = std::path::Path::new(&project_path)
                .join(".operon")
                .join("plan_history");
            std::fs::create_dir_all(&history_dir)
                .map_err(|e| format!("Failed to create plan_history dir: {}", e))?;
            let archive_name = format!("plan_{}.md", ts);
            std::fs::copy(&plan_path, history_dir.join(&archive_name))
                .map_err(|e| format!("Failed to archive plan: {}", e))?;
            eprintln!(
                "[operon] Archived implementation_plan.md → .operon/plan_history/{}",
                archive_name
            );
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Archived plan entry returned to the frontend.
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct PlanHistoryEntry {
    pub filename: String,
    pub timestamp: String, // e.g. "2026-03-29 14:30:05"
    pub title: String,     // first heading or "Untitled Plan"
    pub lines: u64,
    pub path: String, // full path to the archived file
}

/// List all archived plans from .operon/plan_history/, newest first.
#[tauri::command]
pub async fn list_plan_history(project_path: String) -> Result<Vec<PlanHistoryEntry>, String> {
    let history_dir = std::path::Path::new(&project_path)
        .join(".operon")
        .join("plan_history");
    if !history_dir.is_dir() {
        return Ok(vec![]);
    }

    let mut entries: Vec<PlanHistoryEntry> = Vec::new();
    let dir = std::fs::read_dir(&history_dir).map_err(|e| e.to_string())?;
    for entry in dir.flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();
        if !fname.starts_with("plan_") || !fname.ends_with(".md") {
            continue;
        }
        // Parse timestamp from filename: plan_YYYY-MM-DD_HHMMSS.md
        let ts_part = fname.trim_start_matches("plan_").trim_end_matches(".md");
        let timestamp = ts_part
            .replacen('_', " ", 1) // "2026-03-29 143005"
            .chars()
            .enumerate()
            .map(|(i, c)| {
                // Insert colons into HHMMSS → HH:MM:SS
                if i == 13 || i == 15 {
                    ':'
                } else {
                    c
                }
            })
            .collect::<String>();

        let full_path = entry.path();
        let content = std::fs::read_to_string(&full_path).unwrap_or_default();
        let line_count = content.lines().count() as u64;

        // Extract title from first heading
        let title = content
            .lines()
            .find(|l| l.starts_with("# "))
            .map(|l| l.trim_start_matches("# ").trim().to_string())
            .unwrap_or_else(|| "Untitled Plan".to_string());

        entries.push(PlanHistoryEntry {
            filename: fname,
            timestamp,
            title,
            lines: line_count,
            path: full_path.to_string_lossy().to_string(),
        });
    }

    // Sort newest first
    entries.sort_by(|a, b| b.filename.cmp(&a.filename));
    Ok(entries)
}

/// Read the content of a specific archived plan.
#[tauri::command]
pub async fn read_plan_history_entry(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("Failed to read plan: {}", e))
}

// --- Session Management Commands ---

/// Save session metadata to disk. Called by frontend after session starts or updates.
#[tauri::command]
pub async fn save_session_metadata(metadata: SessionMetadata) -> Result<(), String> {
    save_session_to_disk(&metadata)
}

/// Update the agent_session_id for an existing session (called when we capture it from stream).
#[tauri::command]
pub async fn update_session_agent_id(
    session_id: String,
    agent_session_id: String,
) -> Result<(), String> {
    if let Some(mut meta) = load_session_from_disk(&session_id)? {
        meta.agent_session_id = Some(agent_session_id);
        meta.last_activity = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        save_session_to_disk(&meta)
    } else {
        Err(format!("Session {} not found", session_id))
    }
}

/// Mark a session as completed or failed.
#[tauri::command]
pub async fn update_session_status(session_id: String, status: String) -> Result<(), String> {
    if let Some(mut meta) = load_session_from_disk(&session_id)? {
        meta.status = status;
        meta.last_activity = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        save_session_to_disk(&meta)
    } else {
        Err(format!("Session {} not found", session_id))
    }
}

/// List sessions for a given project path (local or remote).
/// Returns sessions sorted by most recent first.
#[tauri::command]
pub async fn list_sessions(
    project_path: Option<String>,
    profile_id: Option<String>,
) -> Result<Vec<SessionMetadata>, String> {
    let all = load_all_sessions_from_disk();
    let filtered: Vec<SessionMetadata> = all
        .into_iter()
        .filter(|s| {
            // Filter by project path or profile if provided
            let path_match = project_path.as_ref().is_none_or(|p| {
                s.project_path == *p || s.remote_path.as_deref() == Some(p.as_str())
            });
            let profile_match = profile_id
                .as_ref()
                .is_none_or(|pid| s.profile_id.as_deref() == Some(pid.as_str()));
            path_match && profile_match
        })
        .collect();
    Ok(filtered)
}

/// Check the status of a session's output files on the filesystem (local or remote).
#[tauri::command]
pub async fn check_session_files(
    ssh_state: tauri::State<'_, super::ssh::SSHManager>,
    session_id: String,
    remote: Option<RemoteContext>,
) -> Result<SessionFileStatus, String> {
    // Load session metadata to find the output file path
    let meta = load_session_from_disk(&session_id)?
        .ok_or_else(|| format!("Session {} not found", session_id))?;

    let base_path = meta.remote_path.as_deref().unwrap_or(&meta.project_path);
    let output_file = format!("{}/.operon-{}.jsonl", base_path, session_id);
    let done_file = format!("{}/.operon-{}.done", base_path, session_id);

    if let Some(ctx) = remote {
        // Remote: check via SSH
        let profile = {
            let profiles = ssh_state.profiles.lock().map_err(|e| e.to_string())?;
            profiles
                .iter()
                .find(|p| p.id == ctx.profile_id)
                .cloned()
                .ok_or_else(|| format!("SSH profile {} not found", ctx.profile_id))?
        };
        let check_cmd = format!(
            "echo -n \"output:\"; test -f '{}' && echo 'yes' || echo 'no'; \
             echo -n \"done:\"; test -f '{}' && echo 'yes' || echo 'no'",
            output_file.replace('\'', "'\\''"),
            done_file.replace('\'', "'\\''"),
        );
        let result = super::ssh::ssh_exec(&profile, &check_cmd).unwrap_or_default();
        let output_exists = result.contains("output:yes");
        let done_exists = result.contains("done:yes");
        Ok(SessionFileStatus {
            session_id,
            output_exists,
            done_exists,
            is_running: output_exists && !done_exists,
            is_completed: output_exists && done_exists,
        })
    } else {
        // Local
        let output_exists = std::path::Path::new(&output_file).exists();
        let done_exists = std::path::Path::new(&done_file).exists();
        Ok(SessionFileStatus {
            session_id,
            output_exists,
            done_exists,
            is_running: output_exists && !done_exists,
            is_completed: output_exists && done_exists,
        })
    }
}

/// Read the full output of a completed session (.jsonl file).
/// Returns the raw content for the frontend to parse into messages.
#[tauri::command]
pub async fn read_session_output(
    ssh_state: tauri::State<'_, super::ssh::SSHManager>,
    session_id: String,
    remote: Option<RemoteContext>,
) -> Result<String, String> {
    let meta = load_session_from_disk(&session_id)?
        .ok_or_else(|| format!("Session {} not found", session_id))?;

    let base_path = meta.remote_path.as_deref().unwrap_or(&meta.project_path);
    let output_file = format!("{}/.operon-{}.jsonl", base_path, session_id);

    if let Some(ctx) = remote {
        let profile = {
            let profiles = ssh_state.profiles.lock().map_err(|e| e.to_string())?;
            profiles
                .iter()
                .find(|p| p.id == ctx.profile_id)
                .cloned()
                .ok_or_else(|| format!("SSH profile {} not found", ctx.profile_id))?
        };
        let cat_cmd = format!("cat '{}'", output_file.replace('\'', "'\\''"));
        let content = super::ssh::ssh_exec(&profile, &cat_cmd)
            .map_err(|e| format!("Failed to read session output: {}", e))?;
        Ok(content)
    } else {
        std::fs::read_to_string(&output_file)
            .map_err(|e| format!("Failed to read session output: {}", e))
    }
}

/// Reconnect to a running session by tailing the .jsonl file.
/// This spawns a tail process and streams events back to the frontend.
#[tauri::command]
pub async fn reconnect_session(
    state: tauri::State<'_, AgentManager>,
    ssh_state: tauri::State<'_, super::ssh::SSHManager>,
    app: tauri::AppHandle,
    session_id: String,       // The old session's ID (to find the files)
    event_session_id: String, // The current frontend session ID (for event channels)
    remote: Option<RemoteContext>,
) -> Result<(), String> {
    let meta = load_session_from_disk(&session_id)?
        .ok_or_else(|| format!("Session {} not found", session_id))?;

    let base_path = meta.remote_path.as_deref().unwrap_or(&meta.project_path);
    let output_file = format!("{}/.operon-{}.jsonl", base_path, session_id);
    let done_file = format!("{}/.operon-{}.done", base_path, session_id);

    let shell = agent_shell();

    if let Some(ctx) = remote {
        let profile = {
            let profiles = ssh_state.profiles.lock().map_err(|e| e.to_string())?;
            profiles
                .iter()
                .find(|p| p.id == ctx.profile_id)
                .cloned()
                .ok_or_else(|| format!("SSH profile {} not found", ctx.profile_id))?
        };

        // Build SSH command to tail the output file
        let mut ssh_tail_args = format!(
            "ssh -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -o TCPKeepAlive=yes {}@{} -p {}",
            profile.user, profile.host, profile.port
        );
        if let Some(key) = &profile.key_file {
            ssh_tail_args.push_str(&format!(" -i {}", key));
        }

        // Tail script: first cat any existing content, then tail -f for new lines
        // If done file already exists, just cat and exit (session already finished)
        let tail_script = format!(
            "if [ -f '{}' ]; then cat '{}'; exit 0; fi; \
             if [ ! -f '{}' ]; then echo '{{\"type\":\"error\",\"error\":{{\"message\":\"Output file not found\"}}}}'; exit 1; fi; \
             cat '{}'; tail -f -n +$(wc -l < '{}' | tr -d ' ') '{}' & TAIL_PID=$!; \
             while [ ! -f '{}' ]; do sleep 1; done; \
             sleep 1; kill $TAIL_PID 2>/dev/null; wait $TAIL_PID 2>/dev/null",
            done_file, output_file,
            output_file,
            output_file, output_file, output_file,
            done_file,
        );
        let b64_tail = base64::engine::general_purpose::STANDARD.encode(tail_script.as_bytes());
        ssh_tail_args.push_str(&format!(" \"echo {} | base64 -d | bash\"", b64_tail));

        let mut tail_cmd = AsyncCommand::new(&shell);
        tail_cmd.arg("-l").arg("-c").arg(&ssh_tail_args);
        tail_cmd.stdout(std::process::Stdio::piped());
        tail_cmd.stderr(std::process::Stdio::piped());
        hide_window_async(&mut tail_cmd);

        let mut child = tail_cmd
            .spawn()
            .map_err(|e| format!("Failed to reconnect: {}", e))?;
        let stdout = child
            .stdout
            .take()
            .ok_or("Failed to capture reconnect stdout")?;

        // Store as a session so it can be stopped
        state
            .sessions
            .lock()
            .map_err(|e| e.to_string())?
            .insert(event_session_id.clone(), AgentSession { child });

        // Stream output to frontend using the CURRENT frontend session ID for events.
        // Remote `.jsonl` is the raw agent output, so we still translate.
        let app_handle = app.clone();
        let sid = event_session_id.clone();
        let engine = meta.agent_engine.clone();
            tokio::spawn(async move {
            let adapter = crate::harness::pick(&engine);
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                let Some(canonical) = adapter.normalize_line(&line) else {
                    continue;
                };
                let _ = app_handle.emit(
                    &format!("agent-event-{}", sid),
                    serde_json::json!({ "line": canonical }),
                );
            }
            let _ = app_handle.emit(&format!("agent-done-{}", sid), serde_json::json!({}));
        });

        Ok(())
    } else {
        // Local reconnect — just read the file
        let content = std::fs::read_to_string(&output_file)
            .map_err(|e| format!("Failed to read output: {}", e))?;
        for line in content.lines() {
            if !line.trim().is_empty() {
                let _ = app.emit(
                    &format!("agent-event-{}", event_session_id),
                    serde_json::json!({ "line": line }),
                );
            }
        }
        let _ = app.emit(
            &format!("agent-done-{}", event_session_id),
            serde_json::json!({}),
        );
        Ok(())
    }
}

/// Reconnect a stalled tail SSH connection without stopping the agent.
/// Kills the existing tail process for this session and spawns a fresh one
/// that cats existing output then tail -f's for new lines.
#[tauri::command]
pub async fn reconnect_tail(
    state: tauri::State<'_, AgentManager>,
    ssh_state: tauri::State<'_, super::ssh::SSHManager>,
    app: tauri::AppHandle,
    session_id: String,
    remote: Option<RemoteContext>,
) -> Result<(), String> {
    // 1. Kill the stalled tail process (but NOT the agent on the compute node)
    let old_session = {
        let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        sessions.remove(&session_id)
    };
    if let Some(mut old) = old_session {
        let _ = old.child.kill().await;
    }

    // 2. Figure out the file paths from session metadata or remote context
    let ctx = remote.ok_or("Reconnect tail is only supported for remote sessions")?;
    let profile = {
        let profiles = ssh_state.profiles.lock().map_err(|e| e.to_string())?;
        profiles
            .iter()
            .find(|p| p.id == ctx.profile_id)
            .cloned()
            .ok_or_else(|| format!("SSH profile {} not found", ctx.profile_id))?
    };

    let output_file = format!("{}/.operon-{}.jsonl", ctx.remote_path, session_id);
    let done_file = format!("{}/.operon-{}.done", ctx.remote_path, session_id);
    let shell = agent_shell();

    // 3. Build a fresh SSH tail command with tighter keepalives
    let mut ssh_tail_args = format!(
        "ssh -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -o TCPKeepAlive=yes {}@{} -p {}",
        profile.user, profile.host, profile.port
    );
    let ctrl_sock = crate::platform::ssh_socket_path(&profile.host, profile.port, &profile.user);
    if ctrl_sock.exists() {
        ssh_tail_args.push_str(&format!(
            " -o \"ControlPath={}\"",
            ctrl_sock.to_string_lossy()
        ));
    }
    if let Some(key) = &profile.key_file {
        ssh_tail_args.push_str(&format!(" -i {}", key));
    }

    // Tail script: cat existing content, then tail -f for new lines.
    // If the done file already exists the session finished while we were stalled — just cat.
    // Uses stdbuf -oL to force line-buffered output and prevent SSH pipe buffering.
    let tail_script = format!(
        "if [ -f '{}' ]; then cat '{}'; exit 0; fi; \
         if [ ! -f '{}' ]; then echo '{{\"type\":\"error\",\"error\":{{\"message\":\"Output file not found — the agent may have finished or the file was cleaned up.\"}}}}'; exit 1; fi; \
         if command -v stdbuf >/dev/null 2>&1; then \
           stdbuf -oL cat '{}'; stdbuf -oL tail -f -n +$(($(wc -l < '{}' | tr -d ' ') + 1)) '{}' & TAIL_PID=$!; \
         else \
           cat '{}'; tail -f -n +$(($(wc -l < '{}' | tr -d ' ') + 1)) '{}' & TAIL_PID=$!; \
         fi; \
         ( while [ ! -f '{}' ]; do sleep 30; printf '{{\"type\":\"heartbeat\"}}\\n'; done ) & HB_PID=$!; \
         while [ ! -f '{}' ]; do sleep 0.5; done; \
         sleep 0.5; kill $TAIL_PID $HB_PID 2>/dev/null; wait $TAIL_PID $HB_PID 2>/dev/null; \
         cat '{}'",
        done_file, output_file,
        output_file,
        output_file, output_file, output_file,
        output_file, output_file, output_file,
        done_file,
        done_file,
        output_file,
    );
    let b64_tail = base64::engine::general_purpose::STANDARD.encode(tail_script.as_bytes());
    ssh_tail_args.push_str(&format!(" \"echo {} | base64 -d | bash\"", b64_tail));

    let mut tail_cmd = AsyncCommand::new(&shell);
    tail_cmd.arg("-l").arg("-c").arg(&ssh_tail_args);
    tail_cmd.stdout(std::process::Stdio::piped());
    tail_cmd.stderr(std::process::Stdio::piped());
    hide_window_async(&mut tail_cmd);

    let mut child = tail_cmd
        .spawn()
        .map_err(|e| format!("Failed to reconnect tail: {}", e))?;
    let stdout = child.stdout.take().ok_or("Failed to capture tail stdout")?;

    // 4. Store the new tail process as the session's child
    state
        .sessions
        .lock()
        .map_err(|e| e.to_string())?
        .insert(session_id.clone(), AgentSession { child });

    // 5. Stream output to frontend — the dedup logic in the frontend handles
    //    re-sent lines gracefully (same message ID = replace, not duplicate).
    let app_handle = app.clone();
    let sid = session_id.clone();
    let engine = load_session_from_disk(&session_id)
        .ok()
        .flatten()
        .map(|m| m.agent_engine)
        .unwrap_or_else(default_session_engine);
    tokio::spawn(async move {
        let adapter = crate::harness::pick(&engine);
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            let Some(canonical) = adapter.normalize_line(&line) else {
                continue;
            };
            let _ = app_handle.emit(
                &format!("agent-event-{}", sid),
                serde_json::json!({ "line": canonical }),
            );
        }
        let _ = app_handle.emit(&format!("agent-done-{}", sid), serde_json::json!({}));
    });

    Ok(())
}

/// Rename a session (update its human-readable name).
#[tauri::command]
pub async fn rename_session(session_id: String, name: String) -> Result<(), String> {
    if let Some(mut meta) = load_session_from_disk(&session_id).map_err(|e| e.to_string())? {
        meta.name = Some(name);
        save_session_to_disk(&meta)?;
        Ok(())
    } else {
        Err(format!("Session {} not found", session_id))
    }
}

/// Delete a session's metadata and optionally its output files.
#[tauri::command]
pub async fn delete_session(
    ssh_state: tauri::State<'_, super::ssh::SSHManager>,
    session_id: String,
    remote: Option<RemoteContext>,
    delete_output: Option<bool>,
) -> Result<(), String> {
    // Delete metadata file
    let dir = sessions_dir()?;
    let path = dir.join(format!("{}.json", session_id));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("Failed to delete session: {}", e))?;
    }

    // Optionally delete output files
    if delete_output.unwrap_or(false) {
        if let Some(meta) = load_session_from_disk(&session_id).ok().flatten() {
            let base_path = meta.remote_path.as_deref().unwrap_or(&meta.project_path);
            let output_file = format!("{}/.operon-{}.jsonl", base_path, session_id);
            let done_file = format!("{}/.operon-{}.done", base_path, session_id);

            if let Some(ctx) = remote {
                let profile = {
                    let profiles = ssh_state.profiles.lock().map_err(|e| e.to_string())?;
                    profiles.iter().find(|p| p.id == ctx.profile_id).cloned()
                };
                if let Some(profile) = profile {
                    let rm_cmd = format!(
                        "rm -f '{}' '{}'",
                        output_file.replace('\'', "'\\''"),
                        done_file.replace('\'', "'\\''"),
                    );
                    let _ = super::ssh::ssh_exec(&profile, &rm_cmd);
                }
            } else {
                let _ = std::fs::remove_file(&output_file);
                let _ = std::fs::remove_file(&done_file);
            }
        }
    }

    Ok(())
}
