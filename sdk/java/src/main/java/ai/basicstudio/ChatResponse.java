package ai.basicstudio;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import java.util.List;

/** `/v1/chat/completions` 响应。 */
@JsonIgnoreProperties(ignoreUnknown = true)
public record ChatResponse(
        String id,
        String model,
        List<Choice> choices,
        String engine,
        Usage usage) {

    /** 首个候选的文本内容。 */
    public String content() {
        if (choices == null || choices.isEmpty()) {
            return "";
        }
        Message message = choices.get(0).message();
        if (message == null || message.content() == null) {
            return "";
        }
        return String.valueOf(message.content());
    }

    @JsonIgnoreProperties(ignoreUnknown = true)
    public record Choice(int index, Message message, String finishReason) {}

    @JsonIgnoreProperties(ignoreUnknown = true)
    public record Message(String role, Object content) {}

    @JsonIgnoreProperties(ignoreUnknown = true)
    public record Usage(int prompt_tokens, int completion_tokens, int total_tokens) {}
}