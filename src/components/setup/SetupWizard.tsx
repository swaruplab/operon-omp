import { useEffect, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  CheckCircle,
  Circle,
  Loader2,
  AlertTriangle,
  ExternalLink,
  Sparkles,
} from 'lucide-react';

interface ToolStatus {
  installed: boolean;
  version: string | null;
  path: string | null;
}

interface ToolCheck {
  id: 'omp' | 'opencode' | 'ollama' | 'vllm';
  label: string;
  required: boolean;
  description: string;
  installable: boolean; // wizard offers a one-click install
  manualHint?: string;  // shown when installable=false
  docsUrl?: string;
  status: ToolStatus | null;
}

const INITIAL_TOOLS: ToolCheck[] = [
  {
    id: 'omp',
    label: 'OMP (oh-my-pi)',
    required: true,
    description:
      'The default agent engine. Operon shells out to `omp --mode json -p ...` to execute every chat turn. Installs as a self-contained ~150MB binary to ~/.local/bin — no root, no Bun/Node needed at runtime.',
    installable: true,
    manualHint: 'Install manually: `curl -fsSL https://omp.sh/install | sh`',
    docsUrl: 'https://github.com/can1357/oh-my-pi',
    status: null,
  },
  {
    id: 'opencode',
    label: 'OpenCode CLI (optional, rollback engine)',
    required: false,
    description:
      'The legacy agent runtime, kept for rollback. Set `agent_engine` to "opencode" in settings to shell out to `opencode run` instead of OMP.',
    installable: true,
    docsUrl: 'https://opencode.ai',
    status: null,
  },
  {
    id: 'ollama',
    label: 'Ollama',
    required: false,
    description:
      "Local LLM runtime. Operon's default model `ollama/kimi-k2.6:cloud` is served by Ollama.",
    installable: true,
    docsUrl: 'https://ollama.com',
    status: null,
  },
  {
    id: 'vllm',
    label: 'vLLM (optional, GPU servers)',
    required: false,
    description:
      'High-throughput Python server for self-hosted models on a GPU. Typically installed on a remote HPC node, not on your laptop.',
    installable: false,
    manualHint: 'On a GPU host: `pip install vllm` then `vllm serve <model>`.',
    docsUrl: 'https://docs.vllm.ai',
    status: null,
  },
];

