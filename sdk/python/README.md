# Basic-Studio Python SDK

零第三方依赖（标准库实现），支持普通请求与 SSE 流式。

## 安装

```bash
pip install ./sdk/python
```

## 快速开始

```python
from basic_studio import Client, ChatMessage

client = Client()                       # 默认 http://127.0.0.1:18596
print(client.health())

reply = client.chat("tts/index-tts2.5", [ChatMessage.user("你好")])
print(reply.content)

for delta in client.chat_stream("chat/mock", "讲个故事"):
    print(delta, end="", flush=True)

resp = client.responses("chat/mock", "写个接口设计", instructions="用中文，简洁")
print(resp.text)

pcm = client.tts("tts/echomock", "你好世界")     # 16bit little-endian PCM
with open("out.pcm", "wb") as fh:
    fh.write(pcm)

print(client.decide("dm/laya", {"hp": 10}).output)
```

## 测试

```bash
cd sdk/python && python3 -m unittest discover -s tests
```
