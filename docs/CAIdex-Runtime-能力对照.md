# CAIdex Runtime 能力对照

基准：Codex 0.160.1，commit `d27764b82f7118f674371e6d6e76271d9d606edb`。执行计划 D 阶段的接口/验收清单，后续每项按实测更新。固定源码与 schema 优先于随版本变化的在线说明。

## 边界与证据

- `Runtime::connect` 执行 initialize → initialized；`RuntimeClient` 提供线程/turn 常用操作及完整锁定协议 `call(method, Option<Value>, deadline)`。无 params 方法保留省略语义；其他参数和结果使用 JSON 保留全部字段。UI 不依赖上游内部 Rust 类型。
- 客户端方法清单直接读取归档 schema：常规 104 个，含实验 167 个；服务端请求 10/11 个，通知 83 个。实验方法必须显式 opt-in；字段级门控由上游检查。清单存在不代表 Host、插件或模型实际支持，initialize 返回值也不是能力位图。
- 服务端请求按原始整数/字符串 ID 管理，不能使用 itemId/approvalId 代替；无自动批准、自动调用工具或自动刷新凭据。所有 11 类及未知请求都转交上层，保留原始信封。
- `decide_approval` 支持常见命令/文件决策并核对 availableDecisions；持久命令/网络 policy amendment、权限 grant、MCP elicitation、动态工具等走显式 `reply` JSON/RPC-error 入口。原始 reply 的具体语义由调用方与上游校验，不能将其当作通用默认批准按钮。
- 用户输入按 question ID 回答，保留 isBlocking/isSecret/options 等提示信息；回答不进入日志。resolved 通知、turn 完成/中断、thread 关闭/删除撤销相应待回应状态；遗留 conversationId 也用于线程归属。
- 本连接上一个请求只允许一次回应尝试；写失败或取消表示结果未知，不重试。多客户端/跨重启幂等和审批竞争依赖 H 阶段生产 Host，当前局部状态不等于该验收完成。
- 新增接口回归与原有传输回归共 19 项。真实 Runtime 测试使用脚本化 loopback Responses SSE，不连接商业提供商，也不读取用户配置/Key；这些测试证明真实 Runtime 处理链路，不证明真实模型或其他 Provider 兼容。
- 上次已验收真实 Linux 用例 5 项：消息/usage/历史/resume/fork；活动轮次原生 Queue CRUD 与审批取消；批准后实际执行临时标记命令并回传工具结果；Plan request_user_input；Steer 前置条件、interrupt 及过期审批撤销。CI 显式执行 ignored 集成测试；Windows/macOS 不执行 Linux 专用批准命令用例。

