import { invoke } from '@tauri-apps/api/core';
import type { MCPServerConfig } from '../types/mcp';

export interface AppSettings {
  theme: string;
  font_size: number;
  font_family: string;
  tab_size: number;
  word_wrap: boolean;
  minimap_enabled: boolean;
  model: string;
  max_turns: number;
  max_budget_usd: number;
  permission_mode: string; // 'full_auto' | 'safe_mode' | 'supervised'
  show_hidden_files: boolean;
  terminal_font_size: number;
  mcp_servers: MCPServerConfig[];
  extension_settings: Record<string, Record<string, unknown>>;
  last_project_path?: string | null;
  /** Use the xterm.js WebGL renderer addon. Turn off on setups where the
   *  WebGL atlas renders with hairline / ghost-stroke artifacts (seen on
   *  some Apple-silicon + Studio Display scaled modes). */
  terminal_use_webgl: boolean;
  /** Wrap new SSH terminals in a shared tmux session so jobs survive Operon
   *  quitting / laptop sleeping. No-op if the remote has no tmux installed. */
  ssh_auto_tmux: boolean;
  /** Name of the shared tmux session Operon attaches to. */
  ssh_tmux_session: string;
  /** First-run setup wizard completion flag. */
  setup_completed: boolean;
}

export const DEFAULT_SETTINGS: AppSettings = {
  theme: 'dark',
  font_size: 13,
  font_family: 'JetBrains Mono',
  tab_size: 2,
  word_wrap: false,
  minimap_enabled: true,
  model: 'ollama/kimi-k2.6:cloud',
  max_turns: 25,
  max_budget_usd: 5.0,
  permission_mode: 'full_auto',
  show_hidden_files: false,
  terminal_font_size: 13,
  mcp_servers: [],
  extension_settings: {},
  last_project_path: null,
  terminal_use_webgl: true,
  ssh_auto_tmux: true,
  ssh_tmux_session: 'operon-main',
  setup_completed: false,
};

export async function getSettings(): Promise<AppSettings> {
  return invoke('get_settings');
}

export async function updateSettings(settings: AppSettings): Promise<void> {
  return invoke('update_settings', { settings });
}
