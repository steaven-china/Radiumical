import { invoke } from "@tauri-apps/api/core";

export interface AppInfo {
  model: string;
  provider: string;
  api_type: string;
  mode: string;
  workspace: string;
  api_base: string;
  api_key_source: string;
  max_context_tokens: number;
  llm_timeout_secs: number;
  tool_timeout_secs: number;
}

export interface SessionMeta {
  name: string;
  created: string;
  updated: string;
  model: string;
  provider: string;
  mode: string;
  thinking_effort: string;
  description: string;
  message_count: number;
}

export interface ChatMessage {
  role: "user" | "assistant" | "tool" | "system";
  content: string;
  reasoning?: string;
  tool_name?: string;
  tool_args?: string;
  tool_result?: string;
  error?: boolean;
}

export interface ToolStartEvent {
  name: string;
  index: number;
  total: number;
  args: string;
}

export interface ProviderSource {
  provider: string;
  name: string;
  api_type: string;
  api_base: string;
  key_env?: string;
  models_endpoint?: string;
  models?: string[];
}

export interface ConfigData {
  model?: string;
  provider?: string;
  api_base?: string;
  llm_timeout_secs?: number;
  max_iterations?: number;
  reasoning_effort?: string;
  mode?: string;
  max_context_tokens?: number;
  context_compress_ratio?: number;
}

export interface DisplayItem {
  type: "user" | "assistant" | "reasoning" | "tool" | "error" | "thinking";
  content?: string;
  streaming?: boolean;
  name?: string;
  args?: string;
  result?: string;
  running?: boolean;
}

export const api = {
  runTask: (task: string) => invoke("run_task", { task }),
  cancelTask: () => invoke("cancel_task"),
  isRunning: () => invoke<boolean>("is_running"),
  newSession: () => invoke("new_session"),
  listSessions: () => invoke<SessionMeta[]>("list_sessions"),
  saveSession: (name: string, description?: string) =>
    invoke("save_session", { name, description }),
  loadSession: (name: string) => invoke("load_session", { name }),
  deleteSession: (name: string) => invoke<boolean>("delete_session", { name }),
  getDisplay: () => invoke<DisplayItem[]>("get_display"),
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  setModel: (model: string) => invoke("set_model", { model }),
  setMode: (mode: string) => invoke("set_mode", { mode }),
  fetchProviders: () => invoke<ProviderSource[]>("fetch_providers"),
  setProvider: (
    providerName: string,
    apiBase: string,
    apiKey: string,
    model: string,
    apiType?: string
  ) => invoke<AppInfo>("set_provider", {
      providerName,
      apiBase,
      apiKey,
      model,
      apiType,
    }),
  fetchModelsForProvider: (
    providerName: string,
    apiBase: string,
    apiKey: string,
    apiType?: string
  ) =>
    invoke<string[]>("fetch_models_for_provider", {
      providerName,
      apiBase,
      apiKey,
      apiType,
    }),
  getConfig: () => invoke<ConfigData>("get_config"),
  saveConfig: (config: ConfigData) => invoke("save_config", { configJson: config }),
  saveApiKey: (apiKey: string) => invoke("save_api_key", { apiKey }),
  reloadConfig: () => invoke<AppInfo>("reload_config"),
  getMessages: () => invoke<ChatMessage[]>("get_messages"),
  getConversationItems: () => invoke<ChatMessage[]>("get_conversation_items"),
  choiceResponse: (id: string, value: string) => invoke("choice_response", { id, value }),
};
