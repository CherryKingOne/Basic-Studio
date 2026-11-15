package ai.basicstudio;

import java.util.LinkedHashMap;
import java.util.Map;

/** `/v1/responses` 请求。 */
public final class ResponsesRequest {

    private final String model;
    private final Object input;
    private String instructions;
    private boolean stream = false;
    private Double temperature;
    private Integer maxOutputTokens;
    private String previousResponseId;

    public ResponsesRequest(String model, Object input) {
        this.model = model;
        this.input = input;
    }

    public ResponsesRequest instructions(String instructions) {
        this.instructions = instructions;
        return this;
    }

    public ResponsesRequest stream(boolean stream) {
        this.stream = stream;
        return this;
    }

    public ResponsesRequest temperature(double temperature) {
        this.temperature = temperature;
        return this;
    }

    public ResponsesRequest maxOutputTokens(int maxOutputTokens) {
        this.maxOutputTokens = maxOutputTokens;
        return this;
    }

    public ResponsesRequest previousResponseId(String previousResponseId) {
        this.previousResponseId = previousResponseId;
        return this;
    }

    public Map<String, Object> toMap() {
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("model", model);
        body.put("input", input);
        body.put("stream", stream);
        if (instructions != null) {
            body.put("instructions", instructions);
        }
        if (temperature != null) {
            body.put("temperature", temperature);
        }
        if (maxOutputTokens != null) {
            body.put("max_output_tokens", maxOutputTokens);
        }
        if (previousResponseId != null) {
            body.put("previous_response_id", previousResponseId);
        }
        return body;
    }
}