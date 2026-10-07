# CAIdex 项目交接

更新：2026-10-07。开始工作先读本文件，再检查 Git；按下一步继续，不重新规划已确认方向。

## 当前任务

- Anthropic input_transformations 正在验收：原生 JSON/SSE 检查报告形状并保留未知项；最终 message_delta 顶层非 null 数组替换 message_start 报告，缺省/null 不覆盖。共同投影/回放检查阻止已识别 thinking_dropped / thinking_mismatch_allowed 交付工具 done、完成或新载体；原生接口仍提供完整报告。不重试、不删除历史。新增 5 项回归；协议 13/投影 8/增量投影 5/HTTP 28、最终 workspace/Clippy/fmt/diff 本地通过，三平台 CI 待验。
- Anthropic 上下文头已三平台验收：源码 f35d2a7，CI 37616230221 completed/success，新增 4 项及扩展流式/配置测试逐平台日志通过。Config.with_local_runtime_context 显式接受三项本地 metadata，不外发；turn-state/错误方向头拒绝。原生 request-id 映射 x-request-id，JSON/SSE 接至 Provider；坏值拒绝。HTTP 26、workspace/Clippy/fmt/diff 本地通过。下一阶段：请求前缀/执行端账户历史绑定，不重做已验传输。
- 本轮发现并修正上一步遗漏：Provider profile 字符串 input 导致 Lite-only / Lite-first 初始化失败，现用合法消息数组，回归已通过且不读 Key。请求前缀/账户契约及 Gateway/实际 Runtime 尚未接入。
- ModelProvider 六方法此前已三平台验收：源码 6ae83b9，CI 37614432384 success（HTTP 22），其余能力及限制详见设计文档。本轮基于该实现继续，不重新实现传输。
- Responses SSE 已三平台验收：源码 6a3986a，CI 37613084679 completed/success，新增 4 项投影及 2 项 HTTP 逐平台日志通过；本地 workspace/Clippy/fmt/diff 通过。custom 输入块结束后完整解包；完整原生回复验证后才发载体及 done；不宣称逐字符 custom 流。

- 固定 Runtime 字段处理已实现并三平台验收：源码 1afee18，runtime_parameters.rs / ServiceTierMapping / 本地元数据开关，新增 3 项测试及扩展经典/Lite 实际 HTTP 两轮均通过；workspace/Clippy/fmt/diff 与 CI 37611149491 success。非空 stream_options/access_programs、priority/flex 仍明确拒绝，原生缓存策略不宣称等价。

- Anthropic summary/context 显式映射已实现并验收：源码 024b211，SummaryMapping / ThinkingContext、新增 3 项推理测试（累计 8 项）、经典/Lite 实际 HTTP 两轮扩展，workspace/Clippy/fmt/diff 与 CI 37610039312 三平台通过。恢复审查修正摘要不能映射 omitted、between_tools 不接受 display，并补齐上一步遗漏的交接。
- 六方法/SSE 已验，再接 Gateway/真实 Runtime。strict:true 结构化输出（9796682）已验；非 strict/JSON mode/description/verbosity 与实际模型保留规则仍需后续验证。
- A–E、F 当前范围与 OpenAI 离线 Adapter 已验收；F/G 整体和 H–R 尚未完成。沿 V3 离线 fixture 授权，不读取/创建真实模型 Key。完整项目及文档公开、提交/push/CI 均沿用用户授权。

## 已完成

- V3 执行计划、UI 规范、品牌原件归档、项目续接规则、Rust workspace、固定依赖和三平台 CI 已建立。原始 V2 仅历史参考，已确认调整优先。
- D：真实 app-server 双向 JSONL/facade、完整常规/实验方法门控、未知字段保留、审批/输入转交、取消/溢出关闭、不自动重试；19 项协议回归通过。
- D 真实 Runtime：历史/resume/fork、Queue CRUD/reorder/page/自动启动、审批 accept/cancel、Plan 输入、Steer/interrupt、apply_patch、MCP 工具/资源/elicitation、PTY/长进程、Goal 生命周期/预算/blocked、经典压缩已验证；本轮 CI 再次通过，详见能力对照。
- E：`credentials/core` Broker 固定 owner，reference 按 owner/provider/profile/kind 隔离；Secret 不可 Serialize/Display/Clone、释放时自身缓冲清零；配置状态仅公开元数据。
- E：Windows native keyring、Linux Secret Service（显式 feature，无 mock/fallback）、明确只读环境映射、Unix 私有文件 backend 已实现。文件保护含 0700/0600、symlink/hardlink/Git 拒绝、持有目录 FD、原子替换/fsync。
- E：结构化字段/HTTP header/已知值脱敏；已替换/删除秘密仍遮蔽。native 错误安全分类，不带原始 payload；锁定/访问拒绝与底层长度上限有专用错误。
- E：CLI `credentials status|set|remove` 已接入；明确 store/owner/provider/profile。set 必须 `--stdin` 管道输入，无 get/export 或秘密参数，env 只读。坏编码/空值/超长不覆盖原值。

