//! First-run setup wizard backend.
//!
//! Provides dependency checks and installers for the three tools the agent
//! relies on:
//!
//! * **OpenCode** — required. The agent CLI itself.
//! * **Ollama**   — recommended. Local LLM runtime that the default
//!                  `ollama/kimi-k2.6:cloud` model points to.
//! * **vLLM**     — advisory. GPU-only Python package, normally installed on
//!                  a remote server, not the user's laptop. We surface a
//!                  dependency check (is `vllm` importable?) but don't try
//!                  to install it from the wizard.

use serde::Serialize;

use crate::platform::shell_exec;

#[derive(Debug, Clone, Serialize)]
pub struct ToolStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

fn check_command(probe: &str) -> ToolStatus {
    let output = shell_exec(probe).output();
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            let combined = if !stdout.trim().is_empty() {
                stdout
            } else {
                stderr
            };
            ToolStatus {
                installed: true,
                version: Some(first_line(&combined)),
                path: None,
            }
        }
        _ => ToolStatus {
            installed: false,
            version: None,
            path: None,
        },
    }
}

fn which(tool: &str) -> Option<String> {
    #[cfg(target_os = "windows")]
    let probe = format!("where.exe {}", tool);
    #[cfg(not(target_os = "windows"))]
    let probe = format!("command -v {}", tool);

    let out = shell_exec(&probe).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).to_string();
    let line = first_line(&s);
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

#[tauri::command]
pub async fn check_opencode() -> Result<ToolStatus, String> {
    let mut status = check_command("opencode --version");
    status.path = which("opencode");
    Ok(status)
}

#[tauri::command]
pub async fn check_ollama() -> Result<ToolStatus, String> {
    let mut status = check_command("ollama --version");
    status.path = which("ollama");
    Ok(status)
}

/// vLLM is a Python package. We check by asking the platform's Python whether
/// it can `import vllm`. The check is best-effort and never errors out.
#[tauri::command]
pub async fn check_vllm() -> Result<ToolStatus, String> {
    let python = crate::platform::python_command();
    let probe = format!(
        "{} -c 'import vllm; print(vllm.__version__)' 2>/dev/null",
        python
    );
    let out = shell_exec(&probe).output();
    let installed = out
        .as_ref()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let version = out
        .ok()
        .filter(|o| o.status.success())
        .map(|o| first_line(&String::from_utf8_lossy(&o.stdout)));
    Ok(ToolStatus {
        installed,
        version,
        path: which(python),
    })
}

/// Install OpenCode using the official installer script.
/// macOS / Linux: `curl -fsSL https://opencode.ai/install | bash`
/// Windows: surfaces a manual instruction since opencode.ai/install is bash-only.
#[tauri::command]
pub async fn install_opencode() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        return Err(
            "Automatic OpenCode install is not yet supported on Windows. \
             Install Node.js, then run: npm install -g @opencode/cli"
                .to_string(),
        );
    }

    #[cfg(not(target_os = "windows"))]
    {
        let cmd = "curl -fsSL https://opencode.ai/install | bash";
        let out = shell_exec(cmd)
            .output()
            .map_err(|e| format!("Failed to invoke installer: {}", e))?;
        if !out.status.success() {
            return Err(format!(
                "OpenCode installer exited with status {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

/// Install Ollama.
/// macOS: prefers Homebrew if present, falls back to the official installer.
/// Linux:  `curl -fsSL https://ollama.com/install.sh | sh`
/// Windows: surfaces a manual instruction.
#[tauri::command]
pub async fn install_ollama() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        return Err(
            "Automatic Ollama install is not yet supported on Windows. \
             Download the installer from https://ollama.com/download/windows"
                .to_string(),
        );
    }

    #[cfg(target_os = "macos")]
    {
        // Try brew first (cleaner uninstall path), then fall back to the
        // official curl installer.
        if let Some(brew) = crate::platform::find_package_manager() {
            let cmd = format!("{} install ollama", brew);
            if let Ok(o) = shell_exec(&cmd).output() {
                if o.status.success() {
                    return Ok(String::from_utf8_lossy(&o.stdout).to_string());
                }
            }
        }
        let cmd = "curl -fsSL https://ollama.com/install.sh | sh";
        let out = shell_exec(cmd)
            .output()
            .map_err(|e| format!("Failed to invoke installer: {}", e))?;
        if !out.status.success() {
            return Err(format!(
                "Ollama installer exited with status {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    #[cfg(target_os = "linux")]
    {
        let cmd = "curl -fsSL https://ollama.com/install.sh | sh";
        let out = shell_exec(cmd)
            .output()
            .map_err(|e| format!("Failed to invoke installer: {}", e))?;
        if !out.status.success() {
            return Err(format!(
                "Ollama installer exited with status {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

/// Mark the first-run setup wizard as complete and persist to disk.
#[tauri::command]
pub async fn complete_setup(
    state: tauri::State<'_, super::settings::SettingsManager>,
) -> Result<(), String> {
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings.setup_completed = true;
    super::settings::SettingsManager::save_to_disk(&settings)?;
    Ok(())
}
