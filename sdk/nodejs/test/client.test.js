import assert from "node:assert/strict";
import { createServer } from "node:http";
import { after, before, test } from "node:test";

import { ChatMessage, Client, StudioError } from "../src/index.js";

let server;
let baseUrl;

function sendJson(res, status, payload) {
  const body = JSON.stringify(payload);
  res.writeHead(status, { "content-type": "application/json" });
  res.end(body);
}

function sendSse(res, frames) {
  res.writeHead(200, { "content-type": "text/event-stream" });
  for (const frame of frames) {
    res.write(frame.endsWith("\n\n") ? frame : `${frame}\n\n`);
  }
  res.end();
}

before(async () => {
  server = createServer((req, res) => {
    let raw = "";
    req.on("data", (chunk) => {
      raw += chunk;
    });
    req.on("end", () => {
      const body = raw ? JSON.parse(raw) : {};

      if (req.method === "GET" && req.url === "/healthz") {
        sendJson(res, 200, { status: "ok" });
        return;
      }
      if (req.method === "GET" && req.url === "/v1/models") {
        sendJson(res, 200, {
          object: "list",
          data: [{ id: "chat/mock", task: "chat", engines: ["vllm"], streaming: true }],
        });
        return;
      }
      if (req.method === "POST" && req.url === "/v1/chat/completions") {
        if (body.stream) {
          sendSse(res, [
            `data: ${JSON.stringify({ choices: [{ delta: { content: "he" } }] })}`,
            `data: ${JSON.stringify({ choices: [{ delta: { content: "llo" } }] })}`,
            `data: ${JSON.stringify({ choices: [], usage: { prompt_tokens: 2, completion_tokens: 2 } })}`,
          ]);
          return;
        }
        sendJson(res, 200, {
          id: "chatcmpl-1",
          model: body.model,
          choices: [{ message: { role: "assistant", content: "hello" }, finish_reason: "stop" }],
          engine: "vllm/mock",
          usage: { prompt_tokens: 2, completion_tokens: 1, total_tokens: 3 },
        });
        return;
      }
      if (req.method === "POST" && req.url === "/v1/responses") {
        if (body.stream) {
          sendSse(res, [
            `event: response.created\ndata: ${JSON.stringify({ type: "response.created" })}`,
            `event: response.output_text.delta\ndata: ${JSON.stringify({ type: "response.output_text.delta", delta: "hi" })}`,
            `event: response.completed\ndata: ${JSON.stringify({ type: "response.completed" })}`,
          ]);
          return;
        }
        sendJson(res, 200, {
          id: "resp_1",
          object: "response",
          model: body.model,
          status: "completed",
          output_text: "hello",
        });
        return;
      }
      if (req.method === "POST" && req.url === "/v1/audio/speech") {
        const pcm = Buffer.alloc(128, 7);
        res.writeHead(200, { "content-type": "audio/pcm", "content-length": String(pcm.length) });
        res.end(pcm);
        return;
      }
      if (req.method === "POST" && req.url === "/v1/decide") {
        sendJson(res, 200, {
          model: body.model,
          output: '{"action":"move"}',
          engine: "vllm/dm",
        });
        return;
      }
      if (req.method === "POST" && req.url === "/v1/embeddings") {
        sendJson(res, 200, { data: [{ embedding: [0.1, 0.2] }] });
        return;
      }
      sendJson(res, 404, { error: { type: "not_found", message: "unknown path" } });
    });
  });

  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  baseUrl = `http://127.0.0.1:${server.address().port}`;
});

after(async () => {
  await new Promise((resolve) => server.close(resolve));
});

test("health and models", async () => {
  const client = new Client({ baseUrl });
  assert.equal((await client.health()).status, "ok");
  const models = await client.models();
  assert.equal(models[0].id, "chat/mock");
});

test("chat non stream", async () => {
  const client = new Client({ baseUrl });
  const response = await client.chat({
    model: "chat/mock",
    messages: [ChatMessage.user("hi")],
    system: "be brief",
  });
  assert.equal(response.choices[0].message.content, "hello");
  assert.equal(response.usage.total_tokens, 3);
});

test("chat stream yields deltas", async () => {
  const client = new Client({ baseUrl });
  const chunks = [];
  for await (const delta of client.chatStream({ model: "chat/mock", messages: "hi" })) {
    chunks.push(delta);
  }
  assert.equal(chunks.join(""), "hello");
});

test("responses and responses stream", async () => {
  const client = new Client({ baseUrl });

  const response = await client.responses({
    model: "chat/mock",
    input: "hi",
    instructions: "be brief",
  });
  assert.equal(response.output_text, "hello");

  const events = [];
  for await (const event of client.responsesStream({ model: "chat/mock", input: "hi" })) {
    events.push(event);
  }
  assert.equal(events[0].type, "response.created");
  assert.equal(events[1].delta, "hi");
  assert.equal(events.at(-1).type, "response.completed");
});

test("embeddings, tts and decide", async () => {
  const client = new Client({ baseUrl });

  assert.ok((await client.embeddings({ model: "chat/mock", input: "hi" })).data);

  const pcm = await client.tts({ model: "tts/mock", text: "hi" });
  assert.equal(pcm.byteLength, 128);

  const decision = await client.decide({ model: "dm/laya", state: { hp: 10 } });
  assert.match(decision.output, /action/);
});

test("api errors carry status and payload", async () => {
  const client = new Client({ baseUrl });
  await assert.rejects(
    () => client.request("GET", "/v1/nope"),
    (error) => error instanceof StudioError && error.status === 404,
  );
});