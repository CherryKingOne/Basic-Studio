package ai.basicstudio;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;

/** `/v1/decide` 响应。 */
@JsonIgnoreProperties(ignoreUnknown = true)
public record DecideResponse(String model, String output, String engine) {}