package ai.basicstudio;

import java.util.LinkedHashMap;
import java.util.Map;

/** 决策模型请求。 */
public final class DecideRequest {

    private final String model;
    private final Object state;
    private String context;

    public DecideRequest(String model, Object state) {
        this.model = model;
        this.state = state;
    }

    public DecideRequest context(String context) {
        this.context = context;
        return this;
    }

    public Map<String, Object> toMap() {
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("model", model);
        body.put("state", state);
        if (context != null) {
            body.put("context", context);
        }
        return body;
    }
}