# CAIdex 项目交接

更新：2026-10-07。每次工作先读本文件，再检查 Git/AGENTS.md；按恢复点继续，不重新规划已确认架构。

## 当前任务

F/G：Anthropic 客户端动态工具发现与 v4 历史载体当前范围已三平台验收，源码 46d03d52806a20b2e0ca3db1d6ed5fbfd31de5ed 已 push；[CI 37650049761](https://github.com/bboytang/CAIdex/actions/runs/37650049761) 三平台 completed/success，新增10项逐平台通过。本轮网页搜索明确拒绝与 SSE 占位状态修正、经典真实 MCP 发现/执行/结果精确回放、app-server 实际重启/落盘 v4 恢复，以及原生 Lite 两轮/Code Mode 审批执行/interrupt 已本地通过；完整 Runtime30、workspace/Clippy/fmt/语法检查通过，独立审查无重要问题。源码与验收文档待聚焦提交/push、新三平台 CI。完整 F/G 未验收，H–R 尚未实施。沿 V3 离线 fixture 授权；完整项目/文档公开、提交/push/CI 已获授权，不重复询问。不读取用户模型 Key，不调用商业 API。

## 已完成

- A–E：V3/UI/品牌原件、固定上游、Rust workspace/CI、真实 app-server facade、审批/Queue/Steer/Goal/MCP/PTY 等离线 Runtime 回归；执行端凭据 Broker、Windows keyring/Linux Secret Service/受保护文件/env、脱敏与 CLI。详细证据见 docs 对应验收文件。
- F 当前范围：core 经典/Lite wire、SSE/opaque/工具原文；loopback Gateway、独立 token、Broker/取消/背压/TLS/安全错误；ModelProvider 六方法、Registry/证据门槛；Custom 与 OpenAI Adapter 已三平台验收。
- Anthropic：原生 Messages/Models/六方法、工具/图片/推理/结构化输出、summary/context、Runtime 参数、verbosity/partial usage、增量 SSE、fallback 身份、thinking beta、认证组织及 v3 compiled 前缀绑定已三平台验收。已有证据保留在 Anthropic 文档，不重做完成阶段。
- tool_search 共享 wire 契约：专用 call/output 借用视图，JSON arguments/原 tools 保留，client call_id 与完整终态校验；源码 d29234c90f512f1f839753612194783f4f8afe6e，CI 37644294771 三平台成功。
- 本轮 Anthropic 发现：专用客户端 call、pending 匹配与加载、deferred reference/新或修订 definition、初始 top tools 稳定、显式 beta/profile/组织门控、v4 原始 discoveries/前缀绑定、JSON/SSE 三轮回放。本地完整检查与本轮三平台 CI 通过；独立审查的重要问题（结果后追加用户文字）已 RED→GREEN 修复，载体降级遗漏也已 RED→GREEN 修复。

## 未完成与下一步顺序

1. 当前恢复点：聚焦提交本轮 Provider/Runtime fixture 和文档，push 后逐平台核对新增 HTTP1/增强状态回归、Runtime5 与整套 Linux30/WindowsmacOS29、原生凭据/schema/doctor；通过后更新 checkpoint。无需再跳过旧经典用例。
2. 留下审查 Minor：重启用例进一步精确比较第三轮回复本身；Lite Code Mode 结果进一步与落盘 custom_tool_call_output 完整对比。现有断言已证明实际执行/审批及 MCP 结果原文相等，但不扩大声明这些未覆盖项。
3. Anthropic 完整经典缓存 web_search 仍不支持：默认请求明确失败且认证/POST为0；正例明确由 Runtime 配置 web_search=disabled，Gateway 不过滤工具，不标 Full。原生映射只有获得等价缓存/text+image语义证据才能开放；缺能力不阻碍其他 Provider 推进。
4. Gemini → 兼容 API/Ollama，按原计划逐项验证原生请求/工具/usage/reasoning/images/context/结构化输出与版本化报告；Models 清单和合成 fixture 不证明商业 Full。
5. 按 V3 的 H/I → Windows → SSH/iOS → CLI → Relay → R 继续。iOS simulator/无签名 archive 在 GitHub macOS；真机/UAC/签名/商业模型兼容性保留独立验收。

## 重要架构决定

- Windows Tauri 2 + React/TypeScript/Rust；iOS SwiftUI、Swift 5/iOS 17+ + UniFFI；Windows11x64、Linuxx86_64 Host/CLI；CLI 英文。品牌只用 assets/brand 用户原件，UI 尽量 1:1 官方参考，CAIdex 模型/Key 放设置。
- 固定真实 Codex 0.160.1，commit d27764b82f7118f674371e6d6e76271d9d606edb；一个 Runtime，不造第二套 Agent。JSONL facade 保留未知方法/字段，实验 opt-in。经典 gpt-5.5 与 Lite/Code Mode gpt-6.1-sol 分别验收，未知模型 fallback 不代表兼容性。
- Gateway Responses HTTP/SSE 只适配，不执行工具；Runtime 负责注册表发现/审批/沙箱/执行。固定 profile 禁止 request/stream 重试，loopback token 仅注入隔离子进程；HTTPS 验证不可削弱。
- Anthropic reasoning.encrypted_content 是 CAIdex 版本化敏感 JSON 载体，不是加密。完整 native wire 是回放权威；v3 绑定实际 system/tools/messages/认证组织，v4 另存初始声明和原始发现输出；已有非延迟 v1–v3 路径不自动升级。旧载体/错误模型/跨 provider 不自动认领；签名真实性由 Provider 校验，生产历史访问控制由 H/I 落实。
- 动态发现须执行端 inline-tools-2026-09-15 beta + 模型 discovery/system 支持 + 预期组织；初始 native tools 不变，新定义按原位置追加，不重写 signed 前缀。不从名称授予模型 Full 或 grammar 硬约束。
- 普通 Chat 独立无 Shell/Git/项目写权限；Remote 使用执行 Host Key，手机不读取已存 Host Key；同步不含凭据。模型切换轮次边界生效，跨 provider 关联分支/新线程。
- Host 独立后台、SQLite journal 先落盘再广播、快照补缺口、请求幂等、审批首次有效；不承诺外部操作恰好执行一次。GUI/SSH 退出不结束任务，不盲目重跑未知结果。
- Remote 先 SSH 后 Noise/Snow Relay；Host 切换不迁移活动线程。Chat 自托管 HTTPS 同步、SQLite/outbox/cursor、冲突分支/tombstone；服务端是信任边界，不宣称历史 E2EE。无签名 iOS archive 不等于可安装 IPA。

## 当前问题 / 环境

- Anthropic 原生缓存网页映射仍无等价证据，已改明确 unsupported_anthropic_web_search 并验证默认经典在 Key/HTTP 前失败。显式禁用网页的真实动态发现正例通过，不能据此宣称默认经典兼容。Windows/macOS 新增执行路径还须本轮 CI；商业 Provider 未验证。
- .git 普通沙箱只读，Git 提交/push 用 require_escalated；用户已授权。gh 已登录 bboytang，repo/workflow scope；旧 helper 缺 workflow scope，发布用 git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main，不改全局配置或输出凭据。
- 正常沙箱禁止 loopback 且 /tmp/.git 触发凭据文件的 Git 拒绝；完整测试使用获授权执行环境、TMPDIR=/var/tmp，不能放松保护。本机无 Windows/Xcode/gnome-keyring-daemon，对应原生验证用 GitHub。
- 同步 SecretStore 读开始后无法强制停止，但取消后不 POST；Broker 引用可换 Key，不等于组织身份；无自动 drop_block/retry。未配置真实模型 Key。

## 文件与 Git 状态

- branch main 跟踪 origin/main；本轮启动时 HEAD=bd1183d，远端同步。本轮源码 46d03d5 与 docs 已 push，CI 已验收；本轮未提交 model/providers/anthropic/src/tools.rs、tests/http/discovery.rs、HANDOFF.md 与下述 Runtime 两文件；当前源码以 git status 为准。源码/CI checkpoint 以实际 git log 为准。
- 本轮：model/providers/anthropic/src/{tools,request,provider,client,binding,projection,response_stream}.rs；tests/{tools,discovery,http}.rs、tests/http/{discovery,compiled}.rs；docs/CAIdex-Anthropic-Provider-设计与验收.md。本轮不改生产 Runtime、依赖和品牌。
- runtime/bridge/tests/real_runtime.rs、tests/fixtures/responses_server.py 的既有未提交 Lite 接线本轮已续接完善：新增默认缓存网页拒绝、显式 scope MCP 发现/执行/重启，合成 SecretStore 读取计数、初始 trace 与 discovery SSE 分片。本轮可连同 Provider 门控一起聚焦提交；生产 Runtime、依赖与品牌未改。
- docs/CAIdex-实施计划-V3.md、CAIdex-UI-规范-V1.md 是执行基准；原 V2 仅历史参考。Runtime-能力对照、Credentials/Model-Gateway/OpenAI/Anthropic 设计与验收保留详细阶段证据。
- credentials/core、apps/cli、model/core/providers/gateway、runtime/bridge、upstream/codex、.github/workflows/ci.yml、scripts 是已建立代码边界；assets/brand 四份原件未改。

## 测试与审查结果

- 本轮 cargo test --workspace --locked exit0（Anthropic HTTP50）；Clippy -D warnings/fmt/diff/Python AST 通过。日志 /tmp/caidex-anthropic-runtime-{workspace,clippy,gates}.log。
- 本地真实 Runtime：--ignored 全部30通过（无 skip），日志 /tmp/caidex-anthropic-runtime-full.log；其中默认经典为明确拒绝负例，不是缓存网页支持。正例实际3轮 MCP + app-server 重启第4轮；Lite3也通过。默认 workspace 不执行 ignored 原生服务/真实 Runtime 项；本机缺原生服务由 CI 验。
- 两项工具映射、显式配置/raw beta、v4 降级与审查发现的用户追加文字已实际 RED→GREEN。JSON/SSE 搜索分片、大整数、原始发现输出、deferred/重复/schema 修订、并行 custom、篡改/配置门控、联合 betas 均本地验证；新增10项本轮三平台 CI 全部通过，不等于真实模型能力。
- 上轮审查的搜索 added 状态 Minor 本轮已 RED→GREEN 修复；网页错误门控也先 RED→GREEN。真实 MCP 初始缺 profile RED，明确开启配置后通过；初版 MCP 文本断言与固定 Runtime structuredContent 格式不符，已改断言实际 JSON 且与落盘原文相等，未改生产数据。本轮独立审查无 Critical/Important，2项覆盖 Minor 留下一步；商业/签名/完整缓存 web、生产 Host 权限及尚未运行的三平台验收单独保留，不授予能力。
- 本轮 [CI 37650049761](https://github.com/bboytang/CAIdex/actions/runs/37650049761)：精确源码 46d03d52806a20b2e0ca3db1d6ed5fbfd31de5ed；三平台 completed/success，日志逐平台新增10/发现4/HTTP49/工具9通过，workspace/fmt/Clippy/native credentials/schema/doctor 全过，既有真实 Runtime Linux25/WindowsmacOS24通过。日志 /tmp/caidex-ci-37650049761.log；未提交的新增原生 Runtime 接线未纳入该 CI，不宣称经典全部或商业 API 通过。
- 共享 core [CI 37644294771](https://github.com/bboytang/CAIdex/actions/runs/37644294771) 三平台 completed/success，core22/新增2逐平台通过；既有 Runtime Linux25/WindowsmacOS24及 native credentials/schema/doctor 全过。参数/usage [CI 37641597463](https://github.com/bboytang/CAIdex/actions/runs/37641597463)、v3 [CI 37637494900](https://github.com/bboytang/CAIdex/actions/runs/37637494900) 均已验收，详细历史见对应 docs。
- 所有本轮 Key/组织/回复均合成 fixture；iOS/商业 API/逐状态 UI/真机/UAC/签名未验。macOS Rust CI 不等于 iOS 应用构建。本地 /tmp 日志可丢失，跨环境优先 GitHub 证据。
