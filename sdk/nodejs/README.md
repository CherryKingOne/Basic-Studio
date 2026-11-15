# Basic-Studio Node.js SDK

零依赖 ESM 实现，Node >= 18（使用内置 `fetch`）。

## 安装

```bash
npm install ./sdk/nodejs
```

## 快速开始

```js
import { Client, ChatMessage } from "@basic-studio/sdk";

const client = new Client();                       // http://127.0.0.1:18596
console.log(await client.health());

const reply = await client.chat({
  model: "chat/mock",
  messages: [ChatMessage.user("你好")],
  system: "be brief",
});
console.log(reply.choices[0].message.content);

for await (const delta of client.chatStream({ model: "chat/mock", messages: "讲个故事" })) {
  process.stdout.write(delta);
}

const response = await client.responses({ model: "chat/mock", input: "写个接口设计" });
console.log(response.output_text);

const pcm = await client.tts({ model: "tts/echomock", text: "你好世界" });
await import("node:fs/promises").then((fs) => fs.writeFile("out.pcm", pcm));

console.log(await client.decide({ model: "dm/laya", state: { hp: 10 } }));
```

## 测试

```bash
cd sdk/nodejs && npm test
```
