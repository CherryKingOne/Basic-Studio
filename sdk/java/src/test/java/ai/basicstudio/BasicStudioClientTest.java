package ai.basicstudio;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class BasicStudioClientTest {

    private HttpServer server;
    private String baseUrl;

    @BeforeEach
    void startServer() throws IOException {
        server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/", this::handle);
        server.start();
        baseUrl = "http://127.0.0.1:" + server.getAddress().getPort();
    }

    @AfterEach
    void stopServer() {
        server.stop(0);
    }

    private void handle(HttpExchange exchange) throws IOException {
        String path = exchange.getRequestURI().getPath();
        String method = exchange.getRequestMethod();
        String body = new String(exchange.getRequestBody().readAllBytes(), StandardCharsets.UTF_8);

        switch (method + " " + path) {
            case "GET /healthz" -> respond(exchange, 200, "{\"status\":\"ok\"}");
            case "GET /readyz" -> respond(exchange, 200, "{\"ready\":true}");
            case "GET /v1/models" -> respond(exchange, 200,
                    "{\"data\":[{\"id\":\"chat/mock\",\"task\":\"chat\",\"engines\":[\"vllm\"],\"streaming\":true}]}");
            case "POST /v1/chat/completions" -> {
                if (body.contains("\"stream\":true")) {
                    stream(exchange, List.of(
                            "data: {\"choices\":[{\"delta\":{\"content\":\"he\"}}]}",
                            "data: {\"choices\":[{\"delta\":{\"content\":\"llo\"}}]}"));
                } else {
                    respond(exchange, 200,
                            "{\"id\":\"chatcmpl-1\",\"model\":\"chat/mock\","
                                    + "\"choices\":[{\"message\":{\"role\":\"assistant\",\"content\":\"hello\"}}],"
                                    + "\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}");
                }
            }
            case "POST /v1/responses" -> respond(exchange, 200,
                    "{\"id\":\"resp_1\",\"model\":\"chat/mock\",\"status\":\"completed\",\"output_text\":\"hello\"}");
            case "POST /v1/audio/speech" -> respond(exchange, 200, new byte[128]);
            case "POST /v1/decide" -> respond(exchange, 200,
                    "{\"model\":\"dm/laya\",\"output\":\"{\\\"action\\\":\\\"move\\\"}\",\"engine\":\"vllm/dm\"}");
            default -> respond(exchange, 404, "{\"error\":{\"type\":\"not_found\",\"message\":\"unknown path\"}}");
        }
    }

    private void respond(HttpExchange exchange, int status, String json) throws IOException {
        byte[] payload = json.getBytes(StandardCharsets.UTF_8);
        exchange.getResponseHeaders().set("content-type", "application/json");
        exchange.sendResponseHeaders(status, payload.length);
        try (OutputStream output = exchange.getResponseBody()) {
            output.write(payload);
        }
    }

    private void respond(HttpExchange exchange, int status, byte[] payload) throws IOException {
        exchange.getResponseHeaders().set("content-type", "audio/pcm");
        exchange.sendResponseHeaders(status, payload.length);
        try (OutputStream output = exchange.getResponseBody()) {
            output.write(payload);
        }
    }

    private void stream(HttpExchange exchange, List<String> frames) throws IOException {
        exchange.getResponseHeaders().set("content-type", "text/event-stream");
        exchange.sendResponseHeaders(200, 0);
        try (OutputStream output = exchange.getResponseBody()) {
            for (String frame : frames) {
                output.write((frame + "\n\n").getBytes(StandardCharsets.UTF_8));
                output.flush();
            }
        }
    }

    @Test
    void healthAndModels() {
        try (var client = new BasicStudioClient(baseUrl, null)) {
            assertEquals("ok", client.health().path("status").asText());

            List<ModelInfo> models = client.models();
            assertEquals(1, models.size());
            assertEquals("chat/mock", models.get(0).id());
        }
    }

    @Test
    void chatReturnsContent() {
        try (var client = new BasicStudioClient(baseUrl, null)) {
            ChatResponse response = client.chat(new ChatRequest("chat/mock", "hi").system("be brief"));
            assertEquals("hello", response.content());
            assertEquals(3, response.usage().total_tokens());
        }
    }

    @Test
    void chatStreamYieldsDeltas() {
        try (var client = new BasicStudioClient(baseUrl, null)) {
            List<String> deltas = new ArrayList<>();
            String full = client.chatStream(new ChatRequest("chat/mock", "hi").stream(true), deltas::add);

            assertEquals("hello", full);
            assertEquals(List.of("he", "llo"), deltas);
        }
    }

    @Test
    void responsesAndTtsAndDecide() {
        try (var client = new BasicStudioClient(baseUrl, null)) {
            ResponsesResponse responses =
                    client.responses(new ResponsesRequest("chat/mock", "hi").instructions("be brief"));
            assertEquals("hello", responses.text());

            byte[] pcm = client.tts(new SpeechRequest("tts/mock", "hi"));
            assertEquals(128, pcm.length);

            DecideResponse decision = client.decide(new DecideRequest("dm/laya", Map.of("hp", 10)));
            assertTrue(decision.output().contains("action"));
        }
    }

    @Test
    void errorCarriesStatus() {
        try (var client = new BasicStudioClient(baseUrl, null)) {
            StudioException exception = assertThrows(
                    StudioException.class,
                    () -> client.embeddings("chat/mock", "hi"));
            assertEquals(404, exception.status());
            assertTrue(exception.getMessage().contains("unknown path"));
        }
    }

    @Test
    void builderAppliesBaseUrlAndKey() {
        try (var client = BasicStudioClient.builder()
                .baseUrl(baseUrl + "/")
                .apiKey("secret")
                .build()) {
            assertNotNull(client);
        }
    }
}