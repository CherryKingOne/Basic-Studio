export type ChatRole = "system" | "user" | "assistant";

export interface ChatMessageLike {
  role: ChatRole;
  content: unknown;
}

export declare class ChatMessage implements ChatMessageLike {
  role: ChatRole;
  content: unknown;
  constructor(role: ChatRole, content: unknown);
  toJSON(): ChatMessageLike;
  static system(content: unknown): ChatMessage;
  static user(content: unknown): ChatMessage;
  static assistant(content: unknown): ChatMessage;
}

export interface ClientOptions {
  baseUrl?: string;
  apiKey?: string;
  timeout?: number;
}

export interface ModelInfo {
  id: string;
  object?: string;
  task?: string;
  engines?: string[];
  adapter?: string | null;
  api_version?: string | null;
  streaming?: boolean;
}

export interface ChatOptions {
  model: string;
  messages: string | Array<string | ChatMessageLike | ChatMessage>;
  system?: string;
  temperature?: number;
  topP?: number;
  maxTokens?: number;
}

export interface ChatResponse {
  id: string;
  object?: string;
  model: string;
  choices: Array<{
    index?: number;
    message?: { role?: string; content?: string };
    finish_reason?: string | null;
  }>;
  engine?: string;
  usage?: { prompt_tokens: number; completion_tokens: number; total_tokens: number };
}

export interface ResponsesOptions {
  model: string;
  input: string | ChatMessageLike[];
  instructions?: string;
  temperature?: number;
  maxOutputTokens?: number;
  previousResponseId?: string;
}

export interface ResponsesResponse {
  id: string;
  object?: string;
  model: string;
  status?: string;
  output_text?: string;
  output?: unknown[];
}

export interface SpeechOptions {
  model: string;
  text: string;
  voice?: string;
  emotion?: string;
}

export interface DecideOptions {
  model: string;
  state: unknown;
  context?: string;
}

export declare class StudioError extends Error {
  status?: number;
  body?: unknown;
}

export declare const DEFAULT_BASE_URL: string;

export declare class Client {
  constructor(options?: ClientOptions);
  baseUrl: string;
  apiKey?: string;
  timeout: number;

  health(): Promise<Record<string, unknown>>;
  ready(): Promise<Record<string, unknown>>;
  models(): Promise<ModelInfo[]>;

  chat(options: ChatOptions): Promise<ChatResponse>;
  chatStream(options: ChatOptions): AsyncGenerator<string, void, unknown>;

  responses(options: ResponsesOptions): Promise<ResponsesResponse>;
  responsesStream(options: ResponsesOptions): AsyncGenerator<Record<string, unknown>, void, unknown>;

  embeddings(options: { model: string; input: string }): Promise<Record<string, unknown>>;

  tts(options: SpeechOptions): Promise<Uint8Array>;

  decide(options: DecideOptions): Promise<{ model: string; output: string; engine?: string }>;
}

export default Client;