package ai.basicstudio;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Chat Completions 请求。 */
public final class ChatRequest {

    private String model;
    private List<Message> messages = new ArrayList<>();
    private String system;
    private boolean stream = false;
    private Double temperature;
    private Double topP;
    private Integer maxTokens;

    public ChatRequest() {}

    public ChatRequest(String model, String prompt) {
        this.model = model;
        this.messages.add(Message.user(prompt));
    }

    public ChatRequest model(String model) {
        this.model = model;
        return this;
    }

    public ChatRequest messages(List<Message> messages) {
        this.messages = messages;
        return this;
    }

    public ChatRequest system(String system) {
        this.system = system;
        return this;
    }

    public ChatRequest stream(boolean stream) {
        this.stream = stream;
        return this;
    }

    public ChatRequest temperature(double temperature) {
        this.temperature = temperature;
        return this;
    }

    public ChatRequest topP(double topP) {
        this.topP = topP;
        return this;
    }

    public ChatRequest maxTokens(int maxTokens) {
        this.maxTokens = maxTokens;
        return this;
    }

    /** 序列化为网关协议请求体。 */
    public Map<String, Object> toMap() {
        List<Map<String, Object>> payload = new ArrayList<>();
        if (system != null && !system.isEmpty()) {
            payload.add(messageMap("system", system));
        }
        for (Message message : messages) {
            payload.add(messageMap(message.role(), message.content()));
        }

        Map<String, Object> body = new LinkedHashMap<>();
        body.put("model", model);
        body.put("messages", payload);
        body.put("stream", stream);
        if (temperature != null) {
            body.put("temperature", temperature);
        }
        if (topP != null) {
            body.put("top_p", topP);
        }
        if (maxTokens != null) {
            body.put("max_tokens", maxTokens);
        }
        return body;
    }

    private static Map<String, Object> messageMap(String role, Object content) {
        Map<String, Object> message = new LinkedHashMap<>();
        message.put("role", role);
        message.put("content", content);
        return message;
    }

    /** 单条对话消息。 */
    public record Message(String role, Object content) {
        public static Message system(Object content) {
            return new Message("system", content);
        }

        public static Message user(Object content) {
            return new Message("user", content);
        }

        public static Message assistant(Object content) {
            return new Message("assistant", content);
        }
    }
}