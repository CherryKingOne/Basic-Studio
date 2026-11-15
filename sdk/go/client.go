// Package basicstudio 是 Basic-Studio 网关的 Go SDK。
//
//	client := basicstudio.New("http://127.0.0.1:18596")
//	reply, err := client.Chat(ctx, basicstudio.ChatRequest{
//		Model:    "chat/mock",
//		Messages: []basicstudio.Message{basicstudio.User("你好")},
//	})
package basicstudio

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

// DefaultBaseURL 是网关默认监听地址。
const DefaultBaseURL = "http://127.0.0.1:18596"

// Client 是网关客户端，可并发使用。
type Client struct {
	BaseURL string
	APIKey  string
	HTTP    *http.Client
}

// New 创建客户端；baseURL 为空时使用默认地址。
func New(baseURL string) *Client {
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Client{
		BaseURL: strings.TrimRight(baseURL, "/"),
		HTTP:    &http.Client{Timeout: 60 * time.Second},
	}
}

// WithAPIKey 设置 Bearer Token（网关启用 [gateway].api_keys 时必填）。
func (c *Client) WithAPIKey(key string) *Client {
	c.APIKey = key
	return c
}

// WithHTTPClient 使用自定义 http.Client（例如自定义超时、代理）。
func (c *Client) WithHTTPClient(httpClient *http.Client) *Client {
	c.HTTP = httpClient
	return c
}

// APIError 表示服务端返回的非 2xx。
type APIError struct {
	StatusCode int
	Message    string
	Body       []byte
}

func (e *APIError) Error() string {
	return fmt.Sprintf("basic-studio: http %d: %s", e.StatusCode, e.Message)
}

// IsRetryable 判断错误是否值得重试。
func IsRetryable(err error) bool {
	if err == nil {
		return false
	}
	if apiErr, ok := err.(*APIError); ok {
		return apiErr.StatusCode == http.StatusTooManyRequests || apiErr.StatusCode >= 500
	}
	return true
}

func (c *Client) do(ctx context.Context, method, path string, body any, accept string, out any) error {
	var reader io.Reader
	if body != nil {
		encoded, err := json.Marshal(body)
		if err != nil {
			return fmt.Errorf("encode request: %w", err)
		}
		reader = bytes.NewReader(encoded)
	}

	request, err := http.NewRequestWithContext(ctx, method, c.BaseURL+path, reader)
	if err != nil {
		return err
	}
	request.Header.Set("Accept", accept)
	if body != nil {
		request.Header.Set("Content-Type", "application/json")
	}
	if c.APIKey != "" {
		request.Header.Set("Authorization", "Bearer "+c.APIKey)
	}

	response, err := c.HTTP.Do(request)
	if err != nil {
		return fmt.Errorf("request %s %s: %w", method, path, err)
	}
	defer response.Body.Close()

	if response.StatusCode < 200 || response.StatusCode >= 300 {
		raw, _ := io.ReadAll(response.Body)
		return &APIError{
			StatusCode: response.StatusCode,
			Message:    errorMessage(raw, response.Status),
			Body:       raw,
		}
	}

	if out == nil {
		_, err = io.Copy(io.Discard, response.Body)
		return err
	}
	return json.NewDecoder(response.Body).Decode(out)
}

// Health 返回 /healthz。
func (c *Client) Health(ctx context.Context) (map[string]any, error) {
	var payload map[string]any
	err := c.do(ctx, http.MethodGet, "/healthz", nil, "application/json", &payload)
	return payload, err
}

// Ready 返回 /readyz。
func (c *Client) Ready(ctx context.Context) (map[string]any, error) {
	var payload map[string]any
	err := c.do(ctx, http.MethodGet, "/readyz", nil, "application/json", &payload)
	return payload, err
}

// Models 返回已注册模型。
func (c *Client) Models(ctx context.Context) ([]ModelInfo, error) {
	var payload struct {
		Data []ModelInfo `json:"data"`
	}
	if err := c.do(ctx, http.MethodGet, "/v1/models", nil, "application/json", &payload); err != nil {
        return nil, err
	}
	return payload.Data, nil
}

// Chat 非流式对话。
func (c *Client) Chat(ctx context.Context, request ChatRequest) (*ChatResponse, error) {
	var response ChatResponse
	if err := c.do(ctx, http.MethodPost, "/v1/chat/completions", request.body(), "application/json", &response); err != nil {
		return nil, err
	}
	return &response, nil
}