- F 第一步：新增 model/core，保留经典/Lite wire/未知 item/event/opaque 与工具字符串；增量 SSE/UTF-8/换行/帧上限/终态/序号/响应身份验证。真实固定 Runtime 经典和 Lite 两轮请求/推理回放通过，仅合成回复。
- F 第二步（三平台已验）：model/gateway 只监听 loopback、随机独立访问 token、固定模型/endpoint/dialect、执行端 Broker 认证；SSE/非流式 JSON、单槽背压、真实断开/取消、超时、429/HTTP 错误分类和诊断脱敏。真实 Runtime 经典/Lite 各两轮通过，interrupt 在两条路径均实际关闭上游 socket；没有调用商业模型或执行 Code Mode 工具。
- F 第三步（三平台已验）：ModelProvider 六方法实际实现、CanonicalResponse/能力 Registry/版本化兼容性报告、独立 Custom Responses client。配置清单无自动发现/Full 标签，未知能力/上限不编造；Broker reference metadata 无秘密。HTTP-date、四项请求/两项响应 context header、单槽失败保存、取消/Drop/单并发 slot 释放、TLS 可信/未知 CA/错 hostname/过期 fixture 通过。
- F/G OpenAI 当前范围（三平台已验）：原生 Models GET、配置可调用模型交集、ProviderCatalog 证据门槛、scope/Broker、默认 store=false 与前台门控；原生两轮完整 output 回放、经典/Lite、Gateway 注入入口及独立 deadline/metadata/响应头门控。新 OpenAI 11/Gateway 注入 4 项各平台通过；真实 Runtime 新增 3 项，累计 Linux 23、Windows/macOS 各 22 项。

## 未完成

- iOS 已定义 SecretStore 接入契约，Swift Keychain/UniFFI 原生实现待 M。Gateway 已接入 Broker/诊断脱敏；UI/Host/同步尚未接入，不宣称未来全部输出通道已验证。
- F/G 其余能力及 H–R：其他原生 Provider/模型自动发现与实际兼容性报告、原生 opaque history 重建、生产持久化 Host、独立 Chat/同步、Windows/iOS UI、SSH、完整英文 CLI/attach、生产 Relay、签名/打包/UAT 未完成；未配置真实模型 Key 或调用商业模型。
- macOS Rust CI 不等于 iOS 应用构建；尚无 iOS 工程。其他原生工具/模型能力按后续相关阶段逐项补验收。

## 下一步顺序

1. 验收本轮 input_transformations，再实现请求前缀/执行端账户作用域历史校验，覆盖 mode/tool/trim/resume；保留 compiled system/tools/messages，接实际组织身份与执行端 beta opt-in（thinking-binding-controls-2026-08-01）。不能用 CredentialRef 代替账户身份或自动 drop/retry；缺省报告不能证明没丢块。核对原生 fallback 块的 serving model 身份，当前流式模型未随 fallback 更新。已验上下文头/请求参数/SSE/六方法不重做；生产 Host 尚待 canonical 请求与本地归属持久化。
2. 接 Gateway 及固定 Runtime 经典/Lite 多轮/工具/interrupt 验收。HTTP fixture 通过不等于完整 Adapter/商业模型支持；custom grammar 仅提示，未建立硬约束等价。
3. Gemini→兼容 API/Ollama，逐项验证原生请求/响应/工具/usage/reasoning/images/context/结构化输出与兼容性报告。Models 可用清单不能证明 Codex 兼容性，不猜 Full。
4. H/I→Windows→SSH/iOS→CLI→Relay→R，按 V3 验收；iOS simulator/无签名 archive 用 GitHub macOS runner。真实 Key/付费调用须明确授权；不重做 A–E 或重复请求公开授权。

## 重要架构决定

