/**
 * Basic-Studio Node.js SDK（零依赖，Node >= 18，使用内置 fetch）。
 */

export const DEFAULT_BASE_URL = "http://127.0.0.1:18596";

export class StudioError extends Error {
  /**
   * @param {string} message
   * @param {number} [status]
   * @param {unknown} [body]
   */
  constructor(message, status, body) {
    super(message);
    this.name = "StudioError";
    this.status = status;
    this.body = body;
  }
}

function normalizeMessages(messages, system) {
  if (typeof messages === "string") {
    return system
      ? [{ role: "system", content: system }, { role: "user", content: messages }]
      : [{ role: "user", content: messages }];
  }

  const list = messages.map((item) => {
    if (typeof item === "string") return { role: "user", content: item };
    if (item instanceof ChatMessage) return item.toJSON();
    return item;
  });

  if (system) list.unshift({ role: "system", content: system });
  return list;
}

export class ChatMessage {
  /** @param {string} role @param {unknown} content */
  constructor(role, content) {
    this.role = role;
    this.content = content;
  }

  toJSON() {
    return { role: this.role, content: this.content };
  }

  static system(content) {
    return new ChatMessage("system", content);
  }

  static user(content) {
    return new ChatMessage("user", content);
  }

  static assistant(content) {
    return new ChatMessage("assistant", content);
  }
}

export class Client {
  /**
   * @param {{ baseUrl?: string, apiKey?: string, timeout?: number }} [options]
   */
  constructor(options = {}) {
    this.baseUrl = (options.baseUrl ?? DEFAULT_BASE_URL).replace(/\/+$/, "");
    this.apiKey = options.apiKey;
    this.timeout = options.timeout ?? 60_000;
  }

  headers(extra = {}) {
    /** @type {Record<string,string>} */
    const headers = { "content-type": "application/json" };
    if (this.apiKey) headers.authorization = `Bearer ${this.apiKey}`;
    return { ...headers, ...extra };
  }

  async request(method, path, body, accept = "application/json") {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeout);
    try {
      const response = await fetch(`${this.baseUrl}${path}`, {
        method,
        headers: this.headers({ accept }),
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: controller.signal,
      });

      if (!response.ok) {
        const text = await response.text();
        let parsed;
        try {
          parsed = JSON.parse(text);
        } catch {
          parsed = text;
        }
        const message = parsed?.error?.message ?? `http status ${response.status}`;
        throw new StudioError(message, response.status, parsed);
      }
      return response;
    } catch (error) {
      if (error instanceof StudioError) throw error;
      if (error?.name === "AbortError") {
        throw new StudioError(`request to ${this.baseUrl} timed out`);
      }
      throw new StudioError(`cannot reach ${this.baseUrl}: ${error?.message ?? error}`);
    } finally {
      clearTimeout(timer);
    }
  }

  async json(method, path, body) {
    const response = await this.request(method, path, body);
    return response.json();
  }

  async health() {
    return this.json("GET", "/healthz");
  }

  async ready() {
    return this.json("GET", "/readyz");
  }

  async models() {
    const payload = await this.json("GET", "/v1/models");
    return payload.data ?? [];
  }

  async chat(options) {
    const { model, messages, system, temperature, topP, maxTokens } = options;
    const body = {
      model,
      messages: normalizeMessages(messages, system),
      stream: false,
    };
    if (temperature !== undefined) body.temperature = temperature;
    if (topP !== undefined) body.top_p = topP;
    if (maxTokens !== undefined) body.max_tokens = maxTokens;

    return this.json("POST", "/v1/chat/completions", body);
  }

  /** 流式对话，返回文本增量异步迭代器。 */
  async *chatStream(options) {
    const { model, messages, system, temperature, topP, maxTokens } = options;
    const body = {
      model,
      messages: normalizeMessages(messages, system),
      stream: true,
    };
    if (temperature !== undefined) body.temperature = temperature;
    if (topP !== undefined) body.top_p = topP;
    if (maxTokens !== undefined) body.max_tokens = maxTokens;

    const response = await this.request("POST", "/v1/chat/completions", body, "text/event-stream");
    for await (const payload of sseEvents(response)) {
      const choices = payload.choices ?? [];
      if (!choices.length) continue;
      const delta = choices[0]?.delta?.content;
      if (delta) yield delta;
    }
  }

  async responses(options) {
    const { model, input, instructions, temperature, maxOutputTokens, previousResponseId } = options;
    const body = { model, input, stream: false };
    if (instructions !== undefined) body.instructions = instructions;
    if (temperature !== undefined) body.temperature = temperature;
    if (maxOutputTokens !== undefined) body.max_output_tokens = maxOutputTokens;
    if (previousResponseId !== undefined) body.previous_response_id = previousResponseId;

    return this.json("POST", "/v1/responses", body);
  }

  /** Responses 流式接口，产出原始事件对象。 */
  async *responsesStream(options) {
    const { model, input, instructions } = options;
    const body = { model, input, stream: true };
    if (instructions !== undefined) body.instructions = instructions;

    const response = await this.request("POST", "/v1/responses", body, "text/event-stream");
    for await (const payload of sseEvents(response)) {
      if (payload.type === "response.failed") {
        throw new StudioError(payload.message ?? "response failed");
      }
      yield payload;
    }
  }

  async embeddings(options) {
    const { model, input } = options;
    return this.json("POST", "/v1/embeddings", { model, input });
  }

  /** 语音合成，返回 PCM（16bit little-endian）字节。 */
  async tts(options) {
    const { model, text, voice, emotion } = options;
    const body = { model, input: text, stream: false };
    if (voice !== undefined) body.voice = voice;
    if (emotion !== undefined) body.emotion = emotion;

    const response = await this.request("POST", "/v1/audio/speech", body, "audio/pcm");
    return new Uint8Array(await response.arrayBuffer());
  }

  async decide(options) {
    const { model, state, context } = options;
    const body = { model, state };
    if (context !== undefined) body.context = context;
    return this.json("POST", "/v1/decide", body);
  }
}

/**
 * 解析 SSE 响应体，逐条产出 JSON。
 * @param {Response} response
 */
async function* sseEvents(response) {
  const reader = response.body?.getReader();
  if (!reader) return;

  const decoder = new TextDecoder();
  let buffer = "";

  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });

    let index;
    while ((index = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, index).trim();
      buffer = buffer.slice(index + 1);

      if (!line.startsWith("data:")) continue;
      const payload = line.slice(5).trim();
      if (!payload || payload === "[DONE]") continue;

      try {
        yield JSON.parse(payload);
      } catch {
        // 忽略无法解析的心跳行
      }
    }
  }
}

export default Client;