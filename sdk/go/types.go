package basicstudio

// Message 是一条对话消息。
type Message struct {
	Role    string `json:"role"`
	Content any    `json:"content"`
}

// System 构造 system 消息。
func System(content any) Message { return Message{Role: "system", Content: content} }

// User 构造 user 消息。
func User(content any) Message { return Message{Role: "user", Content: content} }

// Assistant 构造 assistant 消息。
func Assistant(content any) Message { return Message{Role: "assistant", Content: content} }

// ChatRequest 是 Chat Completions 请求。
type ChatRequest struct {
	Model       string
	Messages    []Message
	System      string
	Stream      bool
	Temperature *float64
	TopP        *float64
	MaxTokens   *int
}

func (r ChatRequest) body() map[string]any {
	messages := r.Messages
	if r.System != "" {
		messages = append([]Message{System(r.System)}, messages...)
	}

	body := map[string]any{
		"model":    r.Model,
		"messages": messages,
		"stream":   r.Stream,
	}
	if r.Temperature != nil {
		body["temperature"] = *r.Temperature
	}
	if r.TopP != nil {
		body["top_p"] = *r.TopP
	}
	if r.MaxTokens != nil {
		body["max_tokens"] = *r.MaxTokens
	}
	return body
}

// Usage 是 token 用量。
type Usage struct {
	PromptTokens     int `json:"prompt_tokens"`
	CompletionTokens int `json:"completion_tokens"`
	TotalTokens      int `json:"total_tokens"`
}

// ChatChoice 是 Chat 返回的一个候选。
type ChatChoice struct {
	Index        int     `json:"index"`
	Message      Message `json:"message"`
	FinishReason string  `json:"finish_reason"`
}

// ChatResponse 是 Chat Completions 响应。
type ChatResponse struct {
	ID      string       `json:"id"`
	Model   string       `json:"model"`
	Choices []ChatChoice `json:"choices"`
	Engine  string       `json:"engine"`
	Usage   Usage        `json:"usage"`
}

// Content 返回首个候选的文本。
func (r *ChatResponse) Content() string {
	if len(r.Choices) == 0 {
		return ""
	}
	if text, ok := r.Choices[0].Message.Content.(string); ok {
		return text
	}
	return ""
}

// ResponsesRequest 是 Responses API 请求。
type ResponsesRequest struct {
	Model              string
	Input              any
	Instructions       string
	Stream             bool
	Temperature        *float64
	MaxOutputTokens    *int
	PreviousResponseID string
}

func (r ResponsesRequest) body() map[string]any {
	body := map[string]any{
		"model":  r.Model,
		"input":  r.Input,
		"stream": r.Stream,
	}
	if r.Instructions != "" {
		body["instructions"] = r.Instructions
	}
	if r.Temperature != nil {
		body["temperature"] = *r.Temperature
	}
	if r.MaxOutputTokens != nil {
		body["max_output_tokens"] = *r.MaxOutputTokens
	}
	if r.PreviousResponseID != "" {
		body["previous_response_id"] = r.PreviousResponseID
	}
	return body
}

// ResponsesResponse 是 Responses API 响应。
type ResponsesResponse struct {
	ID        string `json:"id"`
	Model     string `json:"model"`
	Status    string `json:"status"`
	OutputText string `json:"output_text"`
}

// SpeechRequest 是语音合成请求。
type SpeechRequest struct {
	Model   string
	Text    string
	Voice   string
	Emotion string
}

func (r SpeechRequest) body() map[string]any {
	body := map[string]any{
		"model":  r.Model,
		"input":  r.Text,
		"stream": false,
	}
	if r.Voice != "" {
		body["voice"] = r.Voice
	}
	if r.Emotion != "" {
		body["emotion"] = r.Emotion
	}
	return body
}

// DecideRequest 是决策模型请求。
type DecideRequest struct {
	Model   string
	State   any
	Context string
}

func (r DecideRequest) body() map[string]any {
	body := map[string]any{"model": r.Model, "state": r.State}
	if r.Context != "" {
		body["context"] = r.Context
	}
	return body
}

// DecideResponse 是决策模型响应。
type DecideResponse struct {
	Model  string `json:"model"`
	Output string `json:"output"`
	Engine string `json:"engine"`
}

// ModelInfo 是 /v1/models 中的条目。
type ModelInfo struct {
	ID         string   `json:"id"`
	Task       string   `json:"task"`
	Engines    []string `json:"engines"`
	Adapter    string   `json:"adapter"`
	APIVersion string   `json:"api_version"`
	Streaming  bool     `json:"streaming"`
}