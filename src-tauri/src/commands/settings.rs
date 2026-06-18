use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

use super::mcp::MCPServerConfig;

fn default_permission_mode() -> String {
    "full_auto".to_string()
}

fn default_model() -> String {
    // Local Ollama model id (provider/model form expected by OpenCode).
    "ollama/kimi-k2.6:cloud".to_string()
}

fn default_terminal_use_webgl() -> bool {
    // Default ON: WebGL is faster. Users on specific GPU/display combos (e.g.
    // Mac mini + Apple Studio Display scaled modes) where the xterm.js WebGL
    // atlas renders with subpixel artifacts can switch this off to force the
    // canvas renderer.
    true
}

fn default_ssh_auto_tmux() -> bool {
    // Default ON: wrap SSH sessions in `tmux new-session -A` so long-running
    // jobs survive the user logging out of Operon. User can switch this off
    // if they prefer plain bare-ssh (or if the remote has no tmux installed).
    true
}

fn default_ssh_tmux_session() -> String {
    "operon-main".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub theme: String,
    pub font_size: u32,
    pub font_family: String,
    pub tab_size: u32,
    pub word_wrap: bool,
    pub minimap_enabled: bool,
    #[serde(default = "default_model")]
    pub model: String,
    pub max_turns: u32,
    pub max_budget_usd: f64,
    /// Permission level: "full_auto", "safe_mode", or "supervised"
    #[serde(default = "default_permission_mode")]
    pub permission_mode: String,
    pub show_hidden_files: bool,
    pub terminal_font_size: u32,
    #[serde(default)]
    pub mcp_servers: Vec<MCPServerConfig>,
    #[serde(default)]
    pub extension_settings: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub last_project_path: Option<String>,
    /// When true, xterm.js terminals use the WebGL renderer addon. Some GPU
    /// and external-display combinations render the WebGL glyph atlas with
    /// hairline / ghost-stroke artifacts — in that case users can switch to
    /// the canvas renderer here (slower, always correct).
    #[serde(default = "default_terminal_use_webgl")]
    pub terminal_use_webgl: bool,
    /// When true, new SSH terminals automatically wrap the remote shell in a
    /// shared tmux session (`new-session -A -s ssh_tmux_session`). Persistent
    /// tmux means jobs you launch keep running after Operon (or your laptop)
    /// goes to sleep. Auto-wrap is a no-op if the remote has no tmux.
    #[serde(default = "default_ssh_auto_tmux")]
    pub ssh_auto_tmux: bool,
    /// Name of the shared tmux session Operon attaches to on the remote.
    #[serde(default = "default_ssh_tmux_session")]
    pub ssh_tmux_session: String,
    /// First-run setup wizard completion flag. When false, App.tsx routes to
    /// the SetupWizard instead of the main shell.
    #[serde(default)]
    pub setup_completed: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            font_size: 13,
            font_family: "JetBrains Mono".to_string(),
            tab_size: 2,
            word_wrap: false,
            minimap_enabled: true,
            model: default_model(),
            max_turns: 25,
            max_budget_usd: 5.0,
            permission_mode: "full_auto".to_string(),
            show_hidden_files: false,
            terminal_font_size: 13,
            mcp_servers: Vec::new(),
            extension_settings: HashMap::new(),
            last_project_path: None,
            terminal_use_webgl: true,
            ssh_auto_tmux: true,
            ssh_tmux_session: default_ssh_tmux_session(),
            setup_completed: false,
        }
    }
}

pub struct SettingsManager {
    pub settings: Mutex<AppSettings>,
}

impl SettingsManager {
    pub fn new() -> Self {
        // Try to load from disk, fall back to defaults
        let settings = Self::load_from_disk().unwrap_or_default();
        Self {
            settings: Mutex::new(settings),
        }
    }

    pub(crate) fn config_path() -> Option<std::path::PathBuf> {
        Some(crate::platform::config_dir().join("settings.json"))
    }

    fn load_from_disk() -> Option<AppSettings> {
        let path = Self::config_path()?;
        let data = std::fs::read_to_string(path).ok()?;
        let mut settings: AppSettings = serde_json::from_str(&data).ok()?;
        // Migration: stale Anthropic model ids saved before Operon Enterprise
        // went OpenCode-only. Reset to the OpenCode default — otherwise the
        // dropdown shows a Claude model that the OpenCode CLI can't run.
        if settings.model.starts_with("claude-") {
            eprintln!(
                "[operon] migrating stale model '{}' -> '{}'",
                settings.model,
                default_model()
            );
            settings.model = default_model();
            let _ = Self::save_to_disk(&settings);
        }
        Some(settings)
    }

    pub fn save_to_disk(settings: &AppSettings) -> Result<(), String> {
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let data = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
            std::fs::write(path, data).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

#[tauri::command]
pub async fn get_settings(state: tauri::State<'_, SettingsManager>) -> Result<AppSettings, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    Ok(settings.clone())
}

#[tauri::command]
pub async fn update_settings(
    state: tauri::State<'_, SettingsManager>,
    settings: AppSettings,
) -> Result<(), String> {
    SettingsManager::save_to_disk(&settings)?;
    let mut current = state.settings.lock().map_err(|e| e.to_string())?;
    *current = settings;
    Ok(())
}

/// Start platform-native speech recognition.
/// On macOS: uses SFSpeechRecognizer + AVAudioEngine via a Swift subprocess.
/// On other platforms: returns an error (dictation not supported).
#[tauri::command]
pub async fn start_dictation(app_handle: tauri::AppHandle) -> Result<(), String> {
    if !crate::platform::supports_dictation() {
        return Err("Dictation is not supported on this platform".to_string());
    }
    crate::platform::start_dictation_platform(&app_handle)
}

pub(crate) struct DictationProcess {
    pub(crate) stdin: std::process::ChildStdin,
    #[allow(dead_code)]
    pub(crate) pid: u32,
}

pub(crate) static DICTATION_PROCESS: std::sync::Mutex<Option<DictationProcess>> =
    std::sync::Mutex::new(None);


/// Stop the currently running dictation process.
#[tauri::command]
pub async fn stop_dictation() -> Result<(), String> {
    use std::io::Write;
    let mut guard = DICTATION_PROCESS.lock().map_err(|e| e.to_string())?;
    if let Some(ref mut process) = *guard {
        let _ = process.stdin.write_all(b"STOP\n");
        let _ = process.stdin.flush();
    }
    *guard = None;
    Ok(())
}