// ChatStream 流式对话。
//
// 返回三个通道：chunks 产出文本增量，errs 产出错误，done 在结束后关闭。
// 调用方应在结束时 cancel ctx 以释放连接。
func (c *Client) ChatStream(ctx context.Context, request ChatRequest) (<-chan string, <-chan error, func()) {
	chunks := make(chan string, 32)
	errs := make(chan error, 1)
	done := make(chan struct{})

	request.Stream = true
	body, err := json.Marshal(request.body())
	if err != nil {
		errs <- err
		close(chunks)
		close(errs)
		close(done)
		return chunks, errs, func() {}
	}

	httpRequest, err := http.NewRequestWithContext(ctx, http.MethodPost, c.BaseURL+"/v1/chat/completions", bytes.NewReader(body))
	if err != nil {
		errs <- err
		close(chunks)
		close(errs)
		close(done)
		return chunks, errs, func() {}
	}
	httpRequest.Header.Set("Content-Type", "application/json")
	httpRequest.Header.Set("Accept", "text/event-stream")
	if c.APIKey != "" {
		httpRequest.Header.Set("Authorization", "Bearer "+c.APIKey)
	}

	response, err := c.HTTP.Do(httpRequest)
	if err != nil {
		errs <- err
		close(chunks)
		close(errs)
		close(done)
		return chunks, errs, func() {}
	}

	go func() {
		defer response.Body.Close()
		defer close(chunks)
		defer close(errs)
		defer close(done)

		if response.StatusCode < 200 || response.StatusCode >= 300 {
			raw, _ := io.ReadAll(response.Body)
			errs <- &APIError{StatusCode: response.StatusCode, Message: errorMessage(raw, response.Status), Body: raw}
			return
		}

		scanner := bufio.NewScanner(response.Body)
		scanner.Buffer(make([]byte, 0, 64*1024), 4*1024*1024)

		for scanner.Scan() {
			line := strings.TrimSpace(scanner.Text())
			if !strings.HasPrefix(line, "data:") {
				continue
			}
			payload := strings.TrimSpace(strings.TrimPrefix(line, "data:"))
			if payload == "" || payload == "[DONE]" {
				continue
			}

			var frame struct {
				Choices []struct {
					Delta struct {
						Content string `json:"content"`
					} `json:"delta"`
				} `json:"choices"`
			}
			if err := json.Unmarshal([]byte(payload), &frame); err != nil {
				continue
			}
			if len(frame.Choices) == 0 {
				continue
			}
			if delta := frame.Choices[0].Delta.Content; delta != "" {
				select {
				case chunks <- delta:
				case <-ctx.Done():
					return
				}
			}
		}
		if err := scanner.Err(); err != nil {
			errs <- err
		}
	}()

	cancel := func() { _ = response.Body.Close() }
	return chunks, errs, cancel
}

// Embeddings 向量化。
func (c *Client) Embeddings(ctx context.Context, model, input string) (map[string]any, error) {
	var payload map[string]any
	body := map[string]any{"model": model, "input": input}
	err := c.do(ctx, http.MethodPost, "/v1/embeddings", body, "application/json", &payload)
	return payload, err
}

// TTS 语音合成，返回 PCM（16bit little-endian）字节。
func (c *Client) TTS(ctx context.Context, request SpeechRequest) ([]byte, error) {
	httpRequest, err := http.NewRequestWithContext(
		ctx,
		http.MethodPost,
		c.BaseURL+"/v1/audio/speech",
		bytes.NewReader(mustJSON(request.body())),
	)
	if err != nil {
		return nil, err
	}
	httpRequest.Header.Set("Content-Type", "application/json")
	httpRequest.Header.Set("Accept", "audio/pcm")
	if c.APIKey != "" {
		httpRequest.Header.Set("Authorization", "Bearer "+c.APIKey)
	}

	response, err := c.HTTP.Do(httpRequest)
	if err != nil {
		return nil, err
	}
	defer response.Body.Close()

	if response.StatusCode < 200 || response.StatusCode >= 300 {
		raw, _ := io.ReadAll(response.Body)
		return nil, &APIError{StatusCode: response.StatusCode, Message: errorMessage(raw, response.Status), Body: raw}
	}
	return io.ReadAll(response.Body)
}

// Decide 决策模型推理。
func (c *Client) Decide(ctx context.Context, request DecideRequest) (*DecideResponse, error) {
	var response DecideResponse
	if err := c.do(ctx, http.MethodPost, "/v1/decide", request.body(), "application/json", &response); err != nil {
		return nil, err
	}
	return &response, nil
}

func mustJSON(value any) []byte {
	encoded, err := json.Marshal(value)
	if err != nil {
		return []byte("{}")
	}
	return encoded
}

func errorMessage(raw []byte, fallback string) string {
	var payload struct {
		Error struct {
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.Unmarshal(raw, &payload); err == nil && payload.Error.Message != "" {
		return payload.Error.Message
	}
	if len(raw) > 0 {
		return string(raw)
	}
	return fallback
}