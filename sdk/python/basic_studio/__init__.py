"""Basic-Studio Python SDK.

仅依赖标准库：

```python
from basic_studio import Client

client = Client()                       # 默认 http://127.0.0.1:18596
print(client.chat("chat/mock", "你好"))
for delta in client.chat_stream("chat/mock", "讲个故事"):
    print(delta, end="", flush=True)
```
"""

from .client import Client, ClientError, StreamEvent
from .types import ChatMessage, ChatResponse, DecideResponse, ModelInfo, ResponsesResponse

__all__ = [
    "Client",
    "ClientError",
    "StreamEvent",
    "ChatMessage",
    "ChatResponse",
    "DecideResponse",
    "ModelInfo",
    "ResponsesResponse",
]

__version__ = "0.0.1"