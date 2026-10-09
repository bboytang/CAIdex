# CAIdex

基于真实 Codex Runtime 的 Windows、iOS 和 CLI 多模型客户端，目标支持独立 Chat、远程 Codex、官方统一 CAIdex 账户、跨模型长期记忆与可选云同步。当前仍处于基础工程开发阶段，完整客户端及账户/记忆/云服务尚未实现。

正式目标架构：CAIdex 统一运营账户及 PostgreSQL + pgvector 云服务；Windows/iOS 本地缓存/记忆与 Host journal 保留 SQLite。记忆同步默认关闭，独立于自动记忆和 Chat 历史同步，首次开启须登录并确认范围；关闭采用账户级权威状态，支持保留或删除原云端记忆。记忆归属不可变 user_id，Chat/整合/Embedding 模型分开，跨模型共享不迁移活动 Codex 线程。账户登录不授予 Host 执行权限，用户模型 API Key 仍留在执行端；未登录/云故障时本地能力按既有权限继续。

规划云服务使用可替换 EmailSender（Brevo 优先、Resend 备用）及早期单实例 VPS，只做本机有限备份与用户导出，无异地备份；整台 VPS/磁盘损坏可能数据库和备份全损，本地缓存不保证全量恢复。详细边界与全部待实施验收见[Account/Memory/Cloud 设计](docs/CAIdex-Account-Memory-Cloud-设计与验收-V1.md)，按 V3 在 H/I 及后续客户端阶段实现，不代表当前已有账户/Memory Engine/UI。

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
- `model/providers/anthropic`：原生 Messages/Models/SSE、Responses 转换和绑定历史；Gateway、经典 MCP 发现/执行/重启和 Lite Code Mode 审批/执行/取消已三平台离线验收，默认缓存网页搜索及商业模型兼容性待验。
- `model/providers/google`：Gemini 原生 Models/generateContent、媒体/推理/结构输出和绑定历史；Gateway 与固定经典/Lite Runtime 离线接线已三平台验收，商业模型兼容性待验。
- `model/providers/ollama`：复用共享传输的原生 Responses 适配，六方法、能力/推理、媒体/结构输出、绑定历史和 deferred 工具发现已三平台离线验收；固定经典 Runtime MCP 执行/重启及 Lite Code Mode 审批/执行/取消/磁盘恢复亦已三平台验证，真实 daemon/模型兼容性待验。
- `model/providers/deepseek`：原生 Models 目录、六方法和经典文本 JSON/SSE，复用共享传输与独立执行端凭据；基础 Gateway、显式本地上下文、developer 优先指令与 verbosity 编译已三平台离线验收；新增显式 Classic 函数/namespace、明文推理绑定历史及执行端 effort 映射也已分别完成精确三平台离线验收；新增独立显式 custom apply_patch 与 v2 历史绑定（grammar 仅作指导）已精确三平台离线验收；显式 Runtime+history 组合新增本地 summary auto/context all_turns/include 控制，DeepSeek42 项及 workspace423/418/422 已精确三平台离线验收；多 reasoning/content-part 与交错流索引及未知扩展边界已精确三平台离线验收（DeepSeek46 项，workspace427/422/426）；其他 custom/高级控制、Lite/实际 Runtime 接线、真实模型兼容性待验。
- `model/gateway`：Rust 库形式的本地 Responses HTTP/SSE Gateway，可注入原生 ModelProvider；生产 Host/CLI 配置入口和剩余兼容 API 接入待实现。
- `credentials/core`：执行端身份/profile 隔离、系统/环境/Unix 文件存储与诊断脱敏。
- `apps/cli`：开发阶段诊断与本地凭据管理命令；最终 CLI/共享 Host 接入仍待实现。
- `upstream/codex`：版本/源码锁定、协议基准和上游许可证。
- `assets/brand`：用户指定的品牌原件。
- `docs/CAIdex-实施计划-V3.md`：阶段顺序与验收条件。
- `docs/CAIdex-Account-Memory-Cloud-设计与验收-V1.md`：官方账户/PostgreSQL、长期记忆、账户级同步开关、隐私/删除/本机备份及待实施验收矩阵。
- `docs/CAIdex-Model-Gateway-设计与验收.md`：模型协议/真实 wire 验证与 Gateway 恢复点。
- `docs/CAIdex-OpenAI-Provider-设计与验收.md`：原生模型发现、认证/存储边界与离线验收。
- `docs/CAIdex-Anthropic-Provider-设计与验收.md`：Anthropic 原生协议、工具发现与固定 Runtime 离线验收。
- `docs/CAIdex-Gemini-Provider-设计与验收.md`：Gemini 原生协议、绑定历史与固定 Runtime 离线验收。
- `docs/CAIdex-Ollama-Provider-设计与验收.md`：Ollama 配置、明确拒绝的控制项与三平台离线验收。
- `docs/CAIdex-DeepSeek-Provider-设计与验收.md`：DeepSeek 原生契约、基础 Adapter 范围与待验证能力。
- `docs/CAIdex-Runtime-能力对照.md`：完整固定协议清单、实现范围与 CLI 对照验收状态。
- `docs/CAIdex-UI-规范-V1.md`：UI 布局和 CAIdex 功能入口。
- `HANDOFF.md`：当前进度、验证与下一步；每次续接先读。

当前适配层是本地传输组件，尚不包含生产 Host 的持久化、重连和生命周期管理。CLI 可通过 `caidex credentials --help` 查看凭据管理用法；没有秘密导出命令，保存只接受显式管道输入。普通 Chat、模型 Key 设置页面和 Windows/iOS 页面尚未实现。

## CI 与许可证

基础 CI 在 Linux、Windows 和 macOS 验证 Rust 工程及离线协议。iOS 应用工程建立后加入 simulator 测试和无签名 archive；目前 macOS Rust 检查不等同于 iOS 构建。

上游 Codex 与归档协议遵循 `upstream/codex/LICENSE`、`NOTICE`。CAIdex 自有代码及品牌的对外分发许可证尚未确定。
