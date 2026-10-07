# CAIdex 项目交接

更新：2026-10-07。每次工作先读本文件，再检查 Git/AGENTS.md；按恢复点继续，不重新规划已确认架构。

## 当前任务

F/G：Anthropic 原生 Gateway / 实际 Runtime 本轮离线范围已三平台验收，源码 e27e9035eb41fec89eeda5dab43b1fd10640ed38 已提交/push；[CI 37654366666](https://github.com/bboytang/CAIdex/actions/runs/37654366666) 三平台 completed/success，HTTP50、真实 Runtime Linux30/WindowsmacOS29通过。当前按 V3 接 Gemini：原生 Models 分页/元数据和认证/预算/取消/TLS HTTP 基础已有未提交实现，本地 catalog4+HTTP9 与 workspace/Clippy 日志通过，独立审查无重要问题，文档已补齐，正在提交及三平台验收，不重做已验 Anthropic 阶段。完整 F/G 未验收，H–R 尚未实施。

沿 V3 离线 fixture 授权；完整项目/文档公开、提交/push/CI 已获授权，不重复询问。不读取用户模型 Key，不调用商业 API。默认经典 Anthropic cached web_search 明确不支持；显式禁用网页的正例不授予 Full。

## 已完成

- A–E：V3/UI/品牌原件、固定上游、Rust workspace/CI、真实 app-server facade、审批/Queue/Steer/Goal/MCP/PTY 等离线 Runtime 回归；执行端凭据 Broker、Windows keyring/Linux Secret Service/受保护文件/env、脱敏与 CLI。详细证据见 docs 对应验收文件。
- F 当前范围：core 经典/Lite wire、SSE/opaque/工具原文；loopback Gateway、独立 token、Broker/取消/背压/TLS/安全错误；ModelProvider 六方法、Registry/证据门槛；Custom 与 OpenAI Adapter 已三平台验收。
- Anthropic：原生 Messages/Models/六方法、工具/图片/推理/结构化输出、summary/context、Runtime 参数、verbosity/partial usage、增量 SSE、fallback 身份、thinking beta、认证组织及 v3 compiled 前缀绑定已三平台验收。已有证据保留在 Anthropic 文档，不重做完成阶段。
- tool_search 共享 wire 契约：专用 call/output 借用视图，JSON arguments/原 tools 保留，client call_id 与完整终态校验；源码 d29234c90f512f1f839753612194783f4f8afe6e，CI 37644294771 三平台成功。
- Anthropic 发现：专用客户端 call、pending 匹配与加载、deferred reference/新或修订 definition、初始 top tools 稳定、显式 beta/profile/组织门控、v4 原始 discoveries/前缀绑定、JSON/SSE 三轮回放。源码46d03d5的CI37650049761三平台通过；独立审查的重要问题（结果后追加用户文字）已 RED→GREEN 修复，载体降级遗漏也已 RED→GREEN 修复。

## 未完成与下一步顺序

1. Gemini：先原生 Models 分页/元数据及认证/预算/取消的 HTTP 基础，再原生生成/流式、版本化历史、请求转换与六方法/Gateway/固定 Runtime；沿用 core、Broker、Limits、endpoint/TLS 和证据门槛，不新建 Agent。当前源码在 model/providers/google；先完成该基础模块提交与三平台 CI，再继续原生生成及流式。尚未实现 ModelProvider 六方法/Gateway/Runtime 接线，不标 Gemini Full。
2. 预检已核对 [Models](https://ai.google.dev/api/models)、[generateContent](https://ai.google.dev/api/generate-content) 和 [认证](https://ai.google.dev/gemini-api/docs/api-key)：Models 使用 pageSize/pageToken/nextPageToken，原始字段保留；能力不从名称猜。当前 thinking/function-calling 指南示例多为 Interactions，不能混用其 signature/事件字段与 generateContent Part.thoughtSignature；实现时以所选原生 API reference 验证。
3. 留下审查 Minor：重启用例单独精确比较第三轮回复；Lite Code Mode 输出与落盘 custom_tool_call_output 完整对比。现有检查证明实际执行/审批及 MCP 结果原文相等，两项覆盖补充暂缓。
4. Anthropic 缓存网页仍无等价 cached/text+image 证据；默认请求认证/POST为0，Gateway 不过滤工具，不标 Full。原生映射获得等价证据才能开放，缺口不阻碍其他 Provider。
5. Gemini → 兼容 API/Ollama → V3 H/I → Windows → SSH/iOS → CLI → Relay → R。模型逐项验证并记录版本化报告；合成 fixture/Models清单不证明商业 Full。iOS simulator/无签名 archive 在 GitHub macOS；真机/UAC/签名/商业模型保留独立验收。

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

- Anthropic cached网页、grammar硬约束和商业 Provider 等未验能力保持原边界；生产 Host/历史权限、GUI/iOS 尚未实现。本轮路径词法断言问题已修复并三平台通过，不能重新跳过用例或放松隔离。
- .git 普通沙箱只读，Git 提交/push 用 require_escalated；用户已授权。gh 已登录 bboytang，repo/workflow scope；旧 helper 缺 workflow scope，发布用 git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main，不改全局配置或输出凭据。
- 正常沙箱禁止 loopback 且 /tmp/.git 触发凭据文件的 Git 拒绝；完整测试使用获授权执行环境、TMPDIR=/var/tmp，不能放松保护。本机无 Windows/Xcode/gnome-keyring-daemon，对应原生验证用 GitHub。
- 同步 SecretStore 读开始后无法强制停止，但取消后不 POST；Broker 引用可换 Key，不等于组织身份；无自动 drop_block/retry。未配置真实模型 Key。

## 文件与 Git 状态

- branch main 跟踪 origin/main，HEAD=1672db3；未提交 Cargo.toml/Cargo.lock（新增 google workspace 包，无第三方升级）、model/providers/google/ 全模块、新 Gemini 验收文档、Gateway 过时范围说明及本次 HANDOFF 更新。当前基础模块已实现且独立审查无重要问题，提交/新 CI 未完成，保留这些修改续接。
- Google 模块沿原方案路径，GeminiConfig/Client 使用执行端 google/ApiKey Broker、x-goog-api-key；只原生 Models GET，复用现有 Limits/endpoint/TLS，不增加通用传输抽象。raw 字段保留、完整分页才返回、能力只采用显式字段，不从名称推断。
- 上轮新增原生接线已在 b403b0d 提交：runtime/bridge/tests/real_runtime.rs、tests/fixtures/responses_server.py；Provider web_search/SSE门控在 model/providers/anthropic/src/tools.rs、tests/http/discovery.rs。详见 docs/CAIdex-Anthropic-Provider-设计与验收.md、Runtime-能力对照。
- docs/CAIdex-实施计划-V3.md、CAIdex-UI-规范-V1.md 是执行基准；原 V2 仅历史参考。Runtime-能力对照、Credentials/Model-Gateway/OpenAI/Anthropic 设计与验收保留详细阶段证据。
- credentials/core、apps/cli、model/core/providers/gateway、runtime/bridge、upstream/codex、.github/workflows/ci.yml、scripts 是已建立代码边界；assets/brand 四份原件未改。

## 测试与审查结果

- Gemini 审查 Minor 暂缓：ProtoJSON 替代整数表示目前拒绝；大数断言同源解析不能独立检测共同舍入；100ms caller deadline 的 socket 用例在繁忙 runner 有调度风险。细节及排除项裁定见 docs/CAIdex-Gemini-Provider-设计与验收.md；不冒称已修复。
- Gemini 当前本地：catalog4+HTTP9通过；目录/HTTP各有实现前 RED → GREEN 日志；随后 workspace 与 Clippy 日志完整结束且无失败，最后新增无效 Key/跨 Host Broker 负例包含在 workspace 验证中。/tmp/caidex-google-{catalog-red,catalog-green,http-red,http-green,native-foundation}.log 和 /tmp/caidex-google-foundation-{workspace,clippy}.log。旧执行句柄已消失，以实际日志为准；fmt/diff 已通过，独立只读审查无 Critical/Important，接下来提交及三平台新 CI。所有凭据/TLS/响应均为合成 fixture，未调用商业 API。


- 本轮 CI 37654366666：精确源码 e27e9035eb41fec89eeda5dab43b1fd10640ed38，三平台 completed/success；HTTP50，真实 Runtime Linux30/WindowsmacOS29，无 skip新增用例。逐平台确认 HTTP新增网页拒绝/SSE状态增强、Runtime5（默认经典拒绝、MCP发现执行/重启、Lite两轮、Code Mode审批执行、interrupt）；workspace/fmt/Clippy/native credentials/schema/doctor通过。日志 /tmp/caidex-ci-37654366666{,-linux,-macos,-windows}.log，跨机器优先 GitHub证据。
- 前次 CI37653194672 Linux全过、Windows/macOS各28通过1失败，唯一失败是 Lite两轮 rollout 的原始路径 starts_with。临时目录别名本地同一失败 RED→canonicalize GREEN，再完整 workspace/Runtime30/Clippy/fmt/diff通过；/tmp/caidex-runtime-path-{red,green,workspace,full,clippy}.log。修复后实际 Windows/macOS该用例已通过。
- scope：默认经典为明确拒绝负例，Key/POST0；正例 Runtime显式web_search=disabled，真实MCP执行/模型结果与落盘原文相等、app-server实际重启后第4轮恢复且不重复echo；Lite实际审批执行/结果回放/取消socket通过。普通 workspace 不执行 ignored Runtime/原生服务，CI显式验证。
- 上轮独立审查无 Critical/Important，2项覆盖 Minor见下一步；用户追加文字/v4降级/搜索added状态等已实际RED→GREEN。源码46d03d5的CI37650049761、core d29234c的CI37644294771等已验收，详细阶段证据在 docs，不重做。
- 所有 Key/组织/回复为合成fixture；商业推理/签名真实性/完整缓存web/生产Host权限/逐状态UI/iOS/真机/UAC/签名未验。macOS Rust CI不是iOS应用构建，无签名archive不是可安装IPA。
