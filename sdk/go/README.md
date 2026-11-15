# Basic-Studio Go SDK

仅依赖标准库。

## 安装

```bash
go get github.com/CherryKingOne/Basic-Studio/sdk/go
```

## 快速开始

```go
package main

import (
    "context"
    "fmt"

    basicstudio "github.com/CherryKingOne/Basic-Studio/sdk/go"
)

func main() {
    ctx := context.Background()
    client := basicstudio.New("http://127.0.0.1:18596")

    reply, err := client.Chat(ctx, basicstudio.ChatRequest{
        Model:    "chat/mock",
        Messages: []basicstudio.Message{basicstudio.User("你好")},
        System:   "be brief",
    })
    if err != nil {
        panic(err)
    }
    fmt.Println(reply.Content())

    chunks, errs, closeStream := client.ChatStream(ctx, basicstudio.ChatRequest{
        Model:    "chat/mock",
        Messages: []basicstudio.Message{basicstudio.User("讲个故事")},
    })
    defer closeStream()

    for chunk := range chunks {
        fmt.Print(chunk)
    }
    if err := <-errs; err != nil {
        panic(err)
    }

    pcm, err := client.TTS(ctx, basicstudio.SpeechRequest{Model: "tts/echomock", Text: "你好世界"})
    if err != nil {
        panic(err)
    }
    _ = pcm
}
```

## 测试

```bash
cd sdk/go && go test ./...
```
