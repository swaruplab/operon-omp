import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export async function startAgentSession(params: {
  sessionId: string;
  prompt: string;
  projectPath: string;
  model?: string;
  maxTurns?: number;
  resumeSession?: string;
}): Promise<void> {
  return invoke('start_agent_session', params);
}

export async function stopAgentSession(sessionId: string): Promise<void> {
  return invoke('stop_agent_session', { sessionId });
}

export async function onAgentEvent(
  sessionId: string,
  callback: (line: string) => void,
): Promise<UnlistenFn> {
  return listen<{ line: string }>(`agent-event-${sessionId}`, (event) => {
    callback(event.payload.line);
  });
}

export async function onAgentDone(
  sessionId: string,
  callback: () => void,
): Promise<UnlistenFn> {
  return listen(`agent-done-${sessionId}`, callback);
}
