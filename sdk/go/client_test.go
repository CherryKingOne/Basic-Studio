package basicstudio

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func newTestServer(t *testing.T) *httptest.Server {
	t.Helper()

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body map[string]any
		if r.Body != nil {
			_ = json.NewDecoder(r.Body).Decode(&body)
		}

		switch {
		case r.Method == http.MethodGet && r.URL.Path == "/healthz":
			writeJSON(w, http.StatusOK, map[string]any{"status": "ok"})
		case r.Method == http.MethodGet && r.URL.Path == "/v1/models":
			writeJSON(w, http.StatusOK, map[string]any{
				"data": []map[string]any{{"id": "chat/mock", "task": "chat", "engines": []string{"vllm"}}},
			})
		case r.Method == http.MethodPost && r.URL.Path == "/v1/chat/completions":
			if stream, _ := body["stream"].(bool); stream {
				w.Header().Set("Content-Type", "text/event-stream")
				w.WriteHeader(http.StatusOK)
				flusher := w.(http.Flusher)
				for _, frame := range []string{
					`data: {"choices":[{"delta":{"content":"he"}}]}`,
					`data: {"choices":[{"delta":{"content":"llo"}}]}`,
					`data: {"choices":[],"usage":{"prompt_tokens":2,"completion_tokens":2}}`,
				} {
					_, _ = w.Write([]byte(frame + "\n\n"))
					flusher.Flush()
				}
				return
			}
			writeJSON(w, http.StatusOK, map[string]any{
				"id":      "chatcmpl-1",
				"model":   body["model"],
				"choices": []map[string]any{{"message": map[string]any{"role": "assistant", "content": "hello"}}},
				"usage":   map[string]any{"prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3},
			})
		case r.Method == http.MethodPost && r.URL.Path == "/v1/audio/speech":
			w.Header().Set("Content-Type", "audio/pcm")
			_, _ = w.Write(make([]byte, 128))
		case r.Method == http.MethodPost && r.URL.Path == "/v1/decide":
			writeJSON(w, http.StatusOK, map[string]any{
				"model": body["model"], "output": `{"action":"move"}`, "engine": "vllm/dm",
			})
		case r.Method == http.MethodPost && r.URL.Path == "/v1/responses":
			writeJSON(w, http.StatusOK, map[string]any{
				"id": "resp_1", "model": body["model"], "status": "completed", "output_text": "hello",
			})
		default:
			writeJSON(w, http.StatusNotFound, map[string]any{
				"error": map[string]any{"type": "not_found", "message": "unknown path"},
			})
		}
	})

	server := httptest.NewServer(handler)
	t.Cleanup(server.Close)
	return server
}

func writeJSON(w http.ResponseWriter, status int, payload any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(payload)
}

func TestHealthAndModels(t *testing.T) {
	server := newTestServer(t)
	client := New(server.URL)

	health, err := client.Health(context.Background())
	if err != nil {
		t.Fatalf("health: %v", err)
	}
	if health["status"] != "ok" {
		t.Fatalf("unexpected health payload: %#v", health)
	}

	models, err := client.Models(context.Background())
	if err != nil {
		t.Fatalf("models: %v", err)
	}
	if len(models) != 1 || models[0].ID != "chat/mock" {
		t.Fatalf("unexpected models: %#v", models)
	}
}

func TestChat(t *testing.T) {
	client := New(newTestServer(t).URL)

	response, err := client.Chat(context.Background(), ChatRequest{
		Model:    "chat/mock",
		Messages: []Message{User("hi")},
		System:   "be brief",
	})
	if err != nil {
		t.Fatalf("chat: %v", err)
	}
	if response.Content() != "hello" {
		t.Fatalf("unexpected content: %q", response.Content())
	}
	if response.Usage.TotalTokens != 3 {
		t.Fatalf("unexpected usage: %#v", response.Usage)
	}
}

func TestChatStream(t *testing.T) {
	client := New(newTestServer(t).URL)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	chunks, errs, closeStream := client.ChatStream(ctx, ChatRequest{
		Model:    "chat/mock",
		Messages: []Message{User("hi")},
	})
	defer closeStream()

	var builder strings.Builder
	for chunk := range chunks {
		builder.WriteString(chunk)
	}
	select {
	case err := <-errs:
		if err != nil {
			t.Fatalf("stream error: %v", err)
		}
	default:
	}

	if builder.String() != "hello" {
		t.Fatalf("unexpected stream text: %q", builder.String())
	}
}

func TestTTSAndDecide(t *testing.T) {
	client := New(newTestServer(t).URL)
	ctx := context.Background()

	pcm, err := client.TTS(ctx, SpeechRequest{Model: "tts/mock", Text: "hi"})
	if err != nil {
		t.Fatalf("tts: %v", err)
	}
	if len(pcm) != 128 {
		t.Fatalf("unexpected pcm length: %d", len(pcm))
	}

	decision, err := client.Decide(ctx, DecideRequest{Model: "dm/laya", State: map[string]any{"hp": 10}})
	if err != nil {
        t.Fatalf("decide: %v", err)
	}
	if !strings.Contains(decision.Output, "action") {
		t.Fatalf("unexpected decision: %#v", decision)
	}
}

func TestErrorCarriesStatus(t *testing.T) {
	client := New(newTestServer(t).URL)

	_, err := client.Models(context.Background())
	if err != nil {
		t.Fatalf("models: %v", err)
	}

	if _, err := client.Health(context.Background()); err != nil {
		t.Fatalf("health: %v", err)
	}

	var payload map[string]any
	err = client.do(context.Background(), http.MethodGet, "/v1/nope", nil, "application/json", &payload)
	apiErr, ok := err.(*APIError)
	if !ok {
		t.Fatalf("expected *APIError, got %T", err)
	}
	if apiErr.StatusCode != http.StatusNotFound {
		t.Fatalf("unexpected status: %d", apiErr.StatusCode)
	}
	if !IsRetryable(&APIError{StatusCode: http.StatusBadGateway}) {
		t.Fatal("502 should be retryable")
	}
}