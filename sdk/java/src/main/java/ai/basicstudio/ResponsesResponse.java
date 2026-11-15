package ai.basicstudio;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;

/** `/v1/responses` 响应。 */
@JsonIgnoreProperties(ignoreUnknown = true)
public record ResponsesResponse(
        String id,
        String model,
        String status,
        String output_text,
        int created_at) {

    public String text() {
        return output_text == null ? "" : output_text;
    }
}