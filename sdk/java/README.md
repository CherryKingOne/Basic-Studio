# Basic-Studio Java SDK

Java 17+，使用 `java.net.http.HttpClient` + Jackson。

## 安装

```bash
cd sdk/java
mvn install
```

```xml
<dependency>
  <groupId>ai.basicstudio</groupId>
  <artifactId>basic-studio-sdk</artifactId>
  <version>0.1.0</version>
</dependency>
```

## 快速开始

```java
import ai.basicstudio.*;

import java.util.List;

public class Demo {
    public static void main(String[] args) {
        try (var client = BasicStudioClient.builder()
                .baseUrl("http://127.0.0.1:18596")
                .apiKey("sk-...")          // 无鉴权时可省略
                .build()) {

            ChatResponse reply = client.chat(new ChatRequest("chat/mock", "你好"));
            System.out.println(reply.content());

            String full = client.chatStream(
                    new ChatRequest("chat/mock", "讲个故事").stream(true),
                    System.out::print);
            System.out.println("\n累计长度: " + full.length());

            ResponsesResponse response =
                    client.responses(new ResponsesRequest("chat/mock", "写个接口设计"));
            System.out.println(response.text());

            byte[] pcm = client.tts(new SpeechRequest("tts/echomock", "你好世界"));
            java.nio.file.Files.write(java.nio.file.Path.of("out.pcm"), pcm);

            List<ModelInfo> models = client.models();
            models.forEach(model -> System.out.println(model.id() + " -> " + model.engines()));
        }
    }
}
```

## 测试

```bash
cd sdk/java && mvn test
```
