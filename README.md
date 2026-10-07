# CAIdex

基于真实 Codex Runtime 的 Windows、iOS 和 CLI 多模型客户端，支持独立 Chat、远程 Codex 与自托管历史同步。当前处于基础工程开发阶段，完整客户端尚未实现。

当前可以运行：执行端凭据 Broker、脱敏及 CLI 凭据状态/保存/删除，固定版本 app-server 的双向 JSONL 适配、Runtime facade（线程/轮次/Steer/interrupt、能力门控、审批/输入转交）、协议回归测试与离线 `caidex doctor`。完整协议调用入口保留常规和实验方法；具体模型/工具能力仍需逐项验证，详见能力对照。doctor 只创建隔离的临时线程，不启动模型轮次、不执行项目命令、不读取用户 Codex 登录配置。

## 开发与验证

需要 Rust 1.99.0、Python 3、Codex CLI 0.160.1。Node 22.23.3 用于 CI 工具。

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p caidex-cli -- doctor
node scripts/verify-codex-schema.mjs
```

doctor 输出 JSON 验证结果；版本不匹配直接报错。可通过 `CAIDEX_CODEX_BIN` 指定实际 Codex 可执行文件。

Windows npm 的 `.cmd` 包装器不适合作为原生进程接口。CI 安装固定 npm 包后用以下命令定位原生二进制，再将返回路径设为 `CAIDEX_CODEX_BIN`：

```sh
npm install --prefix .tools/codex --no-audit --no-fund --ignore-scripts @openai/codex@0.160.1
node scripts/codex-binary.mjs
```

## 工程位置

- `runtime/bridge`：上游 stdio 边界、Runtime facade、方法清单、审批/用户输入与事件转交。
- `model/core`：ModelProvider 六方法接口、模型能力/兼容性 Registry、经典/Lite 请求/完整回复/工具/usage 视图、增量 SSE 和流生命周期。
- `model/providers/custom`：可独立调用的 Custom Responses 推理 client，供普通 Chat 与 Gateway 共用；配置模型列表、Broker 认证、显式 context headers、TLS 验证、取消/超时/背压与安全错误。
- `model/providers/openai`：原生 Models 发现/Responses 适配器，显式执行端组织/项目与凭据引用；复用共享传输，默认无服务端历史存储。当前使用合成协议服务验证，真实 API 兼容性待验。
- `model/providers/anthropic`：原生 Messages/Models/SSE 协议与 Broker 认证的原生 HTTP client；已开始离线验收，已有原生流式 HTTP 与背压/取消，已有原生回复投影与回放，请求转换和 Gateway 接入待续。
- `model/gateway`：Rust 库形式的本地 Responses HTTP/SSE Gateway，可注入原生 ModelProvider；生产 Host/CLI 配置入口和其他 Provider 待实现。
- `credentials/core`：执行端身份/profile 隔离、系统/环境/Unix 文件存储与诊断脱敏。
- `apps/cli`：开发阶段诊断与本地凭据管理命令；最终 CLI/共享 Host 接入仍待实现。
- `upstream/codex`：版本/源码锁定、协议基准和上游许可证。
- `assets/brand`：用户指定的品牌原件。
- `docs/CAIdex-实施计划-V3.md`：阶段顺序与验收条件。
- `docs/CAIdex-Model-Gateway-设计与验收.md`：模型协议/真实 wire 验证与 Gateway 恢复点。
- `docs/CAIdex-OpenAI-Provider-设计与验收.md`：原生模型发现、认证/存储边界与离线验收。
- `docs/CAIdex-Runtime-能力对照.md`：完整固定协议清单、实现范围与 CLI 对照验收状态。
- `docs/CAIdex-UI-规范-V1.md`：UI 布局和 CAIdex 功能入口。
- `HANDOFF.md`：当前进度、验证与下一步；每次续接先读。

当前适配层是本地传输组件，尚不包含生产 Host 的持久化、重连和生命周期管理。CLI 可通过 `caidex credentials --help` 查看凭据管理用法；没有秘密导出命令，保存只接受显式管道输入。普通 Chat、模型 Key 设置页面和 Windows/iOS 页面尚未实现。

## CI 与许可证

基础 CI 在 Linux、Windows 和 macOS 验证 Rust 工程及离线协议。iOS 应用工程建立后加入 simulator 测试和无签名 archive；目前 macOS Rust 检查不等同于 iOS 构建。

上游 Codex 与归档协议遵循 `upstream/codex/LICENSE`、`NOTICE`。CAIdex 自有代码及品牌的对外分发许可证尚未确定。