- Windows Tauri 2 + React/TypeScript/Rust；iOS SwiftUI（初期 Swift 5/iOS 17+）+ UniFFI 共享 Rust；CLI 继承上游英文体验；首批 Windows 11 x64、Linux x86_64 Host/CLI。
- 使用真实固定 Runtime，不造第二套 Agent；上游 commit `d27764b82f7118f674371e6d6e76271d9d606edb`，CLI `0.160.1`/tag `rust-v0.160.1`。协议实验 opt-in；常规 104 客户端/10 服务端/83 通知，实验 167/11/83。
- Lite 使用 /responses 与内部 header，指令/工具在 input 中，稳定前缀；core 仅保留收到的 wire。non-OpenAI 上游会清除 encrypted_function_args，未知签名也可能被 Runtime 丢弃；F/G Adapter 需验证原生 opaque history，必要时最小补丁，不宣称被删除的数据已保留。
- Gateway 为 Rust 嵌入库，可注入独立 ModelProvider，已有 Custom/原生 OpenAI；其他原生 Adapter 待接入。Runtime profile 同时禁用 request/stream 重试，token 只在隔离子进程环境。HTTPS 默认验证，TLS 本地正/负 fixture 已验；未验真实 Provider。已开始的同步 SecretStore 读取不能强行中止，取消后不 POST；HTTP-date 转安全秒数元数据、不重试。仅固定上游四项请求/两项响应 context headers；OpenAI org/project 单独来自固定执行端配置，不向客户端开放任意原生认证头。
- Anthropic 原生回复用版本化 CAIdex 载体放入 Runtime 已保留的 reasoning.encrypted_content，JSON 不代表加密；只有同 provider/准确 native model 及完整匹配投影组才能恢复。完整 native wire 是回放权威，不能从展示文字重建 signature。未知块/服务端工具不投影为 Runtime 客户端执行。跨 provider 不得原样转发，密文签名真实性仍由提供商校验；生产历史访问控制/同步按 H/I 落实。
- 普通 Chat 独立于 Codex Host，无 Shell/Git/项目写权限；按提供商实际能力展示工具。Remote 用执行 Host Key，手机不读已保存 Host Key；历史同步不含凭据。
- Gateway 面向 Responses HTTP/SSE，保留不透明签名/推理数据，不执行工具。经典 gpt-5.5 与 Lite/code_mode_only gpt-6.1-sol 路径不同，旧未知模型 fallback fixture 不能代表全部兼容性；F/G 分别验收。
- 模型切换在轮次边界生效；跨提供商关联分支/新线程承接适配历史。Host 独立运行、SQLite 事件持久化后广播、快照补缺口、请求幂等、审批首次有效处理；不承诺外部工具恰好执行一次。
- UI 尽量 1:1 参考官方，缺失部分自主补齐；用户品牌原件为准。CAIdex 模型/API Key 放“设置 → 模型与提供商”，清楚区分本机与 Host 归属。
- Remote 先 SSH 后 Relay，生产 Relay 首版成熟方案端到端加密；Host 切换不迁移活动线程。Chat 自动同步自托管 VPS，SQLite/outbox/cursor、冲突分支/删除 tombstone；HTTPS 服务端为信任边界，不承诺历史同步 E2EE。
- iOS 先测试/无签名构建，未进入 TestFlight；无签名产物不等于可安装 IPA。真机、UAC、签名留独立验收。

## 问题 / 阻塞

- 原自动审批额度问题已恢复，依赖下载/编译成功。正常沙箱注入 `/tmp/.git` 导致文件保护测试被拒绝；相同代码在获授权的正常执行环境通过，未削弱保护。
- `.git` 普通执行只读，提交/push 用 require_escalated。GitHub CLI 2.45.0 已核验登录 `bboytang`、scope repo/workflow；普通沙箱网络受限时 auth status 的 invalid 提示不能作为凭据失效证据。
- 旧 Git helper 缺 workflow scope。发布使用 `git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main`，不输出凭据或修改用户全局配置。
- 本机没有 Windows/Xcode/gnome-keyring-daemon，相关验证转 GitHub。无真实模型、客户端逐状态截图/真机验证；按计划保留明确验收项。
- Anthropic v2 载体仍未绑定 compiled system/tools/messages 和实际组织身份，须补 mode/tool/trim/resume。input_transformations 处理已实现、待最终验收；尚无报告 beta header opt-in，不能仅凭缺省/空报告授予兼容性证据。账户不匹配可静默丢 thinking；CredentialRef 是本地引用、同一引用可换 Key，不能当组织身份。官方 SDK 在 fallback 块更新 serving model，当前 MessageStream 未映射该行为，须在新历史绑定前补核对。依据见 Anthropic Provider 文档，不自动 drop_block/retry。

