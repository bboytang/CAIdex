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
- 真实 Linux 用例 5 项：消息/usage/历史/resume/fork；活动轮次原生 Queue CRUD 与审批取消；批准后实际执行临时标记命令并回传工具结果；Plan request_user_input；Steer 前置条件、interrupt 及过期审批撤销。CI 显式执行 ignored 集成测试；Windows/macOS 不执行 Linux 专用批准命令用例。

- [三平台 CI 37520407878](https://github.com/bboytang/CAIdex/actions/runs/37520407878)，代码基准 `7209e21`：全部 success。Linux 5 项、Windows/macOS 各 4 项真实 Runtime 集成通过；19 项协议回归、常规/实验指纹与 doctor 均通过。归档 bundle 固定 LF，测试服务不依赖反向 DNS。

## 已核实的交互约束

- 审批 UI 采用请求的 availableDecisions。当前测试的 require_escalated 命令不提供 decline，可提供 cancel；取消结束轮次，批准允许执行。模拟用例覆盖 decline，不冒称该具体真实提示提供所有四个按钮。
- 空闲线程添加 Queue 可能立刻触发新轮次。活动轮次中添加后可 list/update/delete；不要在客户端实现另一套队列调度或假定 add 永远只存草稿。
- interrupt 成功是请求被接受，最终状态以 turn/completed 为准。固定上游取消线程回调时不一定发出逐项 resolved 通知，所以终止轮次也必须清理该线程的待回应请求。

## 原 V2 原生能力逐项验收

| 能力 | 已实现或已验证 | 后续验收 |
| --- | --- | --- |
| Agent lifecycle | 真实启动、消息轮次、中断、完成事件 | 多 Host/后台生命周期 H/K/O |
| Shell | Linux 批准后实际执行临时命令及工具结果回传 | Windows 执行、真实模型选择 |
| Unified exec | 真实上游 exec_command 测试工具链 | 长运行、重启及不同平台 |
| PTY | command/exec、process 系列协议保留 | PTY 输入/resize/退出实际测试 |
| Long-running processes | process/backgroundTerminals 方法保留 | 存活、停止、断连与 Host 重启 |
| apply_patch | 文件审批与 patch 通知转交 | 真实补丁接受/拒绝、workspace 根归属 |
| Filesystem | 全 fs 方法保留 | 真实读写/watch/平台权限 |
| Git | 上游工具/command 通道未替换 | worktree、状态、commit/diff 端到端 |
| Sandbox | 真实 read-only 线程与用户审批 | 真实 Windows sandbox/UAC、权限边界 |
| Approval | 原 ID、availableDecisions、局部一次回应、真实 accept/cancel | 跨客户端竞争与持久化 H |
| Network approval | 复杂 decision 原样 reply | 实际 network policy amendment |
| MCP | 配置、状态、tool/resource/stream 方法保留 | MCP 工具/生命周期及失败恢复 |
| Plugins / Apps | 全 marketplace/plugin/app 方法保留 | 安装/移除、缺依赖、执行权限 |
| Skills | list/config/read 方法及技能输入保留 | 发现、启用/禁用和实际选择 |
| Tool auto-selection | 工具仍由真实 Runtime 执行 | 真实模型选择；fixture 不做推理 |
| requestUserInput | 真实 Plan 问题→答案→工具结果链路通过 | 前端交互、非阻塞/secret/超时 |
| MCP elicitation | 请求转交、原始 reply 保留 | form/url/取消的真实 MCP 流程 |
| Context compaction | compact/start、compacted 保留 | 真实压缩及模型 opaque 数据 |
| Interrupt | 真实 Steer 中断、终态和审批撤销通过 | 多客户端恢复前台后的状态核对 |
| Resume | 真实存储历史及已加载线程 resume | 进程/机器重启恢复 H |
| Queue | 真实活动轮次 add/list/update/delete | reorder/start、自动启动与多端 |
| Steer | 真实 expectedTurnId 前置条件与已有轮次输入通过 | 多端竞争、跨模型轮次边界 |
| Diff | turn diff 与 file patch 通知保留 | 真实 diff/review 与 UI 展示 |
| Plan | Plan collaborationMode、输入工具通过；plan 通知保留 | 真实 plan 输出与 UI |
| Goal | thread/goal set/get/clear、通知保留 | 目标推进/暂停/预算实际执行 |
| Sub-agent | Runtime 配置及工具事件保留 | 原生 delegation 与生命周期 |
| Tool result handling | 真实命令和用户输入结果回送；opaque 字段不丢失 | MCP/patch/图像等结果及 Provider 对照 |
| Usage tracking | 真实 tokenUsage 通知通过（fixture 为零） | 真实计费/限流/Provider usage |
| Session state | 线程元数据/历史读取与通知保留 | Host journal/snapshot 与跨设备 H |
| Thread persistence | 真实临时数据目录内历史/resume/fork通过 | durable Host、重启不盲重跑工具 |

## CLI 对照

固定二进制 `codex --help` 已核对，但 CAIdex CLI 当前仅 doctor/help/version。TUI/exec/remote attach 等在 P 阶段整合，不能将协议保留标为 CLI 命令已实现。

| 上游入口 | 当前 CAIdex 与后续计划 |
| --- | --- |
| 无子命令交互 TUI、--remote、--remote-auth-token-env | P：保留上游英文，明确共享 Host/remote attach |
| agents、exec、review、resume、queue、archive、delete、unarchive、fork | D 协议入口保留；P 原生命令接入待实现 |
| login、logout、mcp、plugin、features | E/F 及 P：执行端凭据与原生配置/管理 |
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
| `mcpServer/resource/read` | 常规 | 协议保留，待验收 |
| `mcpServer/tool/call` | 常规 | 协议保留，待验收 |
| `mcpServerStatus/list` | 常规 | 协议保留，待验收 |
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
| `process/kill` | 实验 opt-in | 协议保留，待验收 |
| `process/resizePty` | 实验 opt-in | 协议保留，待验收 |
| `process/spawn` | 实验 opt-in | 协议保留，待验收 |
| `process/writeStdin` | 实验 opt-in | 协议保留，待验收 |
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
| `thread/compact/start` | 常规 | 协议保留，待验收 |
| `thread/decrement_elicitation` | 实验 opt-in | 协议保留，待验收 |
| `thread/delete` | 常规 | 协议保留，待验收 |
| `thread/fork` | 常规 | 真实链路 |
| `thread/goal/clear` | 常规 | 协议保留，待验收 |
| `thread/goal/get` | 常规 | 协议保留，待验收 |
| `thread/goal/set` | 常规 | 协议保留，待验收 |
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
| `thread/queue/reorder` | 实验 opt-in | 协议保留，待验收 |
| `thread/queue/start` | 实验 opt-in | 协议保留，待验收 |
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
| `item/fileChange/requestApproval` | 常规 | 转交回归；实际功能待验收 |
| `item/permissions/requestApproval` | 常规 | 转交回归；实际功能待验收 |
| `item/tool/call` | 常规 | 转交回归；实际功能待验收 |
| `item/tool/requestUserInput` | 常规 | 真实链路 |
| `mcpServer/elicitation/request` | 常规 | 转交回归；实际功能待验收 |


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
