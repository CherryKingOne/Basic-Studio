# Basic-Studio 文档索引

| 文档 | 内容 |
| --- | --- |
| [ARCHITECTURE.md](ARCHITECTURE.md) | 总体架构、分层、核心 trait、引擎矩阵、请求生命周期、可靠性/安全/可观测性 |
| [DIRECTORY_STRUCTURE.md](DIRECTORY_STRUCTURE.md) | 完整目录树（Rust crates / sidecar / models / config / deploy） |
| [ROADMAP.md](ROADMAP.md) | M0~M6 里程碑、验收标准、风险与对策 |
| [API.md](API.md) | 对外接口清单（REST / WS / gRPC / CLI） |
| [ADR/0001-engine-integration.md](ADR/0001-engine-integration.md) | 为什么用"进程编排 + 协议客户端"而非内嵌 CPython |

## 快速了解

1. 先读 `ARCHITECTURE.md` §1 的结论表。
2. 再看 `DIRECTORY_STRUCTURE.md` 的目录树。
3. 想动手就按 `ROADMAP.md` M0 开始。