## 文件与 Git 状态

- `docs/CAIdex-实施计划-V3.md`、`docs/CAIdex-UI-规范-V1.md`、`docs/CAIdex-Runtime-能力对照.md`：执行与验收基准；原 V2 在 docs 归档。
- `credentials/core/`、`apps/cli/src/credentials.rs`、`apps/cli/tests/credentials.rs`、`docs/CAIdex-Credentials-设计与验收.md`：E 实现/回归/限制。
- `.github/workflows/ci.yml`、`scripts/test-linux-secret-service.sh`、Cargo.toml/lock：新增 E 三平台与 Linux 私有服务验收。
- `model/core/`、`model/providers/custom/`、`model/gateway/`、`docs/CAIdex-Model-Gateway-设计与验收.md`：F 协议/模型接口/Registry/共享 HTTP client；Runtime dev-dependency 接入 Gateway 两轮/取消回归，生产 Runtime 未改。
- `model/providers/openai/`、`model/gateway/tests/injected.rs`、`docs/CAIdex-OpenAI-Provider-设计与验收.md`：原生 Models/Responses、scope/store/前台限制、Gateway 注入的当前恢复点。
- `runtime/bridge/`、`upstream/codex/`：固定 Runtime/协议/回归；本轮新增经典/Lite wire 回归/fixture，生产 Runtime 未改；`assets/brand/`：四份原始品牌资产未改。
- branch `main` 跟踪 `origin/main`，上轮验收 6739642 已提交/push；本轮 input_transformations 改动：Anthropic message/projection/stream/response_stream、对应协议/投影/HTTP 测试及两份交接/设计文档，将随本轮源码提交/push。实现与本地检查已完成，CI 验收待完成；没有请求前缀/账户绑定源码改动。恢复按当前任务验收后继续下一步。

## 测试结果