export function SetupWizard({ onComplete }: { onComplete: () => void }) {
  const [tools, setTools] = useState<ToolCheck[]>(INITIAL_TOOLS);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState<string | null>(null);
  const [installError, setInstallError] = useState<string | null>(null);
  const [completing, setCompleting] = useState(false);

  const refresh = useCallback(async () => {
    setChecking(true);
    try {
      const [omp, opencode, ollama, vllm] = await Promise.all([
        invoke<ToolStatus>('check_omp'),
        invoke<ToolStatus>('check_opencode'),
        invoke<ToolStatus>('check_ollama'),
        invoke<ToolStatus>('check_vllm'),
      ]);
      setTools((prev) =>
        prev.map((t) => {
          if (t.id === 'omp') return { ...t, status: omp };
          if (t.id === 'opencode') return { ...t, status: opencode };
          if (t.id === 'ollama') return { ...t, status: ollama };
          if (t.id === 'vllm') return { ...t, status: vllm };
          return t;
        }),
      );
    } finally {
      setChecking(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const installOne = async (id: 'omp' | 'opencode' | 'ollama') => {
    setInstalling(id);
    setInstallError(null);
    try {
      const cmd =
        id === 'omp'
          ? 'install_omp'
          : id === 'opencode'
            ? 'install_opencode'
            : 'install_ollama';
      await invoke<string>(cmd);
      await refresh();
    } catch (e) {
      setInstallError(`${id}: ${String(e)}`);
    } finally {
      setInstalling(null);
    }
  };

  const finish = async () => {
    setCompleting(true);
    try {
      await invoke('complete_setup');
      onComplete();
    } catch (e) {
      setInstallError(`Failed to mark setup complete: ${String(e)}`);
      setCompleting(false);
    }
  };

  const ompInstalled = tools.find((t) => t.id === 'omp')?.status?.installed ?? false;
  // OMP is the only hard requirement. OpenCode is the optional rollback engine;
  // Ollama is recommended; vLLM is informational.
  const canFinish = ompInstalled;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-zinc-950 p-6">
      <div className="w-full max-w-2xl rounded-lg border border-zinc-800 bg-zinc-900 shadow-2xl">
        <div className="border-b border-zinc-800 px-6 py-5">
          <div className="flex items-center gap-3">
            <Sparkles className="h-6 w-6 text-blue-400" />
            <div>
              <h1 className="text-lg font-medium text-zinc-50">Welcome to Operon</h1>
              <p className="mt-0.5 text-xs text-zinc-500">
                One-time setup. We'll check the tools the agent needs to run.
              </p>
            </div>
          </div>
        </div>

        <div className="px-6 py-5 space-y-4">
          {tools.map((tool) => {
            const installed = tool.status?.installed ?? false;
            const isInstalling = installing === tool.id;
            return (
              <div
                key={tool.id}
                className="rounded border border-zinc-800 bg-zinc-950 p-4"
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="flex items-start gap-3 flex-1 min-w-0">
                    {checking && tool.status === null ? (
                      <Loader2 className="h-5 w-5 text-zinc-500 animate-spin shrink-0 mt-0.5" />
                    ) : installed ? (
                      <CheckCircle className="h-5 w-5 text-green-500 shrink-0 mt-0.5" />
                    ) : tool.required ? (
                      <AlertTriangle className="h-5 w-5 text-yellow-500 shrink-0 mt-0.5" />
                    ) : (
                      <Circle className="h-5 w-5 text-zinc-600 shrink-0 mt-0.5" />
                    )}
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-zinc-100">
                          {tool.label}
                        </span>
                        {tool.required && (
                          <span className="text-[10px] uppercase tracking-wide text-yellow-500/80">
                            Required
                          </span>
                        )}
                      </div>
                      <p className="mt-1 text-xs text-zinc-400 leading-relaxed">
                        {tool.description}
                      </p>
                      {installed && tool.status?.version && (
                        <p className="mt-1 text-[11px] text-green-500/80 font-mono">
                          {tool.status.version}
                        </p>
                      )}
                      {!installed && !tool.installable && tool.manualHint && (
                        <p className="mt-1.5 text-[11px] text-zinc-500 font-mono">
                          {tool.manualHint}
                        </p>
                      )}
                    </div>
                  </div>
                  <div className="flex items-center gap-2 shrink-0">
                    {tool.docsUrl && (
                      <button
                        onClick={() => invoke('open_url', { url: tool.docsUrl })}
                        className="text-zinc-500 hover:text-zinc-200 p-1.5"
                        title="Open docs"
                      >
                        <ExternalLink className="h-3.5 w-3.5" />
                      </button>
                    )}
                    {!installed && tool.installable && (
                      <button
                        onClick={() => installOne(tool.id as 'omp' | 'opencode' | 'ollama')}
                        disabled={isInstalling || installing !== null}
                        className="rounded bg-blue-600 hover:bg-blue-500 disabled:opacity-50 px-3 py-1.5 text-xs font-medium text-white transition-colors flex items-center gap-1.5"
                      >
                        {isInstalling && <Loader2 className="h-3 w-3 animate-spin" />}
                        {isInstalling ? 'Installing…' : 'Install'}
                      </button>
                    )}
                  </div>
                </div>
              </div>
            );
          })}

          {installError && (
            <div className="rounded border border-red-900/50 bg-red-950/30 px-3 py-2 text-xs text-red-300">
              {installError}
            </div>
          )}
        </div>

        <div className="border-t border-zinc-800 px-6 py-4 flex items-center justify-between">
          <button
            onClick={refresh}
            disabled={checking || installing !== null}
            className="text-xs text-zinc-400 hover:text-zinc-100 disabled:opacity-50"
          >
            {checking ? 'Checking…' : 'Re-check'}
          </button>
          <div className="flex items-center gap-3">
            <button
              onClick={finish}
              disabled={completing}
              className="text-xs text-zinc-500 hover:text-zinc-300 disabled:opacity-50"
            >
              Skip for now
            </button>
            <button
              onClick={finish}
              disabled={!canFinish || completing}
              className="rounded bg-green-600 hover:bg-green-500 disabled:bg-zinc-700 disabled:text-zinc-500 px-4 py-1.5 text-xs font-medium text-white transition-colors flex items-center gap-1.5"
            >
              {completing && <Loader2 className="h-3 w-3 animate-spin" />}
              {canFinish ? 'Finish setup' : 'Install OMP to continue'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
