import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api, type AppInfo, type SessionMeta, type ProviderSource } from "./api";

export type Panel = "chat" | "sessions" | "settings";

export interface DisplayItem {
  type: "user" | "assistant" | "reasoning" | "tool" | "error" | "thinking";
  content?: string;
  streaming?: boolean;
  name?: string;
  args?: string;
  result?: string;
  running?: boolean;
}

interface PendingChoice {
  id: string;
  mode: string;
  options: string[];
}

interface State {
  display: DisplayItem[];
  appInfo: AppInfo | null;
  sessions: SessionMeta[];
  providers: ProviderSource[];
  isRunning: boolean;
  activePanel: Panel;
  toasts: Toast[];
  sidebarOpen: boolean;
  pendingChoice: PendingChoice | null;
}

export interface Toast { id: number; message: string; level: string; }

type Listener = () => void;
let toastId = 0;

class Store {
  private state: State = {
    display: [],
    appInfo: null,
    sessions: [],
    providers: [],
    isRunning: false,
    activePanel: "chat",
    toasts: [],
    sidebarOpen: false,
    pendingChoice: null,
  };
  private listeners: Set<Listener> = new Set();
  private unlisteners: UnlistenFn[] = [];

  get(): State { return this.state; }

  subscribe(fn: Listener) {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  private update(partial: Partial<State>) {
    Object.assign(this.state, partial);
    this.listeners.forEach((fn) => fn());
  }

  addToast(message: string, level: string = "info") {
    const id = ++toastId;
    this.state.toasts.push({ id, message, level });
    this.listeners.forEach((fn) => fn());
    setTimeout(() => {
      this.state.toasts = this.state.toasts.filter((t) => t.id !== id);
      this.listeners.forEach((fn) => fn());
    }, 4000);
  }

  setPanel(panel: Panel) {
    this.update({ activePanel: panel, sidebarOpen: panel !== "chat" });
  }

  toggleSidebar() {
    const open = !this.state.sidebarOpen;
    this.update({
      sidebarOpen: open,
      activePanel: open ? (this.state.activePanel === "chat" ? "sessions" : this.state.activePanel) : "chat",
    });
  }

  async init() {
    try {
      const info = await api.getAppInfo();
      this.update({ appInfo: info });
    } catch (e) {
      console.error("Failed to get app info:", e);
      this.addToast("Failed to load app info", "error");
    }
    // Load provider registry early so the UI can resolve api_base/model.
    await this.loadProviders();
    // Fetch initial display from backend
    try {
      const display = await api.getDisplay();
      this.update({ display });
    } catch (e) {
      console.error("Failed to get display:", e);
    }
    this.setupEventListeners();
  }

  private setupEventListeners() {
    const on = async (event: string, handler: (payload: any) => void) => {
      const unlisten = await listen(event, (e) => handler(e.payload));
      this.unlisteners.push(unlisten);
    };

    // Main display sync — replaces everything
    on("display-sync", (display: DisplayItem[]) => {
      this.state.display = display;
      this.state.isRunning = display.some(
        (d) => (d.type === "assistant" && d.streaming) || (d.type === "reasoning" && d.streaming) || (d.type === "tool" && d.running) || d.type === "thinking"
      );
      this.listeners.forEach((fn) => fn());
    });

    on("toast", (data: { message: string; level: string }) => {
      this.addToast(data.message, data.level);
    });

    on("session-loaded", () => {
      this.setPanel("chat");
    });

    on("providers-loaded", (data: { sources: ProviderSource[] }) => {
      this.update({ providers: data.sources });
    });

    on("provider-changed", async () => {
      try {
        const info = await api.getAppInfo();
        this.update({ appInfo: info });
      } catch {}
    });

    on("choice", (data: { id: string; mode: string; options: string[] }) => {
      this.showChoiceModal(data.id, data.mode, data.options);
    });
  }

  showChoiceModal(id: string, mode: string, options: string[]) {
    this.update({ pendingChoice: { id, mode, options } });
  }

  async resolveChoice(value: string) {
    const choice = this.state.pendingChoice;
    if (!choice) return;
    this.update({ pendingChoice: null });
    try {
      await api.choiceResponse(choice.id, value);
    } catch (e: any) {
      this.addToast(`Choice failed: ${e}`, "error");
    }
  }

  cancelChoice() {
    this.update({ pendingChoice: null });
  }

  async sendMessage(task: string) {
    if (this.state.isRunning) return;
    try {
      await api.runTask(task);
    } catch (e: any) {
      this.addToast(String(e), "error");
    }
  }

  async cancelTask() {
    try {
      await api.cancelTask();
      this.addToast("Cancelled", "info");
    } catch {}
  }

  async newSession() {
    if (this.state.isRunning) await this.cancelTask();
    await api.newSession();
    this.addToast("New session", "info");
  }

  async loadSessions() {
    try {
      this.update({ sessions: await api.listSessions() });
    } catch {}
  }

  async loadSession(name: string) {
    try {
      await api.loadSession(name);
      this.addToast(`Loaded: ${name}`, "info");
    } catch (e: any) {
      this.addToast(`Failed: ${e}`, "error");
    }
  }

  async saveSession(name: string, description?: string) {
    try {
      await api.saveSession(name, description);
      this.addToast(`Saved: ${name}`, "info");
      await this.loadSessions();
    } catch (e: any) {
      this.addToast(`Failed: ${e}`, "error");
    }
  }

  async deleteSession(name: string) {
    try {
      await api.deleteSession(name);
      this.addToast(`Deleted: ${name}`, "info");
      await this.loadSessions();
    } catch (e: any) {
      this.addToast(`Failed: ${e}`, "error");
    }
  }

  async loadProviders() {
    try {
      this.update({ providers: await api.fetchProviders() });
    } catch (e: any) {
      console.error("Failed to load providers:", e);
      this.addToast(`Failed to load providers: ${e}`, "error");
    }
  }

  async switchProvider(providerName: string, apiBase: string, apiKey: string, model: string, apiType?: string) {
    try {
      const info = await api.setProvider(providerName, apiBase, apiKey, model, apiType);
      this.update({ appInfo: info });
      this.addToast(`Switched: ${providerName}/${model}`, "info");
    } catch (e: any) {
      this.addToast(`Failed: ${e}`, "error");
    }
  }

  async setModel(model: string) {
    try {
      await api.setModel(model);
      this.update({ appInfo: await api.getAppInfo() });
    } catch (e: any) {
      this.addToast(`Failed: ${e}`, "error");
    }
  }

  async setMode(mode: string) {
    try {
      await api.setMode(mode);
      this.update({ appInfo: await api.getAppInfo() });
      this.addToast(`Mode: ${mode}`, "info");
    } catch (e: any) {
      this.addToast(`Failed: ${e}`, "error");
    }
  }

  async saveApiKey(key: string) {
    try {
      await api.saveApiKey(key);
      this.update({ appInfo: await api.getAppInfo() });
      this.addToast("API key saved", "info");
    } catch (e: any) {
      this.addToast(`Failed: ${e}`, "error");
    }
  }

  async reloadConfig() {
    try {
      const info = await api.reloadConfig();
      this.update({ appInfo: info });
      await this.loadProviders();
      this.addToast("Config reloaded", "info");
    } catch (e: any) {
      this.addToast(`Failed to reload config: ${e}`, "error");
    }
  }

  showHelp() {
    const help = `**Commands** — \`/help\` \`/new\` \`/clear\` \`/sessions\` \`/settings\` \`/provider\`
\`/auto\` \`/plan\` \`/exec\` \`/model [name]\` \`/think low|high|max\`
\`/review\` \`/agents\` \`/tools\` \`/skills\` \`/models\` \`/memory\` \`/mcp\`
\`/retry\` \`/status\` \`/perf\` \`/copy\` \`/tips\``;
    this.state.display.push({ type: "assistant", content: help, streaming: false });
    this.listeners.forEach((fn) => fn());
  }

  async retry() {
    for (let i = this.state.display.length - 1; i >= 0; i--) {
      if (this.state.display[i].type === "user") {
        this.sendMessage(this.state.display[i].content!);
        return;
      }
    }
    this.addToast("No previous message", "warning");
  }

  destroy() {
    this.unlisteners.forEach((fn) => fn());
  }
}

export const store = new Store();
