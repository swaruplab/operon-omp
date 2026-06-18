import { useState } from 'react';
import {
  X,
  HelpCircle,
  ChevronRight,
  ChevronDown,
  Bot,
  Terminal,
  Code2,
  Server,
  Keyboard,
  BookOpen,
  Zap,
  PlayCircle,
  Search,
  Sparkles,
  GitBranch,
  Mic,
  BookMarked,
  Settings2,
  Plug,
  Puzzle,
  FileText,
  FolderOpen,
  Paperclip,
  MousePointerClick,
  Cloud,
} from 'lucide-react';
import { adaptShortcut } from '../../lib/platform';

interface HelpPanelProps {
  isOpen: boolean;
  onClose: () => void;
  onNavigate?: (view: string) => void;
}

interface HelpSection {
  id: string;
  title: string;
  icon: React.ElementType;
  iconColor: string;
  items: HelpItem[];
}

interface HelpItem {
  title: string;
  content: string;
  tip?: string;
  action?: { label: string; view: string };
  shortcut?: string;
}

const sections: HelpSection[] = [
  {
    id: 'getting-started',
    title: 'Getting Started',
    icon: Sparkles,
    iconColor: 'text-blue-400',
    items: [
      {
        title: 'Installing OMP (the agent engine)',
        content: 'Operon\'s default agent engine is OMP (oh-my-pi). The first-time setup wizard installs it for you, but you can also install it manually:\n\n    curl -fsSL https://omp.sh/install | sh\n\nThis drops a self-contained ~150MB binary into ~/.local/bin — no root, and no Bun/Node needed at runtime. It works on macOS (arm64/x64), Linux (x64/arm64), and Windows (x64). Operon runs it one-shot as `omp --mode json -p ...`, streaming JSONL events for each chat turn.\n\nOMP is OpenAI-compatible and multi-provider, so it works with the same backends described under AI Providers (Ollama, vLLM, LM Studio, OpenRouter, and any OpenAI-compatible endpoint).\n\nOpenCode is still bundled as an optional rollback engine — set `agent_engine` to "opencode" in settings to switch back.',
        tip: 'OMP needs no Node or Bun at runtime — the install is a single static binary, which makes it easy to drop onto an HPC login node without admin rights.',
        action: { label: 'Open Settings', view: 'settings' },
      },
      {
        title: 'Opening a project',
        content: 'Click "Open Folder" in the file explorer sidebar, or drag a folder onto the app window. Operon will set this as your working directory — the agent will be able to read, edit, and create files within it.',
        action: { label: 'Open Explorer', view: 'files' },
      },
      {
        title: 'Your first prompt',
        content: 'Type a message in the chat panel on the right. Try something like "What does this project do?" or "Help me fix the bug in main.py". the agent will read your files, understand the context, and respond.',
        tip: 'Start with Ask mode to explore your codebase before making changes.',
      },
      {
        title: 'Understanding the layout',
        content: 'Operon has four main areas: the Activity Bar (far left icons), the Sidebar (file explorer, SSH, etc.), the Editor (center, for code), and the Chat Panel (right side). The Terminal lives in a bottom panel you can toggle. All panels are resizable by dragging their borders.',
      },
      {
        title: 'Quick start suggestions',
        content: 'When you start a new conversation, the empty chat shows clickable suggestion cards like "Analyze data", "Write a pipeline", "Search PubMed", and "Debug an error". Click any suggestion to pre-fill the chat input with a relevant prompt — a fast way to get going without typing from scratch.',
      },
      {
        title: 'Relaunch setup wizard',
        content: 'If you need to reconfigure authentication or review the onboarding tour, you can relaunch the setup wizard from settings.',
        action: { label: 'Open Settings', view: 'settings' },
      },
    ],
  },
  {
    id: 'ai-modes',
    title: 'AI Modes',
    icon: Bot,
    iconColor: 'text-purple-400',
    items: [
      {
        title: 'Agent Mode',
        content: 'The default and most powerful mode. the agent can read files, write code, run terminal commands, and make multi-step changes to your project autonomously. Use this for implementing features, fixing bugs, refactoring, running pipelines, and any task where you want the agent to act.',
        tip: 'the agent shows each file it reads and edits in real-time. You can stop it at any time with the stop button.',
      },
      {
        title: 'Plan Mode',
        content: 'the agent creates a detailed implementation plan (saved as implementation_plan.md) before writing any code. You can review the plan in the editor, give feedback to refine it, and approve it when ready. the agent then executes the plan step by step.',
        tip: 'Use quick feedback buttons or type your own feedback in the chat to iterate on the plan before approving.',
      },
      {
        title: 'Ask Mode',
        content: 'the agent answers questions and explains code without making any changes to your project. Use this to understand how code works, get explanations of error messages, learn about libraries, or discuss architecture decisions.',
        tip: 'Great for onboarding onto a new codebase — ask "Walk me through how the authentication flow works".',
      },
      {
        title: 'Report Mode',
        content: 'Generates a structured scientific report (PDF and Markdown) from your project files. Operon scans your project directory, lets you select up to 40 files, asks a few context questions (biological question, key findings, audience), then sends everything to the agent in a single turn to produce a polished report with figures, tables, and references.',
        tip: 'Works with both local and remote/HPC projects. Avoid selecting files over 1 MB — stick to CSVs, scripts, logs, and small PDFs for best results.',
      },
      {
        title: 'Switching modes',
        content: 'Click the mode selector above the chat input to switch between Agent, Plan, Ask, and Report modes. You can switch modes mid-conversation. The mode affects what the agent is allowed to do — it doesn\'t lose context when you switch.',
        shortcut: 'Click the mode selector above the chat input',
      },
    ],
  },
  {
    id: 'ai-providers',
    title: 'AI Providers',
    icon: Cloud,
    iconColor: 'text-fuchsia-400',
    items: [
      {
        title: 'Overview — picking a backend',
        content: 'Operon\'s default agent engine is OMP (oh-my-pi); OpenCode remains available as an optional rollback engine. Both speak the OpenAI Chat Completions format (POST /v1/chat/completions), so any endpoint that exposes that API will work. Backends fall into two groups:\n\n1. Local runtimes — Ollama (default), LM Studio, vLLM. Run on your own machine or a GPU server. Best for privacy, cost control, and offline work.\n\n2. Cloud gateways — OpenRouter, LiteLLM, Groq, Together, DeepInfra. One key, hundreds of hosted models. Best for fast experimentation when outbound internet is allowed.\n\nThe out-of-the-box default is Ollama with `ollama/kimi-k2.6:cloud`. Switch in Settings → AI provider.',
        tip: 'Agentic tool-use quality drops sharply below ~30B-parameter models. For serious coding work, prefer 70B+ local models or cloud-hosted ones like glm-5.1:cloud, deepseek-v3.1:cloud, or GPT-4-class models via OpenRouter.',
        action: { label: 'Open Settings', view: 'settings' },
      },
      {
        title: 'Ollama — setup step by step',
        content: '1. Install the Ollama desktop app from ollama.com/download (macOS, Windows, Linux). Launch it once; the daemon auto-starts on login and listens on http://localhost:11434.\n\n2. For cloud-hosted models (the :cloud suffix), sign in:\n    ollama signin\n  A browser tab opens for ollama.com authentication. One-time setup; credentials are stored on your machine.\n\n3. Pull a model. Either local (runs on your GPU/CPU) or cloud:\n    ollama pull llama3.1:70b             # local\n    ollama pull glm-5.1:cloud             # cloud, proxied by local daemon\n    ollama pull deepseek-v3.1:cloud\n    ollama list                            # confirm it\'s registered\n\n4. Verify the OpenAI-compatible endpoint works:\n    curl -sS http://localhost:11434/v1/models\n  You should see your pulled models in the data array.\n\n5. In Operon: Settings → AI provider → OpenAI-compatible → click the "Ollama" preset → click "Detect models" → select your model → "Test connection".\n\n6. Start a chat. The model you picked is used; :cloud models route via your local daemon up to ollama.com.',
        tip: 'Ollama Cloud is a hybrid: your local daemon is always the endpoint Operon talks to, even for :cloud models. The daemon is what forwards cloud requests upstream using your signed-in credential — so the desktop app must always be running, even for cloud-only usage.',
      },
      {
        title: 'Ollama — common errors',
        content: '• "error sending request for url (http://localhost:11434/...)" — Ollama daemon isn\'t running. Launch the desktop app or run "ollama serve". Also check: curl http://localhost:11434/api/version.\n\n• "bind: address already in use" when running ollama serve — the daemon is already running (desktop app). Don\'t start a second one; just verify with lsof -iTCP:11434 -sTCP:LISTEN.\n\n• "data": null in /v1/models response — no models pulled yet. Run ollama pull <model>.\n\n• 401 Unauthorized on :cloud models — not signed in. Run ollama signin.\n\n• Stale HTTPS_PROXY env blocking localhost — if you ever exported HTTPS_PROXY in the terminal that launched Operon (e.g. for mitmproxy), the running process still has it. Fully quit Operon (Cmd-Q + pkill -f operon) and relaunch from Finder.\n\n• Model responses are short / low-quality — models under ~30B parameters struggle with agentic tool use. Try a 70B local model or a :cloud model.',
      },
      {
        title: 'LM Studio — setup step by step',
        content: '1. Install LM Studio from lmstudio.ai. Open the app.\n\n2. Download a model from the in-app search (the Discover tab). Pick something coding-focused: Qwen2.5-Coder-32B-Instruct, DeepSeek-Coder-V2, or Llama-3.1-70B.\n\n3. Load the model (click it, then "Load"). In the Developer / Local Server tab, click "Start Server". The default endpoint is http://localhost:1234/v1.\n\n4. In Operon: Settings → AI provider → OpenAI-compatible → click "LM Studio" preset (sets Base URL to http://localhost:1234/v1) → click "Detect models" → pick your loaded model → "Test connection".',
        tip: 'LM Studio shows live GPU/CPU utilization and token/sec in the Local Server panel — useful for tuning context length and batch size.',
      },
      {
        title: 'vLLM — setup step by step',
        content: 'vLLM is a production-grade inference server for NVIDIA GPUs. Best suited for lab servers or HPC login nodes, not laptops.\n\n1. Install in a Python env with CUDA:\n    pip install vllm\n\n2. Start the OpenAI-compatible server (adjust model and GPU count):\n    vllm serve Qwen/Qwen2.5-Coder-32B-Instruct \\\n      --host 0.0.0.0 --port 8000 \\\n      --tensor-parallel-size 2\n\n3. If vLLM is on a remote host, tunnel it to your laptop (recommended — keeps the port off the public network):\n    ssh -L 8000:localhost:8000 user@gpu-server\n\n4. In Operon: Settings → AI provider → OpenAI-compatible → click "vLLM" preset → adjust Base URL (http://localhost:8000/v1 if tunneled) → Detect models → Test connection.',
        tip: 'vLLM supports tool calls via --enable-auto-tool-choice and a chat template. Without these flags the agent can still chat but can\'t use tools — so pass both when launching vLLM for agent mode.',
      },
      {
        title: 'LiteLLM — setup step by step',
        content: 'LiteLLM is a thin Python proxy that exposes a unified OpenAI-compatible interface in front of ~100 provider backends (OpenAI, Azure, Bedrock, Ollama, vLLM, Vertex AI, etc.). Useful when you want one config file, one set of keys, and one cost/usage dashboard for many backends.\n\n1. Install:\n    pip install "litellm[proxy]"\n\n2. Write a config at ~/litellm_config.yaml:\n    model_list:\n      - model_name: glm-5.1-cloud\n        litellm_params:\n          model: ollama/glm-5.1:cloud\n          api_base: http://localhost:11434\n      - model_name: gpt-4o\n        litellm_params:\n          model: openai/gpt-4o\n          api_key: os.environ/OPENAI_API_KEY\n\n3. Start the proxy:\n    litellm --config ~/litellm_config.yaml --port 4000\n\n4. In Operon: Settings → AI provider → OpenAI-compatible → click "LiteLLM" preset → Base URL: http://localhost:4000/v1 → Detect models → Test connection.',
        tip: 'LiteLLM exposes a dashboard at http://localhost:4000/ui with cost per model, request logs, and rate-limit settings — useful for keeping track of API spend across providers.',
      },
      {
        title: 'OpenRouter — setup step by step',
        content: 'OpenRouter is a hosted gateway that fronts models from Anthropic, OpenAI, Google, Mistral, Meta, and dozens of others — one API key, one bill, any model.\n\n1. Sign up at openrouter.ai and get an API key from Settings → Keys. Add credits ($5 minimum) or link a card.\n\n2. In Operon: Settings → AI provider → OpenAI-compatible → click "OpenRouter" preset (sets Base URL to https://openrouter.ai/api/v1) → paste your API key.\n\n3. Click "Detect models" — you\'ll see hundreds of models. Pick one:\n  • openai/gpt-4o — flagship OpenAI\n  • google/gemini-2.0-flash-exp — fast, cheap\n  • meta-llama/llama-3.1-405b-instruct — open-weights flagship\n\n4. Test connection. On first real chat, OpenRouter may prompt you to accept its terms for the chosen model family — check the OpenRouter dashboard if a request silently fails.',
        tip: 'OpenRouter routes requests to the cheapest/fastest provider for the model you pick. You can pin a specific provider by appending :nitro or :floor to the model name (see openrouter.ai/docs for routing modifiers).',
      },
      {
        title: 'Custom / self-hosted endpoints',
        content: 'Any endpoint that speaks the OpenAI Chat Completions API will work:\n\n• Groq (https://api.groq.com/openai/v1) — ultra-fast inference for Llama, Mixtral, Qwen.\n• Together (https://api.together.xyz/v1) — wide selection of open models.\n• DeepInfra (https://api.deepinfra.com/v1/openai) — cheap open-weights hosting.\n• Cerebras (https://api.cerebras.ai/v1) — huge models, fast.\n• Any company-internal gateway — if your IT team runs an OpenAI-compatible proxy.\n\nSetup: pick Custom preset (or blank) → paste Base URL → paste API key → Detect models → Test connection.',
      },
      {
        title: 'Switching between providers',
        content: 'You can switch providers at any time from Settings → AI provider. The currently-active session keeps using whatever provider it started with (so your chat history is consistent) — the switch takes effect on the next new conversation.\n\nYour settings for each backend (API keys, base URLs, model choice) are remembered separately, so you can bounce between OpenRouter for heavyweight work and a local Ollama for quick questions without reconfiguring every time.',
        tip: 'For remote (SSH/HPC) sessions that need to reach a model running on your laptop, open a reverse tunnel manually: ssh -R <port>:127.0.0.1:<port> user@host.',
      },
      {
        title: 'Troubleshooting checklist',
        content: 'When a custom endpoint isn\'t working, go through this list in order:\n\n1. Is the endpoint itself up?\n    curl -sS <base_url>/models          # should return JSON list\n    curl -sS <base_url>/chat/completions \\\n      -H "Content-Type: application/json" \\\n      -d \'{"model":"<id>","messages":[{"role":"user","content":"ping"}],"max_tokens":4}\'\n  If curl fails, fix the endpoint first — Operon can\'t help.\n\n2. Is a stale proxy env var interfering? Check:\n    env | grep -iE "proxy|no_proxy"\n    launchctl getenv HTTPS_PROXY  (macOS)\n    scutil --proxy                (macOS system proxy)\n  Unset anything suspicious and fully relaunch Operon.\n\n3. Is the model actually capable of tool use? Small local models (< 7B) often can\'t hold up in Agent mode. Drop to Ask mode to rule out model capability, or switch to a larger model.\n\n4. Tail the agent engine log. For OMP (the default) check ~/.omp/agent/ for config and recent run logs; for the OpenCode rollback engine see ~/.local/share/opencode/log/ on macOS/Linux. Most upstream API errors land there with the full request/response body.',
      },
    ],
  },
  {
    id: 'ai-providers-remote',
    title: 'AI Providers (Remote)',
    icon: Cloud,
    iconColor: 'text-cyan-400',
    items: [
      {
        title: 'Overview — three topologies',
        content: 'When the agent runs on a remote server (HPC/lab machine via SSH + tmux), there are three places the model can actually live. Pick the one that matches your situation, then follow the dedicated item below:\n\n• Option A — Model on the remote server. Cleanest, no tunnels. Best if the remote has spare GPU/CPU.\n\n• Option B — Model on your laptop, remote the agent calls back through an SSH reverse tunnel. Handy if you already have Ollama on your laptop and don\'t want to install anything on the remote.\n\n• Option C — Hosted gateway (OpenRouter, LiteLLM cloud, etc.). Zero local setup, requires outbound internet from the remote.\n\nFor serious coding work on an HPC GPU node, Option A with vLLM is usually the right answer. For a quick test with no install, Option C with OpenRouter is fastest.',
        tip: 'Remote auto-tunneling is not yet wired in this release. Option B requires you to open an SSH reverse tunnel manually and keep it open for the session.',
      },
      {
        title: 'Option A — Model on the remote server',
        content: 'Run the inference server on the remote itself; remote the agent talks to its own localhost.\n\nStep 1 — install runtime on the remote (Ollama example):\n    ssh user@server\n    curl -fsSL https://ollama.com/install.sh | sh\n    ollama serve >/tmp/ollama.log 2>&1 &\n    ollama signin                             # only if using :cloud models\n    ollama pull glm-5.1:cloud                 # or any local model\n    curl -sS http://localhost:11434/v1/models # verify\n\nOr vLLM for real GPU inference:\n    pip install vllm\n    vllm serve Qwen/Qwen2.5-Coder-32B-Instruct \\\n      --host 127.0.0.1 --port 8000 \\\n      --enable-auto-tool-choice --tool-call-parser hermes\n\nStep 2 — Operon setup:\n  1. SSH view → connect to the server.\n  2. Settings → AI provider → OpenAI-compatible.\n  3. Base URL: http://127.0.0.1:11434/v1 (or whichever port the remote uses). The URL is evaluated on the remote, so 127.0.0.1 means the remote itself.\n  4. Chat panel → switch mode to Remote → select server → pick model → send a message.',
        tip: 'The agent engine (OMP by default) talks to the OpenAI-compatible endpoint directly — no extra sidecar needed on the remote. Make sure the runtime (Ollama / vLLM / etc.) is reachable on 127.0.0.1 from the remote shell before starting a chat.',
      },
      {
        title: 'Option B — Model on laptop, remote calls back via tunnel',
        content: 'Keep Ollama running on your laptop; open an SSH reverse tunnel so the remote agent can reach it as if it were local.\n\nStep 1 — start Ollama on laptop. Verify with: curl -sS http://localhost:11434/v1/models.\n\nStep 2 — open the reverse tunnel in a separate terminal and leave it running:\n    ssh -R 11434:127.0.0.1:11434 user@server\n\nThis makes the remote\'s port 11434 forward through SSH to Ollama on your laptop\'s 11434.\n\nStep 3 — Operon setup:\n  1. Connect to the server in the SSH view (independent of the tunnel; Operon uses its own SSH).\n  2. Settings → AI provider → OpenAI-compatible.\n  3. Base URL: http://127.0.0.1:11434/v1 (the remote sees it locally thanks to -R).\n  4. Remote mode → start chatting.',
        tip: 'Auto-tunneling is on the roadmap. Until then you maintain the ssh -R yourself in a separate terminal — quitting that terminal kills the tunnel and the remote chat will fail.',
      },
      {
        title: 'Option B — gotchas',
        content: '• Latency stack-up: remote agent → reverse tunnel → laptop Ollama → ollama.com (for :cloud). Expect 300–800 ms to first token vs ~100 ms on Option A.\n\n• Laptop sleep kills the tunnel. Either prevent sleep (caffeinate -dims) or switch to Option A for long runs.\n\n• Security: as long as your remote\'s sshd_config keeps GatewayPorts off (default), the -R tunnel only listens on the remote\'s loopback, not its public interface. Your laptop Ollama is not exposed to the internet.\n\n• HPC compute nodes: tunnels land on the login node. If the agent actually runs on a compute node (via Operon\'s Terminal mode), you need a second hop from compute node → login node. Easiest workaround: put Ollama on the compute node itself (Option A).\n\n• Port clashes: if the remote already has something on port 11434 (someone else\'s Ollama), pick a free port for -R, e.g. ssh -R 19434:127.0.0.1:11434 user@server and set Base URL to http://127.0.0.1:19434/v1.',
      },
      {
        title: 'Option C — Hosted gateway (OpenRouter, LiteLLM cloud)',
        content: 'Skip local setup entirely. Remote the agent talks directly to a cloud endpoint over HTTPS.\n\nOpenRouter walkthrough:\n  1. Sign up at openrouter.ai, get an API key (Settings → Keys), add credits.\n  2. Verify outbound internet from the remote (HPC clusters sometimes block this):\n       ssh user@server \'curl -sS -o /dev/null -w "%{http_code}\\n" https://openrouter.ai/api/v1/models\'\n     200 means you are good. 000 / timeout means the cluster blocks egress — fall back to Option A or B.\n  3. In Operon: Remote session → Settings → AI provider → OpenAI-compatible → OpenRouter preset.\n  4. Paste your API key.\n  5. Detect models → pick one (openai/gpt-4o, meta-llama/llama-3.1-405b-instruct, etc.) → Test connection.\n\nLiteLLM self-hosted: run the LiteLLM proxy on a machine reachable by both your laptop and your remote (typically a lab server with public IP). Point Operon at that URL from both local and remote sessions — same URL, same config, works from anywhere.',
        tip: 'If your cluster has a outbound HTTP proxy requirement, set HTTPS_PROXY in your remote shell profile (.bashrc) and re-exec Operon\'s remote session. The proxy setting will propagate to the agent subprocess.',
      },
      {
        title: 'Which option should I pick?',
        content: 'If the remote is an HPC cluster with GPU nodes:\n  • Serious coding work → Option A with vLLM serving Qwen2.5-Coder-32B (or larger) on a GPU allocation. Best latency, best quality, no data leaves the cluster, works even when the cluster blocks outbound internet.\n  • Quick experimentation → Option C with OpenRouter (if outbound internet is allowed).\n  • Avoid Option B for long-running HPC sessions — reverse tunnels break on laptop sleep and sometimes when compute-node firewalls prune idle connections.\n\nIf the remote is a lab workstation you own:\n  • Option A with Ollama is usually the right answer. One-time install, zero ongoing tunnel hassle, full privacy.\n\nIf the remote is a shared cloud VM:\n  • Option C if outbound is allowed and you want simplicity.\n  • Option A if you care about cost predictability (single model, predictable VRAM).',
      },
      {
        title: 'Remote troubleshooting',
        content: 'Before blaming Operon, check the stack from the inside out. SSH into the server and run:\n\n1. Is the model endpoint reachable from the remote itself?\n    curl -sS http://127.0.0.1:11434/v1/models\n  (Or whatever Base URL you configured.) If this fails on the remote but works on your laptop, you are using Option B without a tunnel — see the Option B item.\n\n2. Does the remote actually see the tunneled port (Option B)?\n    ss -ltnp | grep 11434         # Linux\n    lsof -iTCP:11434 -sTCP:LISTEN # macOS\n  Expect a sshd process listening. If nothing, the -R tunnel is down.\n\n3. Does the remote have outbound internet (Option C / :cloud models)?\n    curl -sS -o /dev/null -w "%{http_code}\\n" https://openrouter.ai/api/v1/models\n    curl -sS -o /dev/null -w "%{http_code}\\n" https://ollama.com\n\n4. Is the agent engine itself installed on the remote? Operon\'s setup wizard only installs runtimes locally. On the remote, run:\n    which omp && omp --version       # default engine (OMP)\n  If missing: curl -fsSL https://omp.sh/install | sh (drops a self-contained binary into ~/.local/bin — no root). If you run the OpenCode rollback engine instead: which opencode && opencode --version, install with curl -fsSL https://opencode.ai/install | bash.\n  Note: the remote/HPC path has not yet been validated on a real cluster — confirm the Linux binary\'s glibc compatibility before relying on it for long jobs.\n\n5. If all of the above look right but chat still fails, look at .operon-<id>.jsonl in your remote project directory — that\'s the raw streamed JSONL output; error messages from the agent land there.',
      },
    ],
  },
  {
    id: 'report',
    title: 'Report Generation',
    icon: FileText,
    iconColor: 'text-pink-400',
    items: [
      {
        title: 'What is Report mode?',
        content: 'Report mode creates a publication-ready scientific report from your project files. It scans your project directory (local or remote), lets you pick the relevant files, asks context questions, and generates a formatted PDF and Markdown report — all in one workflow.',
        tip: 'Ideal for summarizing bioinformatics analyses, QC results, or pipeline outputs into a shareable document.',
      },
      {
        title: 'Step 1: Scan your project',
        content: 'Switch to Report mode using the mode selector. Operon scans your project directory and displays a file tree showing all available files grouped by folder, with file sizes and counts. For remote/HPC projects, the scan runs over SSH automatically.',
      },
      {
        title: 'Step 2: Select files',
        content: 'Pick the files you want included in the report — up to 40 files. Check folders to select all files inside them, or pick individual files. Files over 1 MB are flagged with an amber warning icon. Avoid selecting very large files as they slow down report generation.',
        tip: 'Select CSVs, log files, small PDFs, scripts, and QC reports. Operon pre-reads file contents so the agent doesn\'t need to fetch them during generation.',
      },
      {
        title: 'Step 3: Provide context',
        content: 'After selecting files, the agent asks a few clarifying questions: What biological question were you investigating? What are the key findings? Who is the audience? Answer as many as you can, then type "generate report" to proceed. The more context you give, the better the report.',
      },
      {
        title: 'Step 4: Report output',
        content: 'the agent generates a comprehensive report including an introduction, methods, results with figures and tables, discussion, and references. The output is saved as both a PDF and a Markdown file in your project directory with a timestamped filename (e.g., report_2026-03-29_1757.pdf).',
      },
      {
        title: 'Tips for best results',
        content: 'Keep your file selection focused — 10–30 files covering the key outputs is better than 40 random files. Include QC reports, summary CSVs, and pipeline logs. Avoid raw data files (FASTQs, BAMs) as they\'re too large and binary. Providing a clear biological question in the context step dramatically improves the report quality.',
      },
    ],
  },
  {
    id: 'pubmed',
    title: 'PubMed Literature',
    icon: BookMarked,
    iconColor: 'text-emerald-400',
    items: [
      {
        title: 'What is PubMed grounding?',
        content: 'When enabled in Ask mode, Operon automatically searches PubMed for peer-reviewed articles relevant to your question before the agent responds. the agent then grounds its answer in real scientific literature, citing specific papers with links you can follow.',
        tip: 'This is especially powerful for questions about genes, pathways, methods, or any topic covered in biomedical literature.',
      },
      {
        title: 'Enabling PubMed search',
        content: 'Switch to Ask mode using the mode selector above the chat input. You\'ll see a green "PubMed" toggle button appear. Click it to enable or disable literature search. When enabled, every question you ask will first search PubMed, then the agent answers using those papers as context.',
        tip: 'The PubMed toggle only appears in Ask mode — it\'s not available in Agent or Plan modes.',
      },
      {
        title: 'How it works',
        content: 'Operon extracts key scientific terms from your question, queries the NCBI PubMed E-utilities API, and retrieves up to 5 relevant articles with full abstracts. These are injected into the prompt so the agent can cite them by number [1], [2], etc. Each response includes a References section with PubMed links.',
      },
      {
        title: 'Reading the results',
        content: 'After a PubMed-grounded response, a green bar appears above the chat input showing the articles that were found. Click it to expand and see titles, authors, journals, and direct PubMed links. Click any PMID link to open the paper on PubMed.',
      },
      {
        title: 'Tips for better results',
        content: 'Use specific scientific terms for better PubMed matches. For example, "What is the role of TP53 in apoptosis?" will yield better results than "how does cell death work?". Gene names, pathway names, method names, and disease terms all work well as search queries.',
      },
    ],
  },
  {
    id: 'voice',
    title: 'Voice Dictation',
    icon: Mic,
    iconColor: 'text-red-400',
    items: [
      {
        title: 'Using voice input',
        content: 'Click the microphone icon next to the send button in the chat input area. Operon uses macOS native speech recognition (SFSpeechRecognizer) to convert your speech to text in real-time. Click the mic again to stop recording.',
        tip: 'The mic button pulses red while actively listening. Your words appear in the text input as you speak.',
      },
      {
        title: 'First-time setup',
        content: 'The first time you use voice dictation, macOS will prompt you to grant two permissions: Microphone access and Speech Recognition access. Both must be allowed for dictation to work. You can manage these in System Settings → Privacy & Security.',
      },
      {
        title: 'How it works',
        content: 'Operon launches a native macOS speech recognition process using Apple\'s SFSpeechRecognizer framework. Your audio is processed locally or via Apple\'s servers (depending on your macOS settings). Partial results stream into the text field as you speak, and the final transcription replaces them when you stop.',
      },
      {
        title: 'Tips for best results',
        content: 'Speak clearly at a natural pace. Technical terms and gene names may need correction after dictation — review the transcription before sending. You can edit the transcribed text just like any other text in the input field. Dictation works best in quiet environments.',
      },
    ],
  },
  {
    id: 'github',
    title: 'GitHub Integration',
    icon: GitBranch,
    iconColor: 'text-orange-400',
    items: [
      {
        title: 'Overview',
        content: 'Operon includes a full-featured GitHub integration for version control. Initialize repos, stage individual files, manage branches, stash changes, browse commit history, and publish — all from the Git panel in the sidebar.',
        action: { label: 'Open Git Panel', view: 'git' },
      },
      {
        title: 'Setting up GitHub',
        content: 'Open the Git panel from the sidebar (the branch icon). The first time, Operon guides you through a 3-step setup: 1) Install the GitHub CLI (gh) if not present, 2) Sign in to your GitHub account using device authentication, 3) Create a new repository or link an existing one.',
      },
      {
        title: 'Repository selection',
        content: 'In step 3, choose between creating a new repository (with name, description, and public/private toggle) or linking an existing one from your GitHub account. The existing repo picker shows all your repos with a search filter. You can change the linked repository at any time by clicking "Change repository..." below the connection status.',
        tip: 'Use "Change repository..." to switch between personal and organization repos without re-authenticating.',
      },
      {
        title: 'Branch management',
        content: 'Click the branch name at the top of the Git panel to open the branch picker. It shows both local and remote branches. Switch branches with one click, or create a new branch inline by typing a name and pressing Enter. Remote branches are fetched automatically via git fetch.',
      },
      {
        title: 'File-level staging',
        content: 'The Changes section groups files into "Staged" and "Changes" (unstaged). Hover over any file to reveal stage (+), unstage (−), and discard (↩) buttons. Use the header icons to stage all or unstage all at once. This gives you precise control over what goes into each commit.',
      },
      {
        title: 'Publishing and push targets',
        content: 'Write a commit message and click "Publish to GitHub" to commit and push in one step. Click the branch name in the "Push to" indicator to choose a different target branch — useful for pushing a local feature branch to a specific remote branch.',
      },
      {
        title: 'Version tagging',
        content: 'Enable "Tag version" to create a git tag with each publish. Choose from patch (0.1.0 → 0.1.1), minor (0.1.0 → 0.2.0), major (0.1.0 → 1.0.0), or enter a custom version string. Operon uses semantic versioning and pushes tags alongside your commits.',
      },
      {
        title: 'Stash manager',
        content: 'Expand the Stash section to temporarily shelve uncommitted changes. Type an optional message and click the archive icon to stash. Each stash entry shows its message and date — click the restore icon to apply it back, or the trash icon to discard it.',
        tip: 'Stash your work before switching branches to avoid losing uncommitted changes.',
      },
      {
        title: 'Commit history',
        content: 'Expand the History section to see the last 30 commits with author, date, and file count. Click any commit to see its full diff. Use the copy icon to grab a commit hash for reference in chat or terminal commands.',
      },
      {
        title: 'Amend last commit',
        content: 'Below the Publish button, click "Amend last commit" to edit the most recent commit message or fold staged changes into it. The current message is pre-filled for editing.',
      },
    ],
  },
  {
    id: 'chat-features',
    title: 'Chat Features',
    icon: Paperclip,
    iconColor: 'text-sky-400',
    items: [
      {
        title: 'File attachments',
        content: 'Click the paperclip icon in the chat input to attach files and images to your message. You can also drag and drop files directly onto the chat area. Supported types include images (PNG, JPG, GIF), text files, code files, CSV, PDF, JSON, and YAML.',
        tip: 'Attach a screenshot of an error to let the agent see exactly what you see, or attach a CSV so the agent can analyze its contents.',
      },
      {
        title: 'Clipboard image paste',
        content: 'Copy an image or take a screenshot, then paste directly into the chat input with Cmd+V. Operon automatically saves the clipboard image and attaches it to your message — no need to save a file first.',
        shortcut: 'Cmd+V to paste image',
      },
      {
        title: 'File references with @',
        content: 'Type @ in the chat input to search and reference specific files from your project. This focuses the agent on exactly the files you care about instead of searching the entire project. Multiple files can be referenced in a single message.',
        tip: 'Try "@main.py has a bug on line 45" to point the agent directly at the file.',
      },
      {
        title: 'Copy messages',
        content: 'Right-click on any message in the chat to open a context menu with a "Copy message" option. This copies the full message text to your clipboard — useful for saving the agent\'s explanations, code snippets, or analysis results.',
      },
    ],
  },
  {
    id: 'file-explorer',
    title: 'File Explorer',
    icon: FolderOpen,
    iconColor: 'text-yellow-400',
    items: [
      {
        title: 'Navigating folders',
        content: 'Single-click a folder to expand or collapse it in the tree view. Double-click a folder to navigate into it, making it the current root directory. This is useful for focusing on a specific subfolder in a large project.',
        action: { label: 'Open Explorer', view: 'files' },
      },
      {
        title: 'Go-to-folder path bar',
        content: 'Click the path bar at the top of the file explorer to type any path directly. Press Enter to navigate to that directory. This lets you jump to deep paths instantly without clicking through the tree.',
      },
      {
        title: 'Favorites & pinned items',
        content: 'Hover over any file or folder in the explorer and click the pin icon to add it to your Favorites section at the top. Pinned items persist across sessions and provide quick access to frequently used files. Click the pin icon again to unpin.',
      },
      {
        title: 'Creating files and folders',
        content: 'Use the toolbar icons at the top of the file explorer to create new files or folders. A text input appears inline — type a name and press Enter. The new item is created in the currently viewed directory.',
      },
      {
        title: 'cd to terminal',
        content: 'Click the terminal-arrow icon on any folder to automatically run a cd command in the integrated terminal, navigating to that directory. Saves you from typing long paths manually.',
      },
      {
        title: 'File sizes',
        content: 'Hover over any file in the explorer to see its size (B, KB, MB, GB) displayed on the right. This works in both the local and remote file explorers. Files larger than 15 MB show a warning instead of opening in the editor, to prevent the UI from hanging on very large data files.',
      },
      {
        title: 'Search across files',
        content: 'Click the magnifying glass icon in the Activity Bar to open the Search view. Type a query to search across all files in your project. Results show the file path and matching lines with context. Click any result to jump directly to that line in the editor.',
        action: { label: 'Open Search', view: 'search' },
      },
    ],
  },
  {
    id: 'editor',
    title: 'Code Editor',
    icon: Code2,
    iconColor: 'text-green-400',
    items: [
      {
        title: 'Opening files',
        content: 'Click any file in the sidebar explorer to open it in the editor. Files open as tabs — click a tab to switch, or middle-click to close. Double-click a file to pin it (single-click opens as a preview that gets replaced by the next file you open).',
      },
      {
        title: 'Editing and saving',
        content: 'Edit files directly in the Monaco editor. Changes are indicated by a blue dot on the tab. Save with Cmd+S. The editor supports syntax highlighting for 50+ languages, bracket matching, auto-indent, and multi-cursor editing.',
        shortcut: 'Cmd+S to save',
      },
      {
        title: 'Diff view',
        content: 'When the agent edits a file, a diff view shows what changed (green = added, red = removed). You can review changes before they\'re applied. Click "Accept" to keep changes or "Reject" to revert.',
      },
      {
        title: 'Previewing files',
        content: 'Image files (PNG, JPG, SVG, etc.) open in a visual viewer with zoom and rotation. PDFs render inline. HTML files show a live preview. These all open as tabs alongside your code files.',
      },
    ],
  },
  {
    id: 'terminal',
    title: 'Terminal',
    icon: Terminal,
    iconColor: 'text-amber-400',
    items: [
      {
        title: 'Using the terminal',
        content: 'The integrated terminal runs in the bottom panel. It\'s a full shell (zsh/bash) connected to your project directory. You can run any command — build tools, git, scripts, package managers, etc.',
      },
      {
        title: 'The agent and the terminal',
        content: 'In Agent mode, the agent can run terminal commands autonomously. You\'ll see commands and their output in the chat. the agent uses the terminal to install dependencies, run tests, execute scripts, and more.',
      },
      {
        title: 'Multiple terminals',
        content: 'When you connect to a remote server via SSH, a second terminal tab appears for the remote session. You can have both local and remote terminals active simultaneously.',
      },
    ],
  },
  {
    id: 'remote-ssh',
    title: 'Remote SSH & HPC',
    icon: Server,
    iconColor: 'text-teal-400',
    items: [
      {
        title: 'Adding a server',
        content: 'Go to the SSH view in the sidebar and click "Add Server". Enter your hostname, username, and either an SSH key path or password. Operon stores profiles locally and can generate SSH keys for you automatically.',
        action: { label: 'Open SSH View', view: 'ssh' },
      },
      {
        title: 'Connecting',
        content: 'Click "Connect" on a saved profile. This opens an SSH terminal in the bottom panel and switches the file explorer to show the remote filesystem. You can browse, open, and edit remote files.',
      },
      {
        title: 'Running the agent remotely',
        content: 'Select "Remote" next to the mode selector in the chat panel, then pick your connected server. the agent runs inside a tmux session on the remote machine, so sessions persist even if you disconnect or close the app.',
        tip: 'Perfect for long-running bioinformatics pipelines on HPC clusters — start a job and check back later.',
      },
      {
        title: 'SSH key setup',
        content: 'Don\'t have an SSH key? Expand the "Generate one automatically" section when adding a server. Enter your server password once — Operon generates an ed25519 key, copies it to the server, and stores it locally. You\'ll never need the password again.',
      },
      {
        title: 'File transfer (drag & drop)',
        content: 'Drag files from your Mac\'s Finder directly into the remote file explorer to upload them to the server via SCP. A blue overlay shows the drop target. Multiple files and folders are supported — each transfer shows a live progress bar. Right-click any remote file or folder and select "Download to local" to save it to your ~/Downloads folder.',
        tip: 'This works like CyberDuck or WinSCP but built right into the IDE. Use it to move scripts, data files, and results between your laptop and the cluster.',
      },
      {
        title: 'HPC tips',
        content: 'the agent can submit Slurm/PBS jobs, check queue status, parse log files, and process results on your cluster. Try: "Submit a STAR alignment job for the samples in /data/fastq/" or "Check the status of my running jobs".',
      },
    ],
  },
  {
    id: 'server-config',
    title: 'Server Configuration',
    icon: Settings2,
    iconColor: 'text-cyan-400',
    items: [
      {
        title: 'What is Server Configuration?',
        content: 'Server Configuration lets you save HPC-specific settings — like your SLURM account, GPU/CPU partitions, conda environments, and working directories — directly on each SSH profile. Once set, these values are automatically injected into every protocol and AI-generated script for that server.',
        tip: 'Set it once per server, use it everywhere. No more copy-pasting SLURM account names into every script.',
      },
      {
        title: 'Setting up server config',
        content: 'Open the SSH view in the sidebar, then double-click a server profile to edit it. Scroll down and expand the "Server Configuration" section. Fill in any fields that apply to your server — SLURM account, partitions, conda env, paths, etc. Click "Update Connection" to save.',
        action: { label: 'Open SSH View', view: 'ssh' },
      },
      {
        title: 'Available fields',
        content: 'Built-in fields include: SLURM Account, CPU Partition, GPU Partition, GPU Type, Default Conda Env, Default Modules, Scratch Directory, and Working Directory. You can also add custom key-value pairs for anything specific to your setup (e.g., PI name, email, project code).',
      },
      {
        title: 'How it works with AI',
        content: 'When you\'re connected to a remote server and send a message, Operon automatically includes your server config in the prompt context. So when you say "submit a STAR alignment job", the agent already knows your SLURM account, which partition to use, and where your scratch space is — no need to specify these details every time.',
        tip: 'This works with both free-form chat and protocols. Protocols that generate SLURM scripts will use your saved account and partition automatically.',
      },
      {
        title: 'Custom variables',
        content: 'Click "+ Add custom variable" at the bottom of the Server Configuration section to add any key-value pair. These are available in the same way as built-in fields. Useful for lab-specific values like PI names, project codes, or shared data paths that you reference frequently.',
      },
    ],
  },
  {
    id: 'protocols',
    title: 'Protocols',
    icon: BookOpen,
    iconColor: 'text-indigo-400',
    items: [
      {
        title: 'What are protocols?',
        content: 'Protocols are reusable prompt templates for common workflows. Each protocol is a folder with a PROTOCOL.md entry point plus optional reference files, scripts, and templates. When activated, the protocol context is included with every message to the agent.',
        action: { label: 'View Protocols', view: 'protocols' },
      },
      {
        title: 'Creating a protocol',
        content: 'Create a folder in ~/.operon/protocols/ with a PROTOCOL.md file. This markdown file should describe the workflow, expected inputs, and how the agent should handle each step. You can include sub-folders with reference docs, example configs, or script templates.',
      },
      {
        title: 'Example use cases',
        content: 'Protocols are great for standardized workflows: RNA-seq analysis pipelines, variant calling procedures, quality control checklists, paper writing templates, or lab notebook formatting. Any workflow you repeat regularly can become a protocol.',
      },
    ],
  },
  {
    id: 'mcp-servers',
    title: 'MCP Servers',
    icon: Plug,
    iconColor: 'text-rose-400',
    items: [
      {
        title: 'What are MCP servers?',
        content: 'MCP (Model Context Protocol) servers are plugins that give the agent access to external tools and databases during a chat session. When an MCP server is enabled, the agent can automatically call its tools to search databases, fetch data, and perform analyses — all within the conversation.',
        tip: 'Think of MCP servers as superpowers for the agent. Without them, the agent only knows what\'s in your project files. With them, the agent can query ENCODE, PubMed, protein databases, and more.',
      },
      {
        title: 'Built-in research catalog',
        content: 'Operon ships with a curated catalog of research-focused MCP servers:\n\n• ENCODE Toolkit — Access 14 genomics databases including ENCODE, GTEx, ClinVar, GWAS Catalog, JASPAR, CellxGene, gnomAD, Ensembl, UCSC Genome Browser, GEO, PubMed, bioRxiv, ClinicalTrials.gov, and Open Targets. Provides 20 tools for searching experiments, downloading files, and analyzing genomic data.\n\n• BioMCP — Protein structure analysis via PDB. Tools for analyzing active sites and searching disease-associated proteins.',
      },
      {
        title: 'Enabling an MCP server',
        content: 'Go to Settings → MCP Servers. You\'ll see the Research Tools Catalog with available servers. Toggle a server on to enable it. Operon will check that the required runtime (Python or Node.js) is installed, then configure the server automatically. The server becomes available in your next chat session.',
        action: { label: 'Open Settings', view: 'settings' },
      },
      {
        title: 'Using MCP tools in chat',
        content: 'Once a server is enabled, just start a new chat and ask naturally. the agent will call the right tools automatically. For example:\n\n• "Search ENCODE for ATAC-seq experiments in human brain"\n• "Find ClinVar variants associated with BRCA1"\n• "What GTEx tissues show highest TP53 expression?"\n• "Analyze the active site of PDB structure 1A2B"\n\nWhen the agent calls a tool, you\'ll see a labeled badge (e.g. "ENCODE") with the tool name and an expandable view of the input/output.',
      },
      {
        title: 'Adding custom MCP servers',
        content: 'In Settings → MCP Servers, scroll to the "Custom Servers" section. Enter a name, the command to run the server (e.g. "npx" or "uvx"), and any arguments. This lets you connect any MCP-compatible server — your own or third-party — to Operon.',
        tip: 'Custom servers follow the same protocol. Any server that speaks MCP over stdio will work.',
      },
      {
        title: 'Remote MCP servers',
        content: 'MCP servers can also run on remote machines via SSH. When you start a remote chat session, Operon writes the MCP config to the remote server and passes it to the agent running on that machine. This means you can use ENCODE Toolkit or BioMCP even when running the agent on your HPC cluster.',
      },
      {
        title: 'Runtime requirements',
        content: 'Each MCP server requires a runtime:\n\n• ENCODE Toolkit — Python 3.10+ (install with: pip install encode-toolkit)\n• BioMCP — Node.js 20+ (install with: npm install -g @anthropic-ai/bio-mcp)\n\nOperon checks these dependencies before enabling a server and will show install instructions if something is missing.',
      },
    ],
  },
  {
    id: 'extensions',
    title: 'Extensions',
    icon: Puzzle,
    iconColor: 'text-violet-400',
    items: [
      {
        title: 'What are extensions?',
        content: 'Extensions add language support, themes, snippets, and other features to Operon\'s code editor. Operon uses the Open VSX registry (the open-source VS Code marketplace) so you can install thousands of extensions for syntax highlighting, code intelligence, and more.',
      },
      {
        title: 'Browsing and installing',
        content: 'Open the Extensions view from the Activity Bar (puzzle piece icon). Search by name or browse by category. Click "Install" on any extension to download and activate it. Extensions are stored in ~/.config/operon/extensions/ and persist across sessions.',
      },
      {
        title: 'Language Server Protocol (LSP)',
        content: 'Many extensions include a language server that provides real-time code intelligence — autocompletion, hover docs, go-to-definition, error diagnostics, and more. When you open a file, Operon automatically starts the matching language server if one is available from your installed extensions.',
        tip: 'If no LSP is installed for a language you\'re editing, Operon can recommend an extension to install.',
      },
      {
        title: 'Themes and snippets',
        content: 'Extensions can include color themes for the editor and code snippets. After installing a theme extension, go to Settings to select it. Snippets are available automatically — type a snippet prefix in the editor and select from autocomplete.',
      },
      {
        title: 'Extension settings',
        content: 'Many extensions expose configuration options. Go to Settings → Extensions to see per-extension settings. These are parsed from the extension\'s package.json and rendered as forms — toggles, dropdowns, and text fields that you can adjust.',
        action: { label: 'Open Settings', view: 'settings' },
      },
      {
        title: 'Sideloading a VSIX',
        content: 'Have a .vsix file you downloaded manually? Use the sideload option in the Extensions view to install it directly without going through the registry. This is useful for private extensions or pre-release versions.',
      },
      {
        title: 'Remote extensions',
        content: 'When working on a remote server via SSH, Operon can install extensions on the remote machine and run language servers there. This gives you full code intelligence for remote projects without needing anything installed locally beyond Operon itself.',
      },
      {
        title: 'Docker & Singularity tools',
        content: 'Operon includes built-in tool extensions for container management:\n\n• Docker — List, start, stop, restart, and remove containers. View images and volumes. Read container logs. All from the sidebar without opening a terminal.\n\n• Singularity/Apptainer — Manage .sif images and instances for HPC environments. Pull images, start/stop instances, and run commands inside containers.',
      },
    ],
  },
  {
    id: 'shortcuts',
    title: 'Keyboard Shortcuts',
    icon: Keyboard,
    iconColor: 'text-zinc-400',
    items: [
      {
        title: 'Chat shortcuts',
        content: 'Cmd+K: New conversation\nCmd+L: Focus chat input\nEnter: Send message\nShift+Enter: New line in message\nEsc: Stop the agent\'s response',
      },
      {
        title: 'Editor shortcuts',
        content: 'Cmd+S: Save file\nCmd+W: Close tab\nCmd+P: Quick open file\nCmd+Shift+P: Command palette\nCmd+/: Toggle comment\nCmd+D: Select next occurrence',
      },
      {
        title: 'Navigation',
        content: 'Cmd+1: Focus sidebar\nCmd+B: Toggle sidebar\nCmd+J: Toggle terminal\nCmd+\\: Focus editor\nCmd+Shift+E: Explorer view',
      },
    ],
  },
  {
    id: 'tips',
    title: 'Tips & Best Practices',
    icon: Zap,
    iconColor: 'text-yellow-400',
    items: [
      {
        title: 'Be specific with the agent',
        content: 'Instead of "fix this", try "fix the TypeError on line 45 of parser.py — it\'s failing when the input file has empty lines". The more context you provide, the better the agent\'s response.',
      },
      {
        title: 'Use Plan mode for complex tasks',
        content: 'For multi-file changes, pipeline setups, or architectural decisions, switch to Plan mode first. Review the plan, give feedback, and iterate before the agent writes any code. This prevents wasted effort on the wrong approach.',
      },
      {
        title: 'Reference files with @',
        content: 'Type @ in the chat input to reference specific files. This helps the agent focus on exactly the files you care about instead of searching the entire project.',
      },
      {
        title: 'Break big tasks into steps',
        content: 'Instead of "build me a complete analysis pipeline", start with "set up the project structure and config for a Nextflow RNA-seq pipeline", then iterate from there. the agent works best with focused, incremental tasks.',
      },
      {
        title: 'Check the agent\'s work',
        content: 'Always review diffs before accepting changes. Use Ask mode to have the agent explain its approach. Run tests after the agent makes changes. the agent is powerful but not infallible — treat it as a very capable collaborator, not an oracle.',
      },
    ],
  },
];

