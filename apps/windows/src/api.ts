import { invoke } from "@tauri-apps/api/core";
export type Preferences = {
  language: "zh" | "en";
  theme: "system" | "light" | "dark";
};
export type Kind =
  | "openai"
  | "anthropic"
  | "google"
  | "deepseek"
  | "qwen"
  | "openrouter"
  | "ollama"
  | "custom_responses"
  | "custom_chat_completions";
export type Profile = {
  id: string;
  name: string;
  kind: Kind;
  endpoint: string;
  model: string;
};
export type ProfileStatus = { profile: Profile; configured: boolean };
export type Conversation = {
  id: string;
  title: string;
  project_id: string | null;
};
export type Message = {
  id: string;
  parent: string | null;
  role: string;
  sequence: number;
  content: { text: string; attachments: string[] };
  end: string;
};
export type Snapshot = {
  messages: Message[];
  operation: string | null;
  draft: string | null;
};
export type Bootstrap = {
  preferences: Preferences;
  profiles: ProfileStatus[];
  selected: string | null;
  conversations: Conversation[];
};
// Only this typed boundary reaches Rust. No token/key read endpoint or web fallback.
export const api = {
  bootstrap: () => invoke<Bootstrap>("bootstrap"),
  preferences: (preferences: Preferences) =>
    invoke<void>("preferences", { preferences }),
  saveProfile: (profile: Profile, key: string | null) =>
    invoke<void>("save_profile", { profile, key }),
  selectProfile: (id: string) => invoke<void>("select_profile", { id }),
  removeKey: (id: string) => invoke<void>("remove_key", { id }),
  create: (title: string) => invoke<string>("create", { title }),
  conversations: (after: string) =>
    invoke<Conversation[]>("conversations", { after }),
  snapshot: (conversation: string, after = 0) =>
    invoke<Snapshot>("snapshot", { conversation, after }),
  draft: (conversation: string, body: string | null = null) =>
    invoke<string>("draft", { conversation, body }),
  attach: (
    conversation: string,
    name: string,
    mediaType: string,
    bytes: number[],
  ) => invoke<string>("attach", { conversation, name, mediaType, bytes }),
  send: (
    operation: string,
    conversation: string,
    parent: string | null,
    text: string,
    attachments: string[],
  ) =>
    invoke<Message>("send", {
      operation,
      conversation,
      parent,
      text,
      attachments,
    }),
  cancel: (operation: string) => invoke<void>("cancel", { operation }),
  exportChat: (conversation: string) =>
    invoke<string>("export_chat", { conversation }),
};
export const providers: {
  kind: Kind;
  name: string;
  endpoint: string;
  example: string;
}[] = [
  {
    kind: "openai",
    name: "OpenAI",
    endpoint: "https://api.openai.com/v1/",
    example: "gpt-5",
  },
  {
    kind: "anthropic",
    name: "Anthropic",
    endpoint: "https://api.anthropic.com/v1/",
    example: "claude-sonnet-4-5",
  },
  {
    kind: "google",
    name: "Google Gemini",
    endpoint: "https://generativelanguage.googleapis.com/v1beta/",
    example: "models/gemini-2.5-flash",
  },
  {
    kind: "deepseek",
    name: "DeepSeek",
    endpoint: "https://api.deepseek.com/",
    example: "deepseek-chat",
  },
  {
    kind: "qwen",
    name: "Qwen",
    endpoint: "https://dashscope.aliyuncs.com/",
    example: "qwen-plus",
  },
  {
    kind: "openrouter",
    name: "OpenRouter",
    endpoint: "https://openrouter.ai/api/v1/",
    example: "provider/model",
  },
  {
    kind: "ollama",
    name: "Ollama",
    endpoint: "http://127.0.0.1:11434/v1/",
    example: "llama3.2",
  },
  {
    kind: "custom_responses",
    name: "Custom · Responses",
    endpoint: "",
    example: "model-id",
  },
  {
    kind: "custom_chat_completions",
    name: "Custom · Chat Completions",
    endpoint: "",
    example: "model-id",
  },
];