- [三平台 CI 37523663697](https://github.com/bboytang/CAIdex/actions/runs/37523663697)，代码基准 `5a593a4`：全部 success。Linux 15 项、Windows/macOS 各 14 项真实 Runtime 集成通过；19 项协议回归、常规/实验指纹与 doctor 均通过。归档 bundle 固定 LF，测试服务不依赖反向 DNS。

- 此前新增（三平台已验证）：patch accept/cancel 两项；MCP stdio 发现/资源读取/工具调用/form accept/decline/cancel 一项；PTY 输入/resize/UTF-8 一项；长运行进程 duplicate handle/kill/过期输入一项。当时累计 Linux 10 项、Windows/macOS 各 9 项。

- 本轮续接（三平台通过）：Queue 排序/分页/busy/中断恢复/指定启动、Goal 暂停/激活/预算/clear/空回复 breaker、经典手动压缩与后续摘要承接通过；空闲 Queue add 自动启动亦通过；新增 5 项全部通过；累计 Linux 15 项、Windows/macOS 各 14 项。

- F/G 后续验收：[CI 37573720266](https://github.com/bboytang/CAIdex/actions/runs/37573720266)，源码 e7253c8，三平台通过；累计真实 Runtime Linux 23、Windows/macOS 各 22 项。新增原生 OpenAI Adapter 经典/Lite 两轮、两路径 interrupt 关闭上游 socket，与此前直接/Custom Gateway wire 验证均保持通过；仍使用合成回复，不代表商业推理或 Code Mode 工具执行。详见 [原生 Provider 验收](CAIdex-OpenAI-Provider-设计与验收.md)。

- Anthropic 接线当前离线范围三平台通过：Lite两轮v3/落盘、Code Mode真实审批后临时执行/结果回放、interrupt socket；经典显式禁用网页后真实注册表发现→原生 inline→MCP执行、实际app-server重启/disk resume。默认经典cached网页明确失败且不读Key/不POST；不是网页支持正例。本地完整 Runtime30通过，无 skip；源码b403b0d的CI37653194672仅Lite两轮路径词法断言失败，目录别名复现RED→GREEN后以canonicalize两方检查隔离；修复源码e27e9035eb41fec89eeda5dab43b1fd10640ed38的[CI37654366666](https://github.com/bboytang/CAIdex/actions/runs/37654366666)三平台completed/success，实际Runtime Linux30/WindowsmacOS29、HTTP50及workspace/fmt/Clippy/native credentials/schema/doctor通过，商业模型/生产Host未验。详见 [Anthropic 作用域与验收](CAIdex-Anthropic-Provider-设计与验收.md)。

- DeepSeek实际Classic/Lite本地接线新增7项、累计Runtime50项通过：真实审批/隔离执行/磁盘resume不重跑、多调用与默认门控拒绝、stream/待审批取消、未提供Decline拒绝且Cancel按真实决策中断。新源码1b501ba/[CI37886226991](https://github.com/bboytang/CAIdex/actions/runs/37886226991)三平台已精确验收，Runtime50/49/49、workspace438/433/437及全部新旧通过名核对正确；不改生产Runtime，也不认领H持久Host或商业模型Full；见[DeepSeek当前验收](CAIdex-DeepSeek-Provider-设计与验收.md)。

## D 边界验收

- 已实现的完整请求入口、11 类服务端请求与未知事件/扩展字段保留，由 19 项协议回归验证；实验方法显式门控、审批显式回应、无自动重试。
- 真实固定 app-server 的线程/轮次、审批、用户输入、MCP、进程、Queue、Goal 与经典压缩链路已在三平台验证；CLI 对照明确 doctor 当前入口和 P 阶段整合项。
- 达到 V3 D 的接口边界验收，进入 E。下表保留待验收能力，分别由 F/G 的真实模型/协议兼容、H 的持久 Host、K/O 的客户端端到端及 P 的 CLI 集成继续完成。

## 已核实的交互约束

- 审批 UI 采用请求的 availableDecisions。当前测试的 require_escalated 命令不提供 decline，可提供 cancel；取消结束轮次，批准允许执行。模拟用例覆盖 decline，不冒称该具体真实提示提供所有四个按钮。
- 空闲线程添加 Queue 可能立刻触发新轮次。活动轮次中添加后可 list/update/delete；不要在客户端实现另一套队列调度或假定 add 永远只存草稿。
- interrupt 成功是请求被接受，最终状态以 turn/completed 为准。固定上游取消线程回调时不一定发出逐项 resolved 通知，所以终止轮次也必须清理该线程的待回应请求。

- 不同模型元数据改变可用工具与 wire：固定 bundled gpt-5.5 声明经典 Responses/freeform patch；gpt-6.1-sol 等声明 `use_responses_lite=true`、`tool_mode=code_mode_only`。本轮 patch 明确使用前者的本地元数据，不调用商业模型；原 gpt-5.1-codex fixture 为未知模型 fallback。F/G 必须分别验证经典与 Lite/Code Mode，不能以旧 fallback 测试覆盖整个能力范围。
- 补丁 cancel 进入 interrupted，可能只有 fileChange started 而无 item/completed；UI 应依据轮次终态结束交互，不能一直等待不存在的 item 完成通知。
- process/* 为连接范围的 Host 进程接口，不自动归属于某个线程，也不能据此宣称生产 Host 断连恢复已实现。

- Queue reorder 要求每个现存 submission ID 恰好出现一次；busy start 不消费条目；中断后队列暂停，显式 start 可选非队首。用户消息事件的 `clientId` 对应队列 `clientUserMessageId`。
- Goal 另需 `features.goals`，不是 initialize 的实验 opt-in 就能保证支持。active 可自动开始轮次；负预算拒绝；budgetLimited/blocked 是上游状态。blocked 更新可先于最后一轮 turn/completed，UI 分别处理目标与轮次终态。
- 手动 compact/start 返回 `{}` 只代表请求受理；最终依据 contextCompaction item 与 turn/completed。原有经典本地摘要证据保持；2026-10-10新增Lite本地摘要和Classic/Lite远端v2 opaque压缩/磁盘恢复本地通过，精确三平台CI已验，详下文。

## 原 V2 原生能力逐项验收

| 能力 | 已实现或已验证 | 后续验收 |
| --- | --- | --- |
| Agent lifecycle | 真实启动、消息轮次、中断、完成事件 | 多 Host/后台生命周期 H/K/O |
| Shell | Linux 批准后实际执行临时命令及工具结果回传 | Windows 执行、真实模型选择 |
| Unified exec | 真实上游 exec_command 测试工具链 | 长运行、重启及不同平台 |
| PTY | 三平台真实 process PTY stdin/resize/UTF-8 字节/exit 通过 | 工具级 PTY 与平台特殊情况 |
| Long-running processes | 三平台真实进程存活、重复句柄拒绝、显式 kill/过期输入拒绝通过 | 工具级背景进程与 Host 重启 |
| apply_patch | 三平台真实 freeform patch accept/cancel 通过，实际临时目标/changes 事件核对 | 其他路径/文件类型与真实模型 |
| Filesystem | 全 fs 方法保留 | 真实读写/watch/平台权限 |
| Git | 上游工具/command 通道未替换 | worktree、状态、commit/diff 端到端 |
| Sandbox | 真实 read-only 线程与用户审批 | 真实 Windows sandbox/UAC、权限边界 |
| Approval | 原 ID、availableDecisions、局部一次回应、真实 accept/cancel | 跨客户端竞争与持久化 H |
| Network approval | 复杂 decision 原样 reply | 实际 network policy amendment |
| MCP | 三平台真实 stdio MCP 握手/发现、resource read、tool call、structuredContent/_meta 通过 | HTTP/OAuth/stream、失败恢复 |
| Plugins / Apps | 全 marketplace/plugin/app 方法保留 | 安装/移除、缺依赖、执行权限 |
| Skills | list/config/read 方法及技能输入保留 | 发现、启用/禁用和实际选择 |
| Tool auto-selection | 工具仍由真实 Runtime 执行 | 真实模型选择；fixture 不做推理 |
| requestUserInput | 真实 Plan 问题→答案→工具结果链路通过 | 前端交互、非阻塞/secret/超时 |
| MCP elicitation | 三平台真实 MCP form accept/decline/cancel 均显式处理 | url/富表单/UI 验证 |
| Context compaction | 三平台Classic/Lite本地摘要、远端opaque/重启及两分支失败/取消/磁盘恢复已验 | 自动阈值、商业模型和生产Host等独立范围，详下文 |
| Interrupt | 真实 Steer 中断、终态和审批撤销通过 | 多客户端恢复前台后的状态核对 |
| Resume | 真实存储历史及已加载线程 resume | 进程/机器重启恢复 H |
| Queue | 三平台 CRUD/reorder/分页/busy/中断保留/指定及默认/自动启动通过 | 多端与持久 Host H |
| Steer | 真实 expectedTurnId 前置条件与已有轮次输入通过 | 多端竞争、跨模型轮次边界 |
| Diff | turn diff 与 file patch 通知保留 | 真实 diff/review 与 UI 展示 |
| Plan | Plan collaborationMode、输入工具通过；plan 通知保留 | 真实 plan 输出与 UI |
| Goal | 三平台 paused/active、预算耗尽、clear、三次空回复 blocked 通过 | 真实模型推进及多端 H |
| Sub-agent | Runtime 配置及工具事件保留 | 原生 delegation 与生命周期 |
| Tool result handling | 真实命令、用户输入、patch、MCP 结果回送；opaque 字段不丢失 | 图像等结果及 Provider 对照 |
| Usage tracking | 真实 tokenUsage 通知与 Goal usage/预算限制通过（脚本化数值） | 真实计费/限流/Provider usage |
| Session state | 线程元数据/历史读取与通知保留 | Host journal/snapshot 与跨设备 H |
| Thread persistence | 真实临时数据目录内历史/resume/fork通过 | durable Host、重启不盲重跑工具 |

## CLI 对照

固定二进制 `codex --help` 已核对，CAIdex CLI当前仅doctor、credentials status/set/remove、help/version。TUI/exec/remote attach等在P阶段整合，不能将协议保留标为CLI命令已实现；正式命名/认证/记忆/审批契约见[CLI独立规范](CAIdex-CLI-完整交互与验收规范-V1.md)。

| 上游入口 | 当前 CAIdex 与后续计划 |
| --- | --- |
| 无子命令交互 TUI、--remote、--remote-auth-token-env | P：保留上游英文，明确共享 Host/remote attach |
| agents、exec、review、resume、queue、archive、delete、unarchive、fork | D 协议入口保留；P 原生命令接入待实现 |
| login、logout、mcp、plugin、features | E/F及P：模型凭据/原生管理；CAIdex login/logout正式只管理CAIdex Account，不透传上游认证；MCP自身OAuth与账户独立 |
| app-server、remote-control、exec-server | B/D 基础协议；H/L/Q/P 接入真实 Host/Remote |
| sandbox、apply、--worktree | K/P：平台 sandbox、真实 Git/worktree 验收 |
| completion、doctor、debug | CAIdex 自有基础 doctor 已实现，其余原生入口 P 对照 |
| update、migrate-rollouts、cloud | P/R：版本锁定迁移/升级与可用性检查 |

## 固定客户端请求清单

“真实链路”仅指上述本地 Responses 用例或离线 doctor，其他行仅有协议入口；尚未执行功能验收。字段级实验要求和请求/响应结构见完整 schema。

| 方法 | API 门控 | 验证状态 |
| --- | --- | --- |

| `account/bedrock/discover` | 实验 opt-in | 协议保留，待验收 |
| `account/bedrock/setup` | 实验 opt-in | 协议保留，待验收 |
| `account/gatewayOAuth/cancel` | 常规 | 协议保留，待验收 |
| `account/gatewayOAuth/login` | 常规 | 协议保留，待验收 |
| `account/gatewayOAuth/read` | 常规 | 协议保留，待验收 |
| `account/login/cancel` | 常规 | 协议保留，待验收 |
| `account/login/start` | 常规 | 协议保留，待验收 |
| `account/logout` | 常规 | 协议保留，待验收 |
| `account/rateLimitResetCredit/consume` | 常规 | 协议保留，待验收 |
| `account/rateLimits/read` | 常规 | 协议保留，待验收 |
| `account/read` | 常规 | 协议保留，待验收 |
| `account/sendAddCreditsNudgeEmail` | 常规 | 协议保留，待验收 |
| `account/usage/read` | 常规 | 协议保留，待验收 |
| `account/workspaceMessages/read` | 常规 | 协议保留，待验收 |
| `app/installed` | 常规 | 协议保留，待验收 |
| `app/list` | 常规 | 协议保留，待验收 |
| `app/read` | 常规 | 协议保留，待验收 |
| `collaborationMode/list` | 实验 opt-in | 协议保留，待验收 |
| `command/exec` | 常规 | 协议保留，待验收 |
| `command/exec/resize` | 常规 | 协议保留，待验收 |
| `command/exec/terminate` | 常规 | 协议保留，待验收 |
| `command/exec/write` | 常规 | 协议保留，待验收 |
| `config/batchWrite` | 常规 | 协议保留，待验收 |
| `config/mcpServer/reload` | 常规 | 协议保留，待验收 |
| `config/read` | 常规 | 协议保留，待验收 |
| `config/value/write` | 常规 | 协议保留，待验收 |
| `configRequirements/read` | 常规 | 协议保留，待验收 |
| `environment/add` | 实验 opt-in | 协议保留，待验收 |
| `environment/info` | 实验 opt-in | 协议保留，待验收 |
| `environment/status` | 实验 opt-in | 协议保留，待验收 |
| `experimentalFeature/enablement/set` | 常规 | 协议保留，待验收 |
| `experimentalFeature/list` | 常规 | 协议保留，待验收 |
| `externalAgentConfig/detect` | 常规 | 协议保留，待验收 |
| `externalAgentConfig/import` | 常规 | 协议保留，待验收 |
| `externalAgentConfig/import/readHistories` | 常规 | 协议保留，待验收 |
| `externalAgentConfig/import/recordHistory` | 常规 | 协议保留，待验收 |
| `feedback/upload` | 常规 | 协议保留，待验收 |
| `fs/copy` | 常规 | 协议保留，待验收 |
| `fs/createDirectory` | 常规 | 协议保留，待验收 |
| `fs/getMetadata` | 常规 | 协议保留，待验收 |
| `fs/readDirectory` | 常规 | 协议保留，待验收 |
| `fs/readFile` | 常规 | 协议保留，待验收 |
| `fs/remove` | 常规 | 协议保留，待验收 |
| `fs/unwatch` | 常规 | 协议保留，待验收 |
| `fs/watch` | 常规 | 协议保留，待验收 |
| `fs/writeFile` | 常规 | 协议保留，待验收 |
| `fuzzyFileSearch` | 常规 | 协议保留，待验收 |
| `fuzzyFileSearch/sessionStart` | 实验 opt-in | 协议保留，待验收 |
| `fuzzyFileSearch/sessionStop` | 实验 opt-in | 协议保留，待验收 |
| `fuzzyFileSearch/sessionUpdate` | 实验 opt-in | 协议保留，待验收 |
| `hooks/list` | 常规 | 协议保留，待验收 |
| `initialize` | 常规 | 真实链路 |
| `marketplace/add` | 常规 | 协议保留，待验收 |
| `marketplace/remove` | 常规 | 协议保留，待验收 |
| `marketplace/upgrade` | 常规 | 协议保留，待验收 |
| `mcpServer/event/stream/start` | 实验 opt-in | 协议保留，待验收 |
| `mcpServer/event/stream/stop` | 实验 opt-in | 协议保留，待验收 |
| `mcpServer/oauth/login` | 常规 | 协议保留，待验收 |
| `mcpServer/resource/read` | 常规 | 三平台真实链路 |
| `mcpServer/tool/call` | 常规 | 三平台真实链路 |
| `mcpServerStatus/list` | 常规 | 三平台真实链路 |
| `memory/reset` | 实验 opt-in | 协议保留，待验收 |
| `memory/status` | 实验 opt-in | 协议保留，待验收 |
| `mock/experimentalMethod` | 实验 opt-in | 协议保留，待验收 |
| `model/list` | 常规 | 协议保留，待验收 |
| `modelProvider/capabilities/read` | 常规 | 协议保留，待验收 |
| `permissionProfile/list` | 常规 | 协议保留，待验收 |
| `plugin/install` | 常规 | 协议保留，待验收 |
| `plugin/installed` | 常规 | 协议保留，待验收 |
| `plugin/list` | 常规 | 协议保留，待验收 |
| `plugin/read` | 常规 | 协议保留，待验收 |
| `plugin/reconcile` | 常规 | 协议保留，待验收 |
| `plugin/search` | 实验 opt-in | 协议保留，待验收 |
| `plugin/share/checkout` | 常规 | 协议保留，待验收 |
| `plugin/share/delete` | 常规 | 协议保留，待验收 |
| `plugin/share/list` | 常规 | 协议保留，待验收 |
| `plugin/share/save` | 常规 | 协议保留，待验收 |
| `plugin/share/updateTargets` | 常规 | 协议保留，待验收 |
| `plugin/skill/read` | 常规 | 协议保留，待验收 |
| `plugin/uninstall` | 常规 | 协议保留，待验收 |
| `process/kill` | 实验 opt-in | 三平台真实链路 |
| `process/resizePty` | 实验 opt-in | 三平台真实链路 |
| `process/spawn` | 实验 opt-in | 三平台真实链路 |
| `process/writeStdin` | 实验 opt-in | 三平台真实链路 |
| `project/create` | 实验 opt-in | 协议保留，待验收 |
| `project/delete` | 实验 opt-in | 协议保留，待验收 |
| `project/import` | 实验 opt-in | 协议保留，待验收 |
| `project/list` | 实验 opt-in | 协议保留，待验收 |
| `project/move` | 实验 opt-in | 协议保留，待验收 |
| `project/read` | 实验 opt-in | 协议保留，待验收 |
| `project/update` | 实验 opt-in | 协议保留，待验收 |
| `remoteControl/client/list` | 实验 opt-in | 协议保留，待验收 |
| `remoteControl/client/revoke` | 实验 opt-in | 协议保留，待验收 |
| `remoteControl/disable` | 实验 opt-in | 协议保留，待验收 |
| `remoteControl/enable` | 实验 opt-in | 协议保留，待验收 |
| `remoteControl/pairing/start` | 实验 opt-in | 协议保留，待验收 |
| `remoteControl/pairing/status` | 实验 opt-in | 协议保留，待验收 |
| `remoteControl/status/read` | 实验 opt-in | 协议保留，待验收 |
| `review/start` | 常规 | 协议保留，待验收 |
| `rollout/compress` | 实验 opt-in | 协议保留，待验收 |
| `server/diagnostics` | 实验 opt-in | 协议保留，待验收 |
| `skills/config/write` | 常规 | 协议保留，待验收 |
| `skills/extraRoots/set` | 常规 | 协议保留，待验收 |
| `skills/list` | 常规 | 协议保留，待验收 |
| `thread/approveGuardianDeniedAction` | 常规 | 协议保留，待验收 |
| `thread/archive` | 常规 | 协议保留，待验收 |
| `thread/attachment/add` | 常规 | 协议保留，待验收 |
| `thread/attachment/list` | 常规 | 协议保留，待验收 |
| `thread/attachment/remove` | 常规 | 协议保留，待验收 |
| `thread/backgroundTerminals/clean` | 实验 opt-in | 协议保留，待验收 |
| `thread/backgroundTerminals/list` | 实验 opt-in | 协议保留，待验收 |
| `thread/backgroundTerminals/terminate` | 实验 opt-in | 协议保留，待验收 |
| `thread/compact/start` | 常规 | 三平台真实链路 |
| `thread/decrement_elicitation` | 实验 opt-in | 协议保留，待验收 |
| `thread/delete` | 常规 | 协议保留，待验收 |
| `thread/fork` | 常规 | 真实链路 |
| `thread/goal/clear` | 常规 | 三平台真实链路 |
| `thread/goal/get` | 常规 | 三平台真实链路 |
| `thread/goal/set` | 常规 | 三平台真实链路 |
| `thread/increment_elicitation` | 实验 opt-in | 协议保留，待验收 |
| `thread/inject_items` | 常规 | 协议保留，待验收 |
| `thread/items/list` | 常规 | 协议保留，待验收 |
| `thread/list` | 常规 | 协议保留，待验收 |
| `thread/loaded/list` | 常规 | 真实链路 |
| `thread/memoryMode/set` | 实验 opt-in | 协议保留，待验收 |
| `thread/metadata/update` | 常规 | 协议保留，待验收 |
| `thread/name/set` | 常规 | 协议保留，待验收 |
| `thread/queue/add` | 实验 opt-in | 真实链路 |
| `thread/queue/delete` | 实验 opt-in | 真实链路 |
| `thread/queue/list` | 实验 opt-in | 真实链路 |
| `thread/queue/reorder` | 实验 opt-in | 三平台真实链路 |
| `thread/queue/start` | 实验 opt-in | 三平台真实链路 |
| `thread/queue/update` | 实验 opt-in | 真实链路 |
| `thread/read` | 常规 | 真实链路 |
| `thread/realtime/appendAudio` | 实验 opt-in | 协议保留，待验收 |
| `thread/realtime/appendSpeech` | 实验 opt-in | 协议保留，待验收 |
| `thread/realtime/appendText` | 实验 opt-in | 协议保留，待验收 |
| `thread/realtime/listVoices` | 实验 opt-in | 协议保留，待验收 |
| `thread/realtime/start` | 实验 opt-in | 协议保留，待验收 |
| `thread/realtime/stop` | 实验 opt-in | 协议保留，待验收 |
| `thread/resume` | 常规 | 真实链路 |
| `thread/revert` | 常规 | 协议保留，待验收 |
| `thread/search` | 实验 opt-in | 协议保留，待验收 |
| `thread/searchOccurrences` | 实验 opt-in | 协议保留，待验收 |
| `thread/section/move` | 常规 | 协议保留，待验收 |
| `thread/settings/update` | 实验 opt-in | 协议保留，待验收 |
| `thread/shellCommand` | 常规 | 协议保留，待验收 |
| `thread/start` | 常规 | 真实链路 |
| `thread/timeline/list` | 实验 opt-in | 协议保留，待验收 |
| `thread/turns/list` | 常规 | 协议保留，待验收 |
| `thread/unarchive` | 常规 | 协议保留，待验收 |
| `thread/unsubscribe` | 常规 | 协议保留，待验收 |
| `threadSection/create` | 常规 | 协议保留，待验收 |
| `threadSection/delete` | 常规 | 协议保留，待验收 |
| `threadSection/list` | 常规 | 协议保留，待验收 |
| `threadSection/update` | 常规 | 协议保留，待验收 |
| `turn/interrupt` | 常规 | 真实链路 |
| `turn/settings/update` | 实验 opt-in | 协议保留，待验收 |
| `turn/start` | 常规 | 真实链路 |
| `turn/steer` | 常规 | 真实链路 |
| `userVerification/cancel` | 实验 opt-in | 协议保留，待验收 |
| `userVerification/delete` | 实验 opt-in | 协议保留，待验收 |
| `userVerification/enroll` | 实验 opt-in | 协议保留，待验收 |
| `userVerification/status` | 实验 opt-in | 协议保留，待验收 |
| `userVerification/verify` | 实验 opt-in | 协议保留，待验收 |
| `windowsSandbox/readiness` | 常规 | 协议保留，待验收 |
| `windowsSandbox/setupStart` | 常规 | 协议保留，待验收 |


## 固定服务端请求清单

下表全部按 ID 转交，不自动回应。具体 JSON response 结构见归档 schema。

| 方法 | 门控 | 验证 |
| --- | --- | --- |
| `account/chatgptAuthTokens/refresh` | 常规 | 转交回归；实际功能待验收 |
| `applyPatchApproval` | 常规 | 转交回归；实际功能待验收 |
| `attestation/generate` | 常规 | 转交回归；实际功能待验收 |
| `currentTime/read` | 实验 | 转交回归；实际功能待验收 |
| `execCommandApproval` | 常规 | 转交回归；实际功能待验收 |
| `item/commandExecution/requestApproval` | 常规 | 真实链路 |
| `item/fileChange/requestApproval` | 常规 | 三平台真实链路 |
| `item/permissions/requestApproval` | 常规 | 转交回归；实际功能待验收 |
| `item/tool/call` | 常规 | 转交回归；实际功能待验收 |
| `item/tool/requestUserInput` | 常规 | 真实链路 |
| `mcpServer/elicitation/request` | 常规 | 三平台真实链路 |


## 固定通知清单

所有通知保留完整原始 JSON，未知通知也转交。

- `account/gatewayOAuth/changed`
- `account/login/completed`
- `account/rateLimits/updated`
- `account/updated`
- `app/list/updated`
- `autoApprovalReview/strictReviewRequired`
- `command/exec/outputDelta`
- `configWarning`
- `deprecationNotice`
- `error`
- `externalAgentConfig/import/completed`
- `externalAgentConfig/import/progress`
- `fs/changed`
- `fuzzyFileSearch/sessionCompleted`
- `fuzzyFileSearch/sessionUpdated`
- `guardianWarning`
- `hook/completed`
- `hook/started`
- `item/agentMessage/delta`
- `item/autoApprovalReview/completed`
- `item/autoApprovalReview/started`
- `item/commandExecution/outputDelta`
- `item/commandExecution/terminalInteraction`
- `item/completed`
- `item/fileChange/outputDelta`
- `item/fileChange/patchUpdated`
- `item/mcpToolCall/progress`
- `item/plan/delta`
- `item/reasoning/summaryPartAdded`
- `item/reasoning/summaryTextDelta`
- `item/reasoning/textDelta`
- `item/started`
- `mcpServer/event/stream/notification`
- `mcpServer/oauthLogin/completed`
- `mcpServer/startupStatus/updated`
- `model/rerouted`
- `model/safetyBuffering/updated`
- `model/verification`
- `modelProvider/authRecoveryCompleted`
- `modelProvider/authRecoveryStarted`
- `process/exited`
- `process/outputDelta`
- `project/changed`
- `remoteControl/status/changed`
- `serverRequest/resolved`
- `skills/changed`
- `thread/archived`
- `thread/attachment/updated`
- `thread/closed`
- `thread/compacted`
- `thread/deleted`
- `thread/environment/connected`
- `thread/environment/disconnected`
- `thread/goal/cleared`
- `thread/goal/updated`
- `thread/name/updated`
- `thread/project/updated`
- `thread/queue/changed`
- `thread/realtime/closed`
- `thread/realtime/error`
- `thread/realtime/item/completed`
- `thread/realtime/item/started`
- `thread/realtime/item/transcript/delta`
- `thread/realtime/itemAdded`
- `thread/realtime/outputAudio/delta`
- `thread/realtime/sdp`
- `thread/realtime/started`
- `thread/realtime/transcript/delta`
- `thread/realtime/transcript/done`
- `thread/reverted`
- `thread/settings/updated`
- `thread/started`
- `thread/status/changed`
- `thread/tokenUsage/updated`
- `thread/unarchived`
- `turn/completed`
- `turn/diff/updated`
- `turn/moderationMetadata`
- `turn/plan/updated`
- `turn/started`
- `warning`
- `windows/worldWritableWarning`
- `windowsSandbox/setupCompleted`


## 来源与复现

- [固定上游源码](https://github.com/openai/codex/tree/d27764b82f7118f674371e6d6e76271d9d606edb)，schema 与 LICENSE/NOTICE 见 `upstream/codex/`。
- [官方 app-server 说明](https://learn.chatgpt.com/docs/app-server)：初始化、事件及双向请求规则；页面会随版本更新。
- [固定版本事件处理](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server/src/bespoke_event_handling.rs)、[回调取消实现](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server/src/outgoing_message.rs)：终止轮次取消线程请求的依据。
- [固定版本 SSE 测试格式](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/tests/common/responses.rs)，本地服务只用于测试，不作为生产 Gateway。

```sh
cargo test --workspace --locked
node scripts/verify-codex-schema.mjs
cargo run -p caidex-cli --locked -- doctor
cargo test -p caidex-runtime --test real_runtime --locked -- --ignored
```

本机 Node spawn/loopback 受沙箱限制时需相应执行权限；GitHub runner 直接运行。真实模型、签名、Windows GUI/UAC 和 iOS 真机仍待具备条件验收。


## Gemini实际Runtime（三平台离线已验收）

固定0.160.1的经典/Lite各三轮实际原生Google→Gateway→Runtime历史已本地通过，第三轮重启app-server从磁盘恢复完整signed Parts/v2 request/chunks/大数。执行端公共model_catalog_json声明不支持client tool_search，正例显式禁用web；默认高级经典/Lite与basic经典web/basic Lite默认parallelfalse另4负例全部Key/POST为0。只在显式Lite profile启用本地EOF前交付数量校验，不承诺Google原生单调用生成约束。

实际Lite Code Mode自带函数执行经真实审批，批准前无临时marker，结果精确保存/回放；移除marker后重启不重复执行。经典静态MCP echo实际调用/结果原文，重启新MCP进程零重跑。Lite双调用整轮失败，无审批/执行/历史载体且1Key/1POST；共享Gateway失败SSE改response.failed，固定Runtime显示静态安全code。classic/Lite interrupt均实际关闭原生未完成socket。

本轮本地实际Runtime37/0/0与workspace312/0/39通过。源码56f9789/[CI37717424972](https://github.com/bboytang/CAIdex/actions/runs/37717424972)三平台success：实际Runtime Linux37/WindowsmacOS36，Google7各一次；workspace Linux312/Windows307/macOS311，0失败；Google90各一次，Clippy/fmt/native credentials/schema/doctor通过。非商业Full、不证明动态发现/网页/所有原生能力；运行状态真源和V3 H–R顺序不变，Rust macOS也不代表iOS应用构建。

## Qwen实际Classic/Lite Runtime（三平台离线已验）

新增7项真实固定Runtime离线回归：Classic/Lite审批批准后临时执行、原生summary/v3/v4与工具结果落盘、重启恢复完整前缀且不重跑、默认/部分策略Key前拒绝、双调用及半流不交付、stream/待审批取消、迟到审批拒绝及真实Cancel。Runtime两路径提供developer/user消息ID，Qwen仅在显式runtime_context下接受typed数组消息有效ID并保留完整绑定；默认/状态门控不放宽，新增Provider2项与Qwen72通过。完整本地workspace510/0/59、实际Runtime57/0/0、Clippy/fmt/diff通过，精确源码bf94c9d/[CI37982550340](https://github.com/bboytang/CAIdex/actions/runs/37982550340)三平台完整通过（workspace510/505/509、实际Runtime57/56/56、Qwen72/新增Qwen Runtime7逐名一次、全通过名568/561/565精确为旧CI+2/+7）；旧Lite CI不代验。仅Qwen生产校验及Runtime测试dev接线变化，无新外部依赖或共享生产实现/workflow改动；商业Live/Full、H生产Host及iOS构建未验。详[Qwen验收](CAIdex-Qwen-Provider-设计与验收.md)/[HANDOFF](../HANDOFF.md)。


## F/G compaction 独立离线验收（2026-10-10）

固定上游[选择分支](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/tasks/compact.rs)按Provider能力选择本地摘要或remote V2；[能力来源](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/model-provider/src/provider.rs)及[远端请求](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/compact_remote_v2_attempt.rs)已读取核实。不是旧responses/compact端点：当前v2在Responses请求input末尾附compaction_trigger，接收原生compaction/encrypted_content。测试显式loopback配置采用OpenAI provider身份选择这个固定分支，requires_openai_auth=false，使用既有Gateway合成凭据；没有更改生产Provider能力或增加独立执行器。

新增real_remote_compaction_keeps_opaque_history_after_classic_and_lite_disk_resume分别覆盖两个明确模式：种子轮次、compact/start受理、同ID contextCompaction started/completed及完成终态、实际rollout compacted checkpoint中的完整compaction item、实际app-server重启/disk resume（没有额外POST）、后续轮次完整item逐值承接。Classic/Lite header、compaction_trigger恰好一个、Key/POST各3次和合成认证均核对。opaque值仅合成载体，不证明模型服务解密或签名认证。

Lite本地摘要新增独立用例，复用旧Classic流程与fixture，实际Lifecycle完成且第三请求包含压缩摘要；不以远端证据代验本地分支。定向远端1/0/0（含Classic/Lite）、本地2/0/0（旧Classic+新Lite），完整workspace623/0/75、固定Runtime73/0/0、Clippy workspace/all-targets-D warnings、fmt/diff通过；旧Runtime保持，新增2名。源码`9b0b48f6705c3847449ccd8cf0e1c3761300f4b1`/[CI38081995688](https://github.com/bboytang/CAIdex/actions/runs/38081995688)整体completed/success，三job各17steps成功或条件跳过，完整日志逐名精确旧Gemini CI+2。Linux/Windows/macOS workspace623/618/622（忽略75/73/73）、固定Runtime73/72/72（无忽略），全部0失败，旧533个Provider函数及原生凭据/Secret doctest保持。未证明压缩失败/取消/自动阈值/无限历史/生产Host恢复/商业模型；fixture不授LiveRuntime/Full。


## Runtime idle 与远端压缩非成功路径（2026-10-10续轮）

新增两个固定Runtime离线用例，生产实现、执行器及审批未变。`real_runtime_idle_timeout_closes_upstream_without_retry_and_releases_slots` 使用OpenAI Adapter合成Classic/Lite路由，原生stream idle设500ms，上游Provider仍90s/600s、Gateway及Provider各单许可。真实轮次以idle timeout失败，上游socket关闭，无自动重试、无不完整assistant/reasoning/tool历史；显式同线程新轮次成功，证明两层许可回收。配置/错误来自固定d27764b源码，不由Provider超时替代。

`real_remote_compaction_failure_and_cancel_keep_history_after_disk_resume` 覆盖Classic/Lite远端v2各失败/取消：实际contextCompaction及failed/interrupted终态、取消关闭上游、无额外POST/Key读取、rollout无成功compacted记录；重启实际app-server并disk resume不发POST，显式下一轮逐值保留原历史前缀，移除compaction_trigger且无伪造compaction。复用既有Harness与fixture，无生产远端能力升级。

两项定向各1/0/0通过；完整回归和精确三平台CI状态见HANDOFF。此证据不覆盖Gemini纯comment keepalive专属时序、本地压缩失败/取消、自动阈值、商业模型或生产Host，不能授LiveRuntime/Full。


源码`8c3ecc86d2b212522bb5aeecf26ad8e272a8320f`/[CI38083310797](https://github.com/bboytang/CAIdex/actions/runs/38083310797)整体completed/success，三job各17steps成功或预期跳过。完整日志精确旧CI38081995688通过集合+2Runtime，无遗漏/重复；Linux/Windows/macOS workspace623/618/622（忽略77/75/75），固定Runtime75/74/74（无忽略），全部0失败。raw2308/1994/2005行、函数通过名698/691/695，另Secret doctest1；watch与日志下载exit0。 本阶段离线证据已验证；后续未验范围保持上述说明。


## Gemini comment 与下游 idle（2026-10-10续轮）

新增`real_google_comments_do_not_prevent_runtime_idle_and_explicit_recovery`，Classic/Lite均使用既有Google catalog/profile、明确关闭web及Lite单调用opt-in；只在测试设置Runtime idle500ms和Provider/Gateway各单许可。首POST每100ms仅SSE comment（至少2帧），上游native默认idle90s/total600s，不能以native超时替代下游路径。纯comment不生成投影事件，实际Runtime明确idle失败，未落盘partial assistant/reasoning/tool；真实上游socket关闭、无自动重试。显式同线程新轮次成功，Key/POST恰2次，仅第二次记录完整native回复，证明许可回收。

复用OpenAI idle断言，未改生产keepalive、Gateway或审批；定向1/0/0通过，完整回归与精确CI状态见HANDOFF。本证据验证comment未传到Runtime的现有边界，不承诺keepalive支持、真实商业推理或生产Host。


源码`74bc3205ca52c129a8a723624cb453b97fbfccea`/[CI38084218039](https://github.com/bboytang/CAIdex/actions/runs/38084218039)整体completed/success，三job各17steps成功或预期跳过，完整日志精确旧CI38083310797集合+1Runtime。Linux/Windows/macOS workspace623/618/622（忽略78/76/76）、固定Runtime76/75/75（无忽略），全部0失败；raw2310/1996/2007行、函数通过名699/692/696，另Secret doctest1。旧Provider、OpenAI idle、远端压缩及审批回归无遗漏/重复，watch和下载exit0。 本专项离线验证完成，商业模型/生产Host未验。


## 本地摘要压缩非成功与磁盘恢复（2026-10-10续轮）

新增`real_local_compaction_failure_and_cancel_keep_history_after_disk_resume`，Classic/Lite各失败与取消四路径。与远端测试复用生命周期/恢复断言，但显式fixture provider身份保持本地摘要分支，不选择OpenAI remote V2。固定stream/request retries=0；第二POST为user摘要请求，无compaction_trigger，失败返回response.failed且无成功completed；取消停在未完成真实socket。实际failed/interrupted终态、取消关闭上游、无额外POST/Key读取，rollout无成功compacted记录。实际app-server重启及disk resume不发POST，显式第三轮成功、Key/POST各3次。

原始history前缀逐值保留。Lite本地压缩会将additional_tools清空；恢复正常轮次后的工具item与首轮原始请求逐值一致，其余history前缀逐值一致。首定向因此失败，核实真实wire后补独立工具恢复断言，最终定向1/0/0通过；未改生产执行/审批/能力。完整回归与精确三平台CI状态见HANDOFF。本证据不代验自动阈值、无限历史、真实商业模型或生产Host。


源码`d72c88a292a4ab41775e38c95c73e7f62d2f7cda`/[CI38085110360](https://github.com/bboytang/CAIdex/actions/runs/38085110360)整体completed/success，三job各17steps成功或预期跳过，完整日志精确旧CI38084218039集合+1本地压缩Runtime。Linux/Windows/macOS workspace623/618/622（忽略79/77/77）、固定Runtime77/76/76（无忽略），全部0失败；raw2312/1998/2009行、函数通过名700/693/697，另Secret doctest1。旧Provider/远端compaction/idle/执行与审批回归无遗漏或重复，watch及完整下载exit0。 本地非成功专项已完整离线验证；自动阈值、商业模型及生产Host仍未验。


## 自动阈值压缩及磁盘恢复（2026-10-10续轮）

新增`real_auto_compaction_threshold_preserves_checkpoint_after_disk_resume`四路径：Classic/Lite × 本地摘要/远端V2。固定源码已核实配置覆盖模型阈值、默认Total scope及采样前检查。显式测试阈值10000、轮末压缩percent=0，首回复合成usage20000、后续0；首轮仅1POST无contextCompaction，第二轮真实自动contextCompaction开始/完成、摘要或opaque压缩与后续正常推理共3POST。没有调用thread/compact/start，仅一个实际rollout checkpoint。

真实app-server重启/disk resume不推理，第三轮正常完成且无再压缩，共4POST/4Key；各请求核对模型与Classic/Lite header。自动压缩后及磁盘恢复后两次input重放checkpoint：远端item全值保留；本地摘要role/type/id/content全值保留，仅模型wire省略已识别Runtime内部summary归属字段，持久checkpoint的content_item_kinds=[compaction.summary]及turn_id另有断言。首定向因内部metadata差异失败，按实际wire收紧断言后四路径1/0/0通过；完整回归/精确CI见HANDOFF。

生产执行/审批、默认阈值和能力不变。此证据仅上述Total采样前路径，不代表所有scope/轮末/TokenBudget/阈值极值/无限历史，也不授商业LiveRuntime/Full或生产Host恢复。