export function HelpPanel({ isOpen, onClose, onNavigate }: HelpPanelProps) {
  const [activeSection, setActiveSection] = useState('getting-started');
  const [expandedItem, setExpandedItem] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');

  if (!isOpen) return null;

  const currentSection = sections.find((s) => s.id === activeSection);

  // Filter items by search
  const filteredSections = searchQuery.trim()
    ? sections
        .map((section) => ({
          ...section,
          items: section.items.filter(
            (item) =>
              item.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
              item.content.toLowerCase().includes(searchQuery.toLowerCase())
          ),
        }))
        .filter((section) => section.items.length > 0)
    : null;

  const handleAction = (view: string) => {
    onClose();
    onNavigate?.(view);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/60" onClick={onClose} />
      <div className="relative w-[750px] max-h-[80vh] bg-zinc-900 rounded-xl border border-zinc-700 shadow-2xl flex overflow-hidden">
        {/* Left nav */}
        <div className="w-[200px] border-r border-zinc-800 flex flex-col shrink-0">
          <div className="flex items-center gap-2 px-4 py-3 border-b border-zinc-800">
            <HelpCircle className="w-4 h-4 text-blue-400" />
            <span className="text-sm font-medium text-zinc-300">Help</span>
          </div>

          {/* Search */}
          <div className="px-3 py-2 border-b border-zinc-800">
            <div className="relative">
              <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3 h-3 text-zinc-600" />
              <input
                type="text"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Search..."
                className="w-full bg-zinc-800 border border-zinc-700 rounded pl-7 pr-2 py-1 text-xs text-zinc-200 outline-none focus:border-zinc-600 placeholder:text-zinc-600"
                spellCheck={false}
              />
            </div>
          </div>

          {/* Section list */}
          <div className="flex-1 overflow-y-auto py-1">
            {sections.map((section) => {
              const Icon = section.icon;
              const isActive = activeSection === section.id;
              const matchCount = filteredSections
                ? filteredSections.find((s) => s.id === section.id)?.items.length
                : undefined;

              if (filteredSections && !matchCount) return null;

              return (
                <button
                  key={section.id}
                  onClick={() => {
                    setActiveSection(section.id);
                    setExpandedItem(null);
                  }}
                  className={`w-full text-left flex items-center gap-2 px-4 py-2 text-sm transition-colors ${
                    isActive
                      ? 'bg-zinc-800 text-zinc-100'
                      : 'text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/50'
                  }`}
                >
                  <Icon className={`w-3.5 h-3.5 ${section.iconColor} shrink-0`} />
                  <span className="truncate">{section.title}</span>
                  {matchCount !== undefined && (
                    <span className="text-[10px] text-blue-400 ml-auto">{matchCount}</span>
                  )}
                </button>
              );
            })}
          </div>

          {/* Footer */}
          <div className="px-4 py-2 border-t border-zinc-800">
            <p className="text-[10px] text-zinc-600 leading-relaxed">
              Powered by OMP
            </p>
          </div>
        </div>

        {/* Right content */}
        <div className="flex-1 overflow-y-auto p-5">
          <button
            onClick={onClose}
            className="absolute top-3 right-3 p-1 rounded hover:bg-zinc-800 text-zinc-500 hover:text-zinc-300"
          >
            <X className="w-4 h-4" />
          </button>

          {/* If searching, show flat results */}
          {filteredSections ? (
            <div className="space-y-4">
              <h3 className="text-sm font-medium text-zinc-300 mb-3">
                Search results for "{searchQuery}"
              </h3>
              {filteredSections.length === 0 ? (
                <div className="flex flex-col items-center justify-center py-12 text-center">
                  <Search className="w-6 h-6 text-zinc-700 mb-2" />
                  <p className="text-sm text-zinc-500">No results found</p>
                  <button
                    onClick={() => setSearchQuery('')}
                    className="text-xs text-blue-400 hover:text-blue-300 mt-2"
                  >
                    Clear search
                  </button>
                </div>
              ) : (
                filteredSections.map((section) =>
                  section.items.map((item, idx) => (
                    <ItemCard
                      key={`${section.id}-${idx}`}
                      item={item}
                      sectionTitle={section.title}
                      onAction={handleAction}
                    />
                  ))
                )
              )}
            </div>
          ) : currentSection ? (
            <div>
              <div className="flex items-center gap-2 mb-4">
                <currentSection.icon className={`w-5 h-5 ${currentSection.iconColor}`} />
                <h3 className="text-base font-medium text-zinc-200">{currentSection.title}</h3>
              </div>

              <div className="space-y-1">
                {currentSection.items.map((item, idx) => {
                  const itemId = `${currentSection.id}-${idx}`;
                  const isExpanded = expandedItem === itemId;

                  return (
                    <div key={itemId} className="border border-zinc-800 rounded-lg overflow-hidden">
                      <button
                        onClick={() => setExpandedItem(isExpanded ? null : itemId)}
                        className={`w-full flex items-center gap-2 px-4 py-2.5 text-left transition-colors ${
                          isExpanded ? 'bg-zinc-800/60' : 'hover:bg-zinc-800/30'
                        }`}
                      >
                        {isExpanded ? (
                          <ChevronDown className="w-3.5 h-3.5 text-zinc-500 shrink-0" />
                        ) : (
                          <ChevronRight className="w-3.5 h-3.5 text-zinc-500 shrink-0" />
                        )}
                        <span className={`text-sm ${isExpanded ? 'text-zinc-200' : 'text-zinc-400'}`}>
                          {item.title}
                        </span>
                        {item.shortcut && (
                          <kbd className="text-[10px] bg-zinc-800 px-1.5 py-0.5 rounded text-zinc-500 font-mono ml-auto shrink-0">
                            {adaptShortcut(item.shortcut)}
                          </kbd>
                        )}
                      </button>

                      {isExpanded && (
                        <div className="px-4 pb-3 pt-1 ml-5 space-y-2.5">
                          <p className="text-[12px] text-zinc-400 leading-relaxed whitespace-pre-line">
                            {adaptShortcut(item.content)}
                          </p>

                          {item.tip && (
                            <div className="flex gap-2 p-2.5 bg-blue-950/20 rounded-lg border border-blue-900/20">
                              <Zap className="w-3.5 h-3.5 text-blue-400 shrink-0 mt-0.5" />
                              <p className="text-[11px] text-blue-300/80 leading-relaxed">{item.tip}</p>
                            </div>
                          )}

                          {item.action && (
                            <button
                              onClick={() => handleAction(item.action!.view)}
                              className="inline-flex items-center gap-1.5 px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 text-[11px] text-zinc-300 rounded-md transition-colors"
                            >
                              <PlayCircle className="w-3.5 h-3.5 text-blue-400" />
                              {item.action.label}
                            </button>
                          )}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

// Expandable card component for search results
function ItemCard({
  item,
  sectionTitle,
  onAction,
}: {
  item: HelpItem;
  sectionTitle: string;
  onAction: (view: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  return (
    <div
      className="p-3 bg-zinc-800/50 rounded-lg border border-zinc-800 cursor-pointer hover:bg-zinc-800/70 transition-colors"
      onClick={() => setExpanded(!expanded)}
    >
      <div className="flex items-center gap-2 mb-1">
        {expanded ? (
          <ChevronDown className="w-3.5 h-3.5 text-zinc-500 shrink-0" />
        ) : (
          <ChevronRight className="w-3.5 h-3.5 text-zinc-500 shrink-0" />
        )}
        <span className="text-sm text-zinc-200">{item.title}</span>
        <span className="text-[10px] text-zinc-600 ml-auto shrink-0">{sectionTitle}</span>
      </div>
      <p className={`text-[11px] text-zinc-500 leading-relaxed ml-5 ${expanded ? '' : 'line-clamp-2'}`}>
        {adaptShortcut(item.content)}
      </p>
      {expanded && item.tip && (
        <div className="flex gap-2 p-2.5 mt-2 ml-5 bg-blue-950/20 rounded-lg border border-blue-900/20">
          <Zap className="w-3.5 h-3.5 text-blue-400 shrink-0 mt-0.5" />
          <p className="text-[11px] text-blue-300/80 leading-relaxed">{item.tip}</p>
        </div>
      )}
      {item.action && (
        <button
          onClick={(e) => { e.stopPropagation(); onAction(item.action!.view); }}
          className="inline-flex items-center gap-1 mt-2 ml-5 text-[10px] text-blue-400 hover:text-blue-300"
        >
          <PlayCircle className="w-3 h-3" />
          {item.action.label}
        </button>
      )}
    </div>
  );
}
