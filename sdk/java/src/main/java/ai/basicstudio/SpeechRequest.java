package ai.basicstudio;

import com.fasterxml.jackson.annotation.JsonInclude;
import java.util.LinkedHashMap;
import java.util.Map;

/** 语音合成请求。 */
@JsonInclude(JsonInclude.Include.NON_NULL)
public final class SpeechRequest {

    private final String model;
    private final String text;
    private String voice;
    private String emotion;

    public SpeechRequest(String model, String text) {
        this.model = model;
        this.text = text;
    }

    public SpeechRequest voice(String voice) {
        this.voice = voice;
        return this;
    }

    public SpeechRequest emotion(String emotion) {
        this.emotion = emotion;
        return this;
    }

    public Map<String, Object> toMap() {
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("model", model);
        body.put("input", text);
        body.put("stream", false);
        if (voice != null) {
            body.put("voice", voice);
        }
        if (emotion != null) {
            body.put("emotion", emotion);
        }
        return body;
    }
}