- F 共享 core 20/Custom 7/Gateway HTTP 18/注入 4 项已三平台通过；凭据 native/文件/环境、CLI、固定 schema/doctor 回归已验。能力与限制详见对应设计/验收文档。
- OpenAI [CI 37573720266](https://github.com/bboytang/CAIdex/actions/runs/37573720266)，源码 e7253c8：三平台 OpenAI 11 项及经典/Lite 两轮/interrupt fixture 通过；未调用商业 API。
- Anthropic 中途指令 [CI 37581359918](https://github.com/bboytang/CAIdex/actions/runs/37581359918)，源码 2ccbe38：三平台请求 12/HTTP 14/协议 12/投影 7/工具 7 项，workspace/fmt/Clippy/native keyring/schema/doctor 全部 success；既有真实 Runtime Linux 25、Windows/macOS 24 项通过。
- Runtime Anthropic 载体执行覆盖 v1；v2 工具/基础请求/中途指令用 canonical wire 及真实 HTTP fixture 验证，不代表完整 Anthropic Gateway/Runtime 工具执行、Code Mode 或商业 API 已验。
- 本轮 effort/thinking：新增推理 5 项、既有请求 12 项及扩展 HTTP 经典/Lite 两轮全部通过；完整 workspace、workspace Clippy/fmt/diff 通过。日志 /tmp/caidex-reasoning-workspace.log；[CI 37582398993](https://github.com/bboytang/CAIdex/actions/runs/37582398993) 三平台全部 success：各平台推理 5/请求 12/HTTP 14/协议 12/投影 7/工具 7 项通过，workspace/fmt/Clippy/native keyring/schema/doctor 通过，既有 Runtime Linux 25、Windows/macOS 24 项通过；日志 /tmp/caidex-ci-37582398993.log。
- Linux 私有 Secret Service fixture 曾被 readiness Ping 自动激活另一服务，已改 NameHasOwner+default collection Locked=false（1fc0d3c），之后三平台 CI 原生凭据通过。不要恢复自动激活 Ping 或削弱生产凭据检查。本机没有 gnome-keyring-daemon，由 CI 验证。
- 本机文件保护回归使用 TMPDIR=/var/tmp；保留 Git 存储拒绝。所有秘密/推理为合成 fixture，不读用户模型 Key。跨机器以 GitHub 链接为准，本地 /tmp 日志可丢失。
- iOS/商业 API/客户端逐状态 UI/真机/UAC/签名未验；macOS Rust CI 不等于 iOS 构建。完整客户端和 H–R 尚未完成。

- 结构化转换 [CI 37583615309](https://github.com/bboytang/CAIdex/actions/runs/37583615309)，源码 SHA 9796682e13cb50e7bf52f3581b543b240eacdbd0：Windows/Linux/macOS 全部 completed/success，逐平台日志核对新增 4 项通过；本地 workspace/Clippy/fmt/diff 通过。日志 /tmp/caidex-structured-workspace.log、/tmp/caidex-ci-37583615309.log。strict:true 阶段源码与验收文档均已提交/push。

- Summary/context [CI 37610039312](https://github.com/bboytang/CAIdex/actions/runs/37610039312)，源码 SHA 024b211bbd6ae31ff0160ca3995ed75a4eadae5d：三平台全部 completed/success，逐平台日志核对新增 3 项与扩展 HTTP fixture 通过；累计推理 8/请求 12/结构化 4/HTTP 14/协议 12/投影 7/工具 7 项，本地完整 workspace/Clippy/fmt/diff 通过。日志 /tmp/caidex-summary-context-workspace.log、/tmp/caidex-ci-37610039312.log。上一步自动审核额度失败未执行完整测试，本次已补验，不存在该测试遗留任务。

- Runtime 字段 [CI 37611149491](https://github.com/bboytang/CAIdex/actions/runs/37611149491)，源码 SHA 1afee186b3162a2d534438eee9e9cbd25bd81832：三平台全部 completed/success，日志核对新增 3 项及扩展 HTTP 两轮逐平台通过；workspace/fmt/Clippy/native keyring/schema/doctor 与既有 Runtime 回归通过。本地日志 /tmp/caidex-runtime-parameters-workspace.log、/tmp/caidex-ci-37611149491.log。未调用商业 API，尚未实际 Anthropic Runtime 工具执行。

- SSE [CI 37613084679](https://github.com/bboytang/CAIdex/actions/runs/37613084679)，源码 SHA 6a3986a0b705f3e8ce0937c9def712bd906b32cf：三平台 success，新增投影 4/HTTP 2 逐平台日志通过；本地 workspace/Clippy/fmt/diff 通过。日志 /tmp/caidex-response-stream-workspace.log、/tmp/caidex-ci-37613084679.log。
- ModelProvider [CI 37614432384](https://github.com/bboytang/CAIdex/actions/runs/37614432384)，源码 SHA 6ae83b96b29d7767de46c5052b595c6f3a53b375：三平台 success，新增 6 项逐平台日志通过（HTTP 累计 22），workspace/fmt/Clippy/native keyring/schema/doctor 全过，既有真实 Runtime Linux 25、Windows/macOS 24 项通过。日志 /tmp/caidex-anthropic-provider-workspace.log、/tmp/caidex-ci-37614432384.log；末次仅增强测试后 HTTP/Clippy 本地复验通过。仍未做实际 Anthropic Gateway/Runtime 工具执行或商业 API 验收。
- 本地上下文/响应关联 [CI 37616230221](https://github.com/bboytang/CAIdex/actions/runs/37616230221)，源码 SHA f35d2a7ba6f53794d20b9c8cf9d07b918231a98d：三平台 completed/success，新增 4 项及扩展流式/配置测试逐平台日志核对通过（HTTP 26），包含 Lite-only/Lite-first 初始化回归。workspace/fmt/Clippy/native keyring/schema/doctor 及既有真实 Runtime Linux 25、Windows/macOS 24 项通过。本地完整 workspace/Clippy/fmt/diff 通过；日志 /tmp/caidex-anthropic-context-workspace.log、/tmp/caidex-ci-37616230221.log。请求前缀/账户历史、完整 Anthropic Gateway/Runtime 工具执行与商业 API 尚未验收。
- 本轮 input_transformations：新增 5 项本地通过（协议 13/投影 8/增量投影 5/HTTP 28），完整 workspace/Clippy/fmt/diff 通过；最终 null 保留初始报告修正后已完整复验，日志 /tmp/caidex-anthropic-input-bindings-workspace.log。三平台 CI 待验；未调用商业 API，完整前缀/账户绑定与 Anthropic Gateway/Runtime 工具执行未验。
