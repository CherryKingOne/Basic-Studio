package ai.basicstudio;

import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.function.Consumer;

/**
 * Basic-Studio 网关 Java 客户端。
 *
 * <pre>{@code
 * var client = BasicStudioClient.builder().build();          // http://127.0.0.1:18596
 * var reply = client.chat(new ChatRequest("chat/mock", "你好"));
 * client.chatStream(new ChatRequest("chat/mock", "讲个故事").stream(true), System.out::print);
 * }</pre>
 *
 * 线程安全：内部 {@link HttpClient} 可并发复用。
 */
public final class BasicStudioClient implements AutoCloseable {

    public static final String DEFAULT_BASE_URL = "http://127.0.0.1:18596";

    private final String baseUrl;
    private final String apiKey;
    private final HttpClient http;
    private final ObjectMapper mapper = new ObjectMapper();

    public BasicStudioClient() {
        this(DEFAULT_BASE_URL, null);
    }

    public BasicStudioClient(String baseUrl, String apiKey) {
        this.baseUrl = stripTrailingSlash(baseUrl == null || baseUrl.isEmpty() ? DEFAULT_BASE_URL : baseUrl);
        this.apiKey = apiKey;
        this.http = HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(10))
                .build();
    }

    public static Builder builder() {
        return new Builder();
    }

    private static String stripTrailingSlash(String value) {
        return value.endsWith("/") ? value.substring(0, value.length() - 1) : value;
    }

    // ------------------------------------------------------------------ 基础接口

    public JsonNode health() {
        return get("/healthz");
    }

    public JsonNode ready() {
        return get("/readyz");
    }

    public List<ModelInfo> models() {
        JsonNode payload = get("/v1/models");
        List<ModelInfo> models = new ArrayList<>();
        for (JsonNode node : payload.path("data")) {
            models.add(new ModelInfo(
                    node.path("id").asText(),
                    node.path("task").asText(),
                    textList(node.path("engines")),
                    node.path("adapter").asText(null),
                    node.path("api_version").asText(null),
                    node.path("streaming").asBoolean(false)));
        }
        return models;
    }

    // ------------------------------------------------------------------ 对话

    public ChatResponse chat(ChatRequest request) {
        JsonNode payload = post("/v1/chat/completions", request.toMap());
        try {
            return mapper.treeToValue(payload, ChatResponse.class);
        } catch (IOException | IllegalArgumentException exception) {
            throw new StudioException("cannot decode chat response: " + exception.getMessage());
        }
    }

    /** 流式对话：同步逐块回调，返回累计文本。 */
    public String chatStream(ChatRequest request, Consumer<String> onDelta) {
        request.stream(true);
        StringBuilder builder = new StringBuilder();

        stream("/v1/chat/completions", request.toMap(), line -> {
            JsonNode frame = parse(line);
            if (frame == null || !frame.has("choices") || frame.path("choices").isEmpty()) {
                return;
            }
            JsonNode delta = frame.path("choices").get(0).path("delta").path("content");
            if (!delta.isMissingNode() && !delta.isNull()) {
                String text = delta.asText();
                builder.append(text);
                onDelta.accept(text);
            }
        });

        return builder.toString();
    }

    // ------------------------------------------------------------------ Responses

    public ResponsesResponse responses(ResponsesRequest request) {
        JsonNode payload = post("/v1/responses", request.toMap());
        return new ResponsesResponse(
                payload.path("id").asText(),
                payload.path("model").asText(),
                payload.path("status").asText("completed"),
                payload.path("output_text").asText(""),
                payload.path("created_at").asInt(0));
    }

    /** Responses 流式：按类型化事件回调。 */
    public void responsesStream(ResponsesRequest request, Consumer<JsonNode> onEvent) {
        request.stream(true);
        stream("/v1/responses", request.toMap(), line -> {
            JsonNode frame = parse(line);
            if (frame != null) {
                onEvent.accept(frame);
            }
        });
    }

    // ------------------------------------------------------------------ 其它任务

    public JsonNode embeddings(String model, String input) {
        return post("/v1/embeddings", java.util.Map.of("model", model, "input", input));
    }

    /** 语音合成，返回 PCM（16bit little-endian）字节。 */
    public byte[] tts(SpeechRequest request) {
        try {
            HttpRequest httpRequest = requestBuilder("/v1/audio/speech")
                    .header("accept", "audio/pcm")
                    .POST(HttpRequest.BodyPublishers.ofString(mapper.writeValueAsString(request.toMap())))
                    .build();

            HttpResponse<byte[]> response = http.send(httpRequest, HttpResponse.BodyHandlers.ofByteArray());
            ensureSuccess(response.statusCode(), new String(response.body()));
            return response.body();
        } catch (IOException exception) {
            throw new StudioException("tts request failed: " + exception.getMessage());
        } catch (InterruptedException exception) {
            Thread.currentThread().interrupt();
            throw new StudioException("tts request interrupted");
        }
    }

    public DecideResponse decide(DecideRequest request) {
        JsonNode payload = post("/v1/decide", request.toMap());
        try {
            return mapper.treeToValue(payload, DecideResponse.class);
        } catch (IOException | IllegalArgumentException exception) {
            throw new StudioException("cannot decode decision response: " + exception.getMessage());
        }
    }

    // ------------------------------------------------------------------ 内部实现

    private JsonNode get(String path) {
        try {
            HttpRequest httpRequest = requestBuilder(path).GET().build();
            HttpResponse<String> response = http.send(httpRequest, HttpResponse.BodyHandlers.ofString());
            ensureSuccess(response.statusCode(), response.body());
            return mapper.readTree(response.body());
        } catch (IOException exception) {
            throw new StudioException("GET " + path + " failed: " + exception.getMessage());
        } catch (InterruptedException exception) {
            Thread.currentThread().interrupt();
            throw new StudioException("GET " + path + " interrupted");
        }
    }

    private JsonNode post(String path, Object body) {
        try {
            HttpRequest httpRequest = requestBuilder(path)
                    .POST(HttpRequest.BodyPublishers.ofString(mapper.writeValueAsString(body)))
                    .build();

            HttpResponse<String> response = http.send(httpRequest, HttpResponse.BodyHandlers.ofString());
            ensureSuccess(response.statusCode(), response.body());
            return mapper.readTree(response.body());
        } catch (IOException exception) {
            throw new StudioException("POST " + path + " failed: " + exception.getMessage());
        } catch (InterruptedException exception) {
            Thread.currentThread().interrupt();
            throw new StudioException("POST " + path + " interrupted");
        }
    }

    private HttpRequest.Builder requestBuilder(String path) {
        HttpRequest.Builder builder = HttpRequest.newBuilder()
                .uri(URI.create(baseUrl + path))
                .timeout(Duration.ofSeconds(60))
                .header("content-type", "application/json");

        if (apiKey != null && !apiKey.isEmpty()) {
            builder.header("authorization", "Bearer " + apiKey);
        }
        return builder;
    }

    /** 逐行读取 SSE，遇到空行忽略；每个 `data:` 行回调一次。 */
    private void stream(String path, Object body, Consumer<String> onData) {
        try {
            HttpRequest httpRequest = requestBuilder(path)
                    .header("accept", "text/event-stream")
                    .POST(HttpRequest.BodyPublishers.ofString(mapper.writeValueAsString(body)))
                    .build();

            HttpResponse<java.io.InputStream> response =
                    http.send(httpRequest, HttpResponse.BodyHandlers.ofInputStream());

            ensureSuccess(response.statusCode(), "streaming request failed");

            try (java.io.InputStream input = response.body();
                 java.io.BufferedReader reader = new java.io.BufferedReader(
                         new java.io.InputStreamReader(input, java.nio.charset.StandardCharsets.UTF_8))) {
                String line;
                while ((line = reader.readLine()) != null) {
                    String trimmed = line.trim();
                    if (!trimmed.startsWith("data:")) {
                        continue;
                    }
                    String payload = trimmed.substring("data:".length()).trim();
                    if (payload.isEmpty() || "[DONE]".equals(payload)) {
                        continue;
                    }
                    onData.accept(payload);
                }
            }
        } catch (IOException exception) {
            throw new StudioException("stream " + path + " failed: " + exception.getMessage());
        } catch (InterruptedException exception) {
            Thread.currentThread().interrupt();
            throw new StudioException("stream " + path + " interrupted");
        }
    }

    private JsonNode parse(String line) {
        try {
            return mapper.readTree(line);
        } catch (IOException exception) {
            return null;
        }
    }

    private void ensureSuccess(int status, String body) {
        if (status >= 200 && status < 300) {
            return;
        }

        String message = body == null ? "" : body;
        try {
            JsonNode payload = mapper.readTree(message);
            JsonNode errorMessage = payload.path("error").path("message");
            if (!errorMessage.isMissingNode() && !errorMessage.isNull()) {
                message = errorMessage.asText();
            }
        } catch (IOException ignored) {
            // 非 JSON 错误体，保留原文
        }
        throw new StudioException("http " + status + ": " + message, status, body);
    }

    private static List<String> textList(JsonNode node) {
        List<String> values = new ArrayList<>();
        node.forEach(item -> values.add(item.asText()));
        return values;
    }

    @Override
    public void close() {
        // HttpClient 在 JDK 21+ 需显式关闭；低版本由 GC 回收
        try {
            var method = HttpClient.class.getMethod("close");
            method.invoke(http);
        } catch (ReflectiveOperationException ignored) {
            // 低版本 JDK 无 close()，忽略
        }
    }

    /** 便捷构造器。 */
    public static final class Builder {
        private String baseUrl = DEFAULT_BASE_URL;
        private String apiKey;

        public Builder baseUrl(String baseUrl) {
            this.baseUrl = baseUrl;
            return this;
        }

        public Builder apiKey(String apiKey) {
            this.apiKey = apiKey;
            return this;
        }

        public BasicStudioClient build() {
            return new BasicStudioClient(baseUrl, apiKey);
        }
    }

    /** 供测试读取的 JSON 解析器（避免重复构造）。 */
    ObjectMapper mapper() {
        return mapper;
    }

    /** 仅供类型引用，避免未使用告警。 */
    @SuppressWarnings("unused")
    private static final TypeReference<List<ModelInfo>> MODEL_LIST = new TypeReference<>() {};
}