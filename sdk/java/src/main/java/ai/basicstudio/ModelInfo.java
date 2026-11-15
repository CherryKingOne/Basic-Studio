package ai.basicstudio;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import java.util.List;

/** `/v1/models` 中的单个模型。 */
@JsonIgnoreProperties(ignoreUnknown = true)
public record ModelInfo(
        String id,
        String task,
        List<String> engines,
        String adapter,
        String apiVersion,
        boolean streaming) {}