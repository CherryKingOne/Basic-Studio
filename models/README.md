# models/ 目录约定

本目录存放模型权重，**不进 Git**（体积大）。仓库只提交 `model.toml` 清单与 `registry.toml`。

## 结构

```
models/
├── registry.toml        # 全局清单（提交）
├── models.lock          # 锁定文件：路径 + sha256 + 大小（提交）
├── tts/
│   ├── index-tts/       # 1.0
│   ├── index-tts1.5/
│   ├── index-tts2.0/
│   └── index-tts2.5/
└── DM/
    ├── laya/
    └── kev/
```

## 每个模型目录必须包含

**每个模型自包含**：权重、tokenizer、vocoder、音色等全部放在该模型自己的目录内，
不存在跨模型的公共目录。

| 文件/目录 | 说明 |
| --- | --- |
| `model.toml` | ★ studio 清单：id、task、引擎候选、显存需求、流式能力、限流参数 |
| `CHECKSUMS.sha256` | 权重校验和（可选，受 `checksum.policy` 控制） |
| 权重文件 | `*.safetensors` / `*.bin` / `*.gguf` / `*.onnx`（忽略） |

可选：`config.json`（上游自带）、`tokenizer/`、`vocoder/`、`speakers/`、`voices/`（用户音色，忽略）。

## 命名规则

- 目录名 = 模型 ID 去掉前缀后的部分；`id` 在 `model.toml` 中写全（如 `tts/index-tts2.0`）。
- TTS 版本号统一用 `index-tts` / `index-tts1.5` / `index-tts2.0` / `index-tts2.5`，
  `api_version` 分别为 `v10` / `v15` / `v20` / `v25`。
- DM 目录首字母大写（`DM/`），与上游命名保持一致。

## 下载

```bash
./scripts/fetch-models.sh                 # 按 registry.toml 拉取 enabled 的模型
./scripts/fetch-models.sh tts/index-tts2.5
```

## 校验

```bash
studio models ls
studio models inspect tts/index-tts2.5
studio models verify --all                # 校验 sha256
```