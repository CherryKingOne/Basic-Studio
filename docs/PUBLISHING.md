# 发布流程（SDK 与网关）

版本：**0.0.1**（第一个正式 tag）。

## 1. 产物与安装

| 语言 | 产物 | 发布目标 | 安装 |
| --- | --- | --- | --- |
| Python | wheel + sdist | PyPI (`basic-studio`) | `pip install basic-studio==0.0.1` |
| Rust | `.crate` | crates.io (`studio-sdk`) | `cargo add studio-sdk@0.0.1` |
| Node.js | `.tgz` | npm (`@basic-studio/sdk`) | `npm install @basic-studio/sdk@0.0.1` |
| Java | `.jar` | Maven Central (`ai.basicstudio:basic-studio-sdk`) | `mvn` 依赖 |
| Go | 源码模块 | GitHub tag（无独立仓库） | `go get .../sdk/go@v0.0.1` |

## 2. 发布步骤

```bash
# 1) 校验
cargo clippy --workspace --all-targets
cargo test --workspace
cd sdk/python && python3 -m unittest discover -s tests
cd ../nodejs && npm test
cd ../java && mvn test
cd ../go && go test ./...

# 2) 构建产物（可选，CI 会做）
./scripts/build-sdk.sh

# 3) 提交与打 tag
git commit -am "release: 0.0.1"
git tag 0.0.1
git push origin main
git push origin 0.0.1
```

## 2.1 手工发布（CI 不可用时）

```bash
python3 -m build sdk/python --outdir dist/python
python3 -m twine upload dist/python/*

cd sdk/nodejs && npm publish --access public

cargo publish -p studio-sdk

mvn -f sdk/java/pom.xml -DskipTests deploy
```

## 3. CI 自动发布

`.github/workflows/sdk-release.yml` 监听形如 `0.0.1` 的 tag：

1. 构建 5 种语言产物并上传 workflow artifact；
2. 创建 GitHub Release 并附加产物；
3. 分别发布 PyPI（trusted publishing）、npm（`NPM_TOKEN`）、crates.io（`CARGO_REGISTRY_TOKEN`）、Maven Central（`OSSRH_*`）。

需要在仓库设置 Secrets：

| Secret | 用途 |
| --- | --- |
| `NPM_TOKEN` | npm publish |
| `CARGO_REGISTRY_TOKEN` | crates.io publish |
| `OSSRH_USERNAME` / `OSSRH_TOKEN` | Maven Central deploy |

PyPI 使用 Trusted Publishing，无需长期 token（在 PyPI 侧配置 publisher）。

## 4. 版本规则

- 主版本：网关接口不兼容变更。
- 次版本：新增接口/SDK 能力。
- 补丁：缺陷修复、文档修正。
- 网关与各语言 SDK 共用同一版本号，便于追踪兼容关系。
- Go 的 `sdk/go/version.go::Version` 常量必须与 tag 一致。

## 4.1 本次发布检查表

- [x] 版本号统一为 0.0.1
- [x] 产物构建脚本 `scripts/build-sdk.sh`
- [x] 发布流水线 `.github/workflows/sdk-release.yml`
- [x] 安装文档 `sdk/README.md`、各语言 `sdk/*/README.md`
- [ ] 本地执行 `./scripts/build-sdk.sh`（需 python build / npm / cargo / mvn）
- [ ] 执行 `git tag 0.0.1` 并推送 tag