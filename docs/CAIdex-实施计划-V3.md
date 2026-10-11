# CAIdex 实施计划 V3

执行基准：2026-10-06；账户/记忆/云架构更新：2026-10-08；CLI正式契约更新：2026-10-09。本文件将已确认的 V3 调整固化为开发阶段；原 V2 保留需求背景，冲突处以本计划、[UI 规范](CAIdex-UI-规范-V1.md)及[Account/Memory/Cloud 设计与验收 V1](CAIdex-Account-Memory-Cloud-设计与验收-V1.md)及[CLI 完整交互与验收规范 V1](CAIdex-CLI-完整交互与验收规范-V1.md)为准。新增账户及完整 CLI 架构尚未实现或功能验收；当前 F/G 进度及精确 CI 证据仍见 [HANDOFF](../HANDOFF.md)。

## 产品与架构

- 平台：Windows 11 x64、Linux x86_64 Host/CLI、iOS 17+；Windows/iOS 默认简体中文及可选英文，CLI 英文。当前不开发 Android、Web 客户端或 macOS 原生客户端。
- Windows：Tauri 2 + React/TypeScript/Rust。iOS：SwiftUI，先 Swift 5 模式；Rust 共享客户端/模型/同步/远程核心经 UniFFI 接入。
- Codex 使用固定 commit 的真实 Runtime/app-server，最小补丁；协议边界隔离上游。接口在验证后版本化，允许明确迁移，不实施原 V2 的永久冻结。
- 普通 Chat 在客户端独立调用模型核心，没有 Shell/Git/项目写权限；与 Codex 共用模型/凭据基础能力。模型和工具支持以实际兼容性报告为准。
- Codex Gateway 对外提供 Responses HTTP/SSE，向内适配提供商；保存不透明签名/推理信息，统一工具 JSON/文本消息，不执行工具。
- Provider 分阶段接入：OpenAI、Anthropic、自定义 Responses-compatible、自定义 OpenAI-compatible（Chat Completions）、Gemini、兼容 API（DeepSeek/Qwen/OpenRouter 等）及 Ollama。每个模型单独验证支持范围。
- 模型在轮次边界切换；同提供商仅支持已验证兼容组合，跨提供商使用关联分支/新线程及适配后的历史。
- 凭据绑定执行端，Windows 系统凭据、iOS Keychain、Linux 受保护存储；不进入 Git、日志和历史同步。手机 Remote 使用 Host 保存的凭据。
- Host 独立管理 app-server 与线程，GUI/SSH 退出不终止任务；进程/机器重启不盲目重跑工具。Runtime 是执行、队列、Steer 和审批的真源。
- 捕获事件按 Host/stream 编号，落盘后广播；快照恢复缺口，提交幂等，审批竞争首次有效处理生效。区分未确认请求和明确失败，不承诺外部操作恰好执行一次。
- Remote 先 SSH：Windows/CLI 系统 OpenSSH，iOS 经共享核心接入。后续 Relay 从生产首版加入成熟端到端加密方案（Noise/Snow），配对身份、撤销与重放处理一起验收。
- 统一 CAIdex Account 由官方运营，用户直接在 Windows/iOS 客户端注册/登录；CLI只登录已有账户，无注册入口，桌面用系统浏览器+Authorization Code/PKCE，VPS/SSH用Device Authorization Grant；不可变 user_id 管理身份、Chat、个人/项目记忆、同步与登录会话。普通用户不自建身份服务；新设备直接登录、默认不限制数量，auth_sessions 支持独立撤销/刷新轮换/重放防护。账户认证与 SSH/Relay/Host 授权分离，登录不授予执行/文件权限。
- 正式云端账户、Chat、长期记忆与同步服务从第一版使用 PostgreSQL + pgvector、版本化迁移和不能绕过 RLS 的最小权限应用角色；Windows/iOS及Linux CLI本地缓存/记忆继续SQLite，Codex Host journal也继续SQLite且逻辑分离。此前“自托管 VPS + SQLite”不再是正式云服务方案；用户自有执行 Host 是独立系统。未登录/云故障不破坏已配置本地能力，云端数据必须认证。
- Chat 历史同步有独立控制项：本地 outbox/变更 cursor、不可变消息、并发分支、删除 tombstone、附件，同步完成/中断轮次。记忆同步对新账户默认关闭，与自动记忆/Chat同步/Provider记忆发送权限分别管理；CLI登录自动继承服务器已有Enabled/Disabled，不重置开关，继承Enabled不授权上传匿名/其他账户或未确认范围的历史本地记忆。HTTPS 云服务是信任边界，不宣称历史/记忆 E2EE。
- CAIdex Memory 使用模型无关的归属/来源/revision 契约，区分个人/项目记忆、原始 Chat、Runtime 事件和临时上下文；复用 ModelProvider 在授权执行端整合。Chat/整合/Embedding 模型独立，跨模型共享授权记忆不迁移活动 Agent；Embedding 分空间/版本、无模型时可本地保存及基础检索。项目 Git/源码/真实测试优先，记忆不能改变 Runtime 审批。
- 记忆同步采用服务器权威账户开关/版本/epoch，首次开启须登录并确认范围；离线关闭本机立即停传，显示待服务器确认，确认后服务器拒绝全账户新记忆上传/下载及旧任务提交。关闭明确 A保留云端/B删除云端；幂等、冲突、墓碑/旧设备防复活及账户缓存隔离按新设计验收。模型 API Key 不上传/同步，手机不能读 Host Key，执行端离线 AI 作业等待。
- 云端早期内测采用已有中国大陆 2核/2GB/50GB VPS 单实例；EmailSender 可替换，Brevo 免费事务邮件优先、Resend 备用，额度/地区/送达部署验证。只做 VPS 本机有限备份/完整性检查/恢复演练和用户导出，不做异地备份；整机/磁盘损坏可能数据库和备份全损，客户端缓存不保证全量恢复。性能容量与公开运营义务另验，不提前承诺人数。
- GitHub 新公共仓库 `bboytang/CAIdex`。公共 CI 最小权限，不向不可信 PR 提供秘密；iOS 先无签名构建，签名/TestFlight 与真机测试以后具备条件再执行。

## 阶段与验收

各阶段以可验证产物推进。依赖先完成；协议、Gateway、生命周期等高风险验证优先于大量 UI。

| 阶段 | 工作 | 通过条件 |
| --- | --- | --- |
| A 环境与基准 | 仓库、工具盘点、上游 commit 与许可证锁定、计划/UI/品牌归档 | 状态可续接、来源可核对、工具版本可重现 |
| B 协议风险验证 | 真实 app-server 初始化/线程/事件；双向请求、超时/乱序/断开；扩展能力清单 | 本机 doctor 与协议回归通过；未验证的能力明确列出 |
| C 工程与 CI | 按既定 monorepo 路径建立所需工程，锁依赖和 action SHA；Linux/Windows/macOS 基础检查 | 各 runner 实际运行通过；iOS 工程建立后追加真实 iOS 检查 |
| D Runtime 边界 | 完整线程/turn facade，审批、用户输入、工具事件、能力协商与 CLI 对照 | 不吞事件/请求；与固定上游行为一致，未知能力不冒充支持 |
| E 凭据 | 执行端凭据存储、配置归属、统一脱敏 | 秘密不出现在代码、日志、快照和同步；跨设备归属正确 |
| F 模型核心与 Gateway | Responses 流及 Custom Chat Completions 转换、取消、工具 normalization、opaque 信息、错误/限流 | F/G-Offline按有限矩阵验收并经用户确认关闭；F/G-Live保留真实模型验证，用户项目最终执行 |
| G 多模型 Codex | 各 Provider（含两类 Custom）工具/推理/切换与模型注册 | 离线路由有版本化范围/拒绝证据，商业版本报告归F/G-Live；客户端能力展示在J/P实际验收 |
| H Host 与持久化 | 背景 Host生命周期/attach-detach、SQLite journal、线程/持久任务、快照/event sequence、提交幂等/结果未知、真实审批持久化及多端竞争/安全阻塞 | GUI/CLI/SSH连接退出不自动取消Host任务；首次有效审批、断连/重启恢复不盲重跑未知工具；不改为云端Agent，账户不代替Host授权 |
| I Chat/账户/云同步/记忆核心 | 共享Rust/API；独立Chat；官方账户/PostgreSQL/Passkey/auth_sessions；CLI公开客户端PKCE/Device协议、token轮换/撤销/settings；Linux本地memory/cache DTO；Memory Engine/混合检索/Embedding；账户开关/继承/首次上传授权/epoch/删除；EmailSender/本机备份 | 无执行工具权限；ACC/SEC/MEM/SYN/REC/OPS及CLI认证/同步核心具备证据，GUI/终端/真实部署仍待验；完整CLI登录交互归P |
| J Windows 框架 | 应用壳、会话/Composer、模型/Key、主题/i18n/品牌；原生注册/登录/恢复、账户/设备/记忆/云同步设置 | 本机可做检查及 Windows 运行/构建通过；原生认证、独立开关/A/B/切账户交互有证据 |
| K Windows Codex | Host/项目/Worktree、执行、审批、Queue/Steer、Diff/Review；账户与 Host 授权状态展示 | 真实 Runtime 端到端流程通过；账户登录无 Host 执行权限，UI 状态可对照 |
| L SSH Remote | Host 服务、SSH 建连/转发、配对、连接恢复、多 Host | SSH 断开不终止任务；同一线程多端接管与竞争处理正确 |
| M iOS 基础 | SwiftUI、UniFFI/XCFramework、原生导航/品牌/i18n/安全存储；注册/登录/验证/恢复/Passkey | GitHub macOS runner simulator 构建/测试、无签名 archive 通过；认证桥接实测 |
| N iOS Chat | 本机模型/Chat/草稿/附件、账户设备设置、个人/项目记忆、Provider权限/独立同步/A/B删除 | 不混用 Host Key；与 Windows 授权历史/记忆及账户开关正确同步、切账户无泄漏 |
| O iOS Codex | Host/任务、审批、Queue/Steer、Changes/Diff/评论；账户与 Host 独立授权 | 远程真实任务与审批有效；恢复前台核对状态；不把账户设备当 Host 配对 |
| P CLI 整合 | 固定上游TUI/exec、Registry/模型切换、Provider/Profile/Key/配置、Host/SSH Remote、threads/task、浏览器PKCE/Device登录与会话、本地记忆/统一同步、真实审批/QueueSteer、MCP/Skills/Plugins、JSONL/退出码/诊断/迁移 | 按CLI独立规范逐项验证CLI-01～34；全英文、无注册、安全输入、exec无头与持久task分开、退出detach及Host失联不回退；O/H/A/D/T证据分层，真实终端/多设备验收由R补齐 |
| Q Relay | Noise/Snow、Host 配对身份/撤销、重放、转发恢复 | 首次生产发布有 E2EE，中继无法读任务载荷；与 auth_sessions 登录设备管理分离 |
| R 最终验收 | UI真交互/截图、回归/升级/打包、真机/Windows；账户/邮件/权限/跨模型记忆/同步/本机恢复/性能及运营门槛；CLI Windows Terminal/Linux SSH/VPS/无TTY/真实Host/认证服务/多设备 | 核心、GUI及CLI-01～34分层收集实际证据；未验明确，不用Rust macOS CI代验iOS或合成测试代验真实CLI/认证，不宣称全故障恢复 |

UI 实现内部顺序参考 UI 规范，整个项目顺序以上表的 Runtime/模型/Host 前置条件为准。

2026-10-10用户执行顺序确认：真实模型验收在全项目完成后由用户自行执行；当前先完成开发端能够实现/离线验证的既定任务。F/G商业模型通过条件仍保留为未验，离线fixture不替代真实证据，不授LiveRuntime/Full；不得因等待商业授权停止可独立推进且已获批准的里程碑；不因此自动授权跨阶段。

### 里程碑授权、退出与独立审计（2026-10-10正式调整）

整体A–R功能、顺序和架构依赖保持；阶段不自动等于一次无限授权。已批准里程碑内持续自主细化、实现、修复、必要测试、commit/push main及精确CI，不逐小步求确认；里程碑之间必须由用户确认。没有新强制分支/PR/人工合并/Tag制度。前次治理任务仅改Markdown并已结束；2026-10-10用户确认F/G-Offline关闭并明确批准H-1，F/G-Live继续待验，H-2/H-3/I仍未授权。

每个里程碑实施前必须写明：目标、有限任务/非目标、输入及实际依赖、验收来源与具体通过条件、代码/测试/运行交付物、停止条件。结果区分计划/已实现/已验证；不能因协议方法存在或文档声明就标功能完成。阻断问题须对应当前通过条件且有失败/源码/复现证据；权限、隔离、数据完整性及后续正确性的明确缺陷不能忽略。非阻断与建议注明影响、所属阶段及处理条件，不以未知未来兼容性或任意参数组合扩大退出门槛。

关键里程碑在自测/相关CI及diff核验后，提交不可变源码SHA、精确CI/测试、可复现运行说明与限制报告；由独立新会话或其他模型只读核对实际代码、调用路径、完整证据与范围。自审不算独立审计；不要求每次微小修改重审。审计阻断在当前里程碑修复并相关复验，非阻断记录后续；用户确认通过并批准下一明确里程碑后才能启动。独立审计不替代自动测试/平台运行，也不新设人工Git合并制度。

独立审计重点为F/G-Offline退出、H持久任务/审批/恢复核心、I认证/RLS及同步删除边界、Windows/iOS Codex可操作链路、P完整CLI、Q加密授权及R交付；各阶段细分关键点在批准该里程碑时明确，当前没有这些未来里程碑的编码授权。

### F/G有限退出与证据分层

- **F/G-Offline**：按[当前有限矩阵及阻断清单](CAIdex-FG-离线验收核对-V1.md)核对协议/六方法/Registry/Router/Gateway、九Provider明确profile、Classic/Lite、固定Runtime执行/审批/恢复/切换、能力三态与版本化离线报告、已确定安全及失败路径。源码/证据阻断清空、相关CI有效、拒绝/限制与归属明确，即提出关闭建议；独立审计及用户确认后记录关闭。未知未来问题不无限续期，已验路径不重复开发。
- **F/G-Live**：商业模型及每个真实版本、实际Ollama/性能、原生签名与真实能力保持待验，用户全项目完成后自行执行。fixture不是LiveRuntime或Full，不读用户Key/不调用收费模型；用户确认Offline退出并批准后续里程碑后，Live等待不无限卡住H–R。若出现影响生产架构/后续正确性的明确缺陷，按证据重新阻断分析。
- 后续阶段原有依赖不变：生产Host不由临时Runtime测试代验，GUI/CLI能力展示不是F/G数据门控的重复开发；Skills/Plugins/MCP/Queue/Steer/Diff/Review原需求保留，完整产品操作按H/K/O/P/R分别验证，不以离线计数认领完成。

### H–R可运行交付与实际依赖

每个关键阶段尽可能提供可操作、可观察或独立复现的结果。交付记录至少包含：不可变源码/构建版本、取得方式、启动方法、测试环境及合成/真实边界、核心操作、预期输出/通过判定、限制与失败恢复。轻量内部客户端仅供开发验证，不冒充正式Windows GUI或完整CLI。实际依赖以已稳定契约为输入，不重复开发Facade/Provider/Broker；商业API、完整GUI、邮件送达或签名设备条件不是全部核心工作的前置等待理由，相关实际验收仍保留。

**H内部里程碑（H-1/H-2已获用户确认审计通过，H-3于2026-10-11明确获批，I仍需另行批准）**

| 分组 | 有限任务/可交付结果 | 通过及停止条件 |
| --- | --- | --- |
| H-1 本地Host生命周期与事件持久化 | 独立Host管理固定Runtime进程；SQLite版本化journal按Host/stream编号、先提交再广播；内部客户端attach/detach、seq补缺口/一致snapshot；用真实非收费初始化/线程事件演示 | 两客户端断连/重连可复现，客户端退出不终止Host；重启读取journal但不重发未知动作，失效状态不伪装恢复；不泄Key。代码/测试/运行证据及关键审计就绪后停止，不把该骨架当完整H |
| H-2 持久任务与原生执行链 | 任务提交/operation幂等、查询/取消/受理与完成区别、实际策略；原生审批转交、执行/终态/结果未知、重启不盲重跑、模型轮次边界及持久关联 | 内部客户端可提交/查询/取消；合成模型驱动真实Runtime隔离执行，回应丢失按原operation恢复，不重复执行；Host退出/重启/未知结果有证据；不得另造执行器/审批或使用云专用history入口 |
| H-3 多端审批、Diff与纵向集成 | 持久审批首次有效、过期/撤销/并发/安全阻塞；真实Runtime事件及Diff/Review数据转交，恢复与有界失败/隔离；复用H-1/2 | 在稳定Host契约上演示“发起任务→流式输出→工具审批→实际执行→完成状态→Diff→断线恢复”；核对两端竞争/迟到回应、seq/snapshot缺口及不重跑。完整H其他既定要求同验，不能用演示省略权限/恢复门槛 |

H-1已正式获批；具体传输/schema/隔离细化对照[CLI Host契约](CAIdex-CLI-完整交互与验收规范-V1.md)和现有Facade，交付见[H-1设计与验收](CAIdex-H1-Host-设计与验收-V1.md)。H纵向链可用集成程序或内部测试客户端，不依赖提前做完整J/K。H的模型关联、权限上限、审批与连接生命周期不依赖官方账户登录代授权，I在既定独立身份域接入。H-1完成不代表完整H通过，也不授权H-2。

**I内部可独立验收分组（细化下文既有五步，不新增重复任务、不减验收项）**

| 分组 | 交付与验收重点 |
| --- | --- |
| I-1 账户/权限与数据库 | 不可变user_id、项目ACL/全部逻辑实体、PostgreSQL+pgvector迁移、最小权限角色/RLS；可重建测试数据库，跨账户/项目及未设置上下文拒绝证据 |
| I-2 认证/会话 | 注册/login/密码/Passkey/恢复、EmailSender接口、auth_sessions；refresh轮换/重放/撤销/迟到回调、CLI PKCE/Device及issuer/audience信任边界；本地测试服务可验，真实邮件/部署另记录 |
| I-3 独立Chat与缓存 | 无项目执行工具权限；不可变消息/并发分支/附件、本地SQLite及账户隔离/outbox；本地可运行收发与切账户演示，复用ModelProvider，不复制Host Key |
| I-4 Memory Engine | 个人/项目与原始Chat/Runtime来源区分、revision/纠错/授权检索；整合/Embedding角色与空间版本、无模型基础检索；跨模型Provider外发许可、授权执行端与Key归属 |
| I-5 Chat同步 | 独立同步控制、cursor/outbox/附件/删除/冲突/中断轮次；两测试客户端可复现同步与隔离，不把Chat开关当Memory授权 |
| I-6 Memory同步状态机 | 新账户默认关、继承权威enabled/settings_version/epoch与独立首次上传范围；多设备冲突、离线关闭/待确认、A保留/B重认证删除、tombstone/旧job/旧设备防复活及账户切换 |
| I-7 运行与恢复 | Email实际送达/地域额度、部署边界、迁移与本机有限备份/恢复、2核2GB资源/磁盘失败、导出；测试运行与正式部署授权分别记录，不承诺异地备份或全故障恢复 |

每个I里程碑都交代码、自动测试、独立运行证据和安全边界。完整[Account/Memory验收矩阵](CAIdex-Account-Memory-Cloud-设计与验收-V1.md#13-可执行验收矩阵全部待实现未执行)及CLI认证/同步要求继续有效，表格不是删减或标完成；认证/授权/隔离不得为进度降级。

**J–K Windows**：保持Tauri2/React/TypeScript/Rust与UI规范。J正式工程建立后逐步提供可运行构建；具备可打包条件后Windows CI上传可下载测试产物，注明SHA/环境/有效期/取得位置及五步试用说明（取得→启动→配置→操作→通过判定）。K接真实Host/审批/QueueSteer/DiffReview，独立账户与Host状态。Windows runner构建/测试不代Windows11桌面/UAC/sandbox/安装升级真交互，Rust单测不标客户端完成。

**L SSH**：复用稳定Host契约与V3正式SSH入口，核对SSH连接和Host独立授权、远程task/seq恢复、断连后台继续、模型/Key执行端归属、多客户端审批、建连失败/超时/撤销。提供两客户端或实际SSH可复现操作；不因账号登录授Host执行权，不能用临时隧道fixture声称生产恢复已验。

**M–O iOS**：保留SwiftUI+UniFFI/UI。M工程建立起在GitHub macOS runner执行真正Simulator构建/测试，按具备条件追加无签名Archive；N/O共享Windows/Host已确认的账户/记忆/任务/审批/Remote契约。分别记录Rust macOS、Simulator、无签名Archive、真机功能、可安装发布包五种证据，互不替代；签名/真机条件缺失不阻止已批准的Simulator工作，但相应门槛保持未验。

**P CLI**：保留完整英文TUI/exec/持久task/Remote/Provider/Profile/模型/账户/Memory/MCP/Skills/Plugins等入口，复用H/I/L契约，按CLI-01～34及原O/H/A/D/T证据层次有限验收。提供真实Windows Terminal/Linux SSH/VPS/无TTY的相应操作说明及输出/退出码；协议方法存在不等于CLI命令完成，缺真实环境的项留R，不重新实现Agent/审批。

**Q Relay**：保留Noise/Snow E2EE、配对身份、撤销/重放/恢复，Account与Host/Relay授权分别验证。提供独立端点的加密/配对/撤销及攻击负例运行证据；中继不能读载荷，登录会话不能代配对。

**R最终验收**：按原范围逐项登记“要求→精确版本/环境→操作/测试→实际证据→结论/未验原因”。分别收集离线协议、固定Runtime、生产Host、Windows实际交互、iOS Simulator/Archive/真机/安装包、CLI真实终端、用户真实商业模型、账户/邮件/云同步/本机恢复、安全权限/跨端操作及构建/安装/升级证据。不得以一种成功代另一种，也不承诺全部未知组合通过。

### I 阶段内部顺序与正式门槛

1. 先完成账户/项目 ACL、17项逻辑实体、记忆来源/revision、同步状态/epoch/DTO、API与迁移契约；PostgreSQL 服务端和本地/Host SQLite 分工固定，不用临时数据库/协议。
2. 认证/EmailSender/首版 Passkey、会话轮换与撤销/登录设备列表、RLS/连接池/本地账户隔离；应用内流程供J/M接入；同时定义CLI公开客户端PKCE/Device Authorization、独立auth_sessions、Access/Refresh轮换撤销及settings查询，P接终端流程。具体库、哈希/验证码参数、RP域名及真实邮件送达需验证。
3. 独立Chat/附件/不可变消息与Windows/iOS/Linux CLI按账户隔离的SQLite本地记忆/cache DTO与迁移；Memory Engine 候选→来源验证→去重→冲突→版本→检索，在授权执行端复用 ModelProvider；Provider 使用权限、跨模型和 Embedding 版本/无能力回退。
4. 独立 Chat 同步与默认关闭的账户记忆同步：首次范围/历史本地归属确认、CLI登录继承同一权威开关/离线待确认、幂等/增量/多端冲突、A/B关闭、墓碑/旧设备不复活、退出/升级/恢复一致性。
5. 本机有限备份/恢复/磁盘与2核2GB资源失败测试，稳定共享核心/API再接 GUI；公开服务前邮件/网络/隐私与运营义务核查。

可执行用例及证据要求以 [新设计第13节验收矩阵](CAIdex-Account-Memory-Cloud-设计与验收-V1.md#13-可执行验收矩阵全部待实现未执行)为准：I 验核心，J–O补 Windows/iOS 真交互，P/Q验证独立身份域，R 汇总真实部署/送达/恢复与性能。**这些新用例全部待实现、未执行**；既有 F/G 三平台 Rust/固定 Runtime 离线 CI 不能替代账户、GUI、iOS或生产云服务验收。

### CLI 的H/I/P/R专项门槛

H先落实共享Host持久任务/事件/审批/幂等恢复；I实现账户公开客户端身份及Linux记忆/同步契约；P整合真实TUI/exec和完整命令；R运行真实平台/多设备/认证服务。Windows同机GUI/CLI配置及凭据权限在J/K核对；L沿既有SSH协议；M/N/O共享同账户状态且审批须独立Host授权；Q保持Noise/Snow配对，Account Token不代Relay凭据。执行前核对[CLI规范第3节](CAIdex-CLI-完整交互与验收规范-V1.md#3-固定上游依据与命名冲突)的login、profile、memories、stop等冲突，验收见[CLI-01～34矩阵](CAIdex-CLI-完整交互与验收规范-V1.md#15-cli-专项验收矩阵)，全部待实现/未执行。普通exec继承实际解析的无头审批策略，持久task允许授权跨端审批，提交成功不等于执行成功。当前doctor/credentials/version/help与既有F/G CI证据保持，不因文档任务开展CLI编码。

补充实施断言：H的task受理须返回真实审批配置，不沿用exec无头默认冒充持久审批；I将账户开关、设备本地上传许可、待确认关闭意图分别建模，重新登录不覆盖尚未确认的停传意图。I/P的登录令牌保存失败、退出撤销待确认、CAS拒绝/恢复旧队列等路径纳入CLI-16/17/21/24/26/32，R补真实服务及终端证据。这些是原正式行为的失败路径，不新增阶段、设备级同步开关或临时架构。

P/R的CLI-15/34须覆盖固定exec的stdin编码与空管道差异：prompt支持UTF-8及带BOM的UTF-16，已有位置参数prompt时空管道不应被错误拒绝；credentials秘密输入仍沿现有UTF-8契约。精确规则见[CLI输入输出规范](CAIdex-CLI-完整交互与验收规范-V1.md#12-输入输出jsonl错误与退出码)，当前仅源码核对，未运行完整CLI验收。

H/P/R补验CLI-11/15/16/32的观察、查询与超时边界：task attach JSONL观察端退出只detach，原任务可按ID/seq重连；exec提交前保存operation/Host，回应丢失可用threads status查原操作；显式exec超时只有真实Turn中断终态确认才124，仅受理/失联未知用75而不重提交。I/P/R补验CLI-17/19/21的取消/到期与授权返回竞态、迟到回调/会话清理，不能保存假登录或启动记忆同步。九份固定上游源码及六项版本/help只读核对不代替这些待实现测试；A–R阶段表、F/G恢复点和既有CI证据不变。

配置复核另读取固定core config及config loader：P保留基础用户配置叠加Profile的语义，显式映射CAIdex目录；CLI-08/33验证旧/新Profile同名冲突、非秘密导入范围及命令作用域，不自动修改原Codex配置。I定义独立账户issuer/client/资源audience及官方认证端点信任配置，P接入PKCE/Device，R实测CLI-17/19/21/30；模型Endpoint、项目配置或通用覆盖不能改变账户认证目标或接收Account Token。十一份源码及help核对仅是设计依据，不代表功能验收。

P/R的CLI-11/15/34另验无头exec fork的ForkOnly例外：无prompt时仅确认分支、无Turn/模型调用，0不代表任务执行；图片/输出选项/ephemeral无prompt拒绝，与TUI picker分开。固定exec resume找不到候选可新建线程，CAIdex接入按明确目标/Host恢复契约拒绝，避免静默开始新任务；保留显式新建入口及参数解析，差异须有实际证据。详见CLI第4/12/15节，仍待实施，不改H/I认证/同步或A–R顺序。

## 当前协议基准与限制

- 固定 Codex 0.160.1 / `d27764b82f7118f674371e6d6e76271d9d606edb`，通过 JSONL stdio 初始化和收发；不依赖 UI 专有源码。
- 官方 [app-server 文档](https://learn.chatgpt.com/docs/app-server) 标记该接口及 WebSocket 传输为实验性。固定版本、回归与升级评估是发布前置条件，不将其稳定性视为已获保证。
- `initialize` 成功后发送 `initialized`；请求/响应通过 ID 关联；带 ID 的服务端方法是请求，需要明确回应，不是普通通知。
- 上游 wire 不要求 `jsonrpc` 头。未知方法/扩展字段保留原样；后续应用协议在独立 facade 中映射。
- 固定模型元数据含经典 Responses 与 `use_responses_lite`/`code_mode_only` 两条工具路径；F/G 需按模型实际 wire/tool mode 分别适配和验收，经典测试不能代表 Lite/Code Mode 已支持。
- 当前 doctor 是离线元数据检查，不验证模型推理、工具执行、真实审批、存储恢复或生产 Host。
- request 超时/取消或事件溢出时连接进入不可复用状态；调用方需通过 Host 状态恢复确认结果，不自动重试动作。当前生产恢复流程待 H 阶段实现。

## 构建环境分工

- 当前环境先完成 Linux Rust、CLI、共享核心、可执行的前端与协议检查。
- Windows 原生应用、MSVC、安装器及本机缺失依赖的检查使用 GitHub Windows runner；真实 Windows 11 桌面/UAC/sandbox 差异单独验收。
- iOS 的测试与最终构建均在 GitHub macOS runner；无签名 app/archive/XCFramework/xcresult 不是可直接安装的 IPA。
- iOS 键盘/手势/后台连接与通知等真机行为，在签名和设备条件具备后验证；不以模拟器或静态图代替。

## 开发纪律

- 每次先读 `HANDOFF.md` 与实际Git/相关源码，按[AGENTS](../AGENTS.md)只续接已批准里程碑；完成/暂停/中断前更新恢复点，达到通过条件后停止等待用户验收及下一里程碑授权。
- 只创建当前阶段用到的模块，保留整体路径方向；不提前填满空抽象或假功能。
- 对接口和运行风险做有意义测试，复用已验实现/有效证据，按真实结果记录；非阻断改进进入后续清单，不无限延长阶段。原始 V2 的 CLI/Provider/Remote/工具能力需求仍需逐项验收。
- 实际付费 API 调用与凭据复用/创建需先明确授权；在此之前使用离线元数据与协议测试服务，不读取用户密钥。

## F/G Custom 正式范围补充（2026-10-10）

用户确认V2两类Custom全部保留；V3未逐字列出通用Chat Completions不构成取消。新增独立标准`/chat/completions` Adapter，复用现有ModelProvider、Canonical Protocol、共享传输、Credential Broker、ModelRouter、Responses Gateway及固定Codex Runtime原生执行/审批，不改变A–R顺序。既有DeepSeek/Qwen/OpenRouter/Custom Responses不重写，只有具体测试证明必要时最小扩展共享接口。

请求、JSON/SSE、工具调用与结果、usage、reasoning能力边界、错误/限流、取消/超时及历史安全必须逐项离线验收；不支持能力明确拒绝，不宣称完整原生Responses。新增Adapter定向测试、整体回归、Linux/Windows/macOS精确CI均为门槛；现阶段范围已确认、实现与上述离线验证已完成，精确证据见下段。新增Adapter不自动完成F/G，[要求级验收矩阵](CAIdex-FG-离线验收核对-V1.md)继续核实全部剩余项；商业真实验收由用户在全项目完成后执行，fixture不授商业兼容性或生产Host证据。

2026-10-10 F/G开发端离线复核：两类Custom均已实现；新Chat Adapter及共享传输、Registry/Router/九Provider/固定Runtime完整回归以源码586199fb9655ccd8bff1830968b209aee3d6bf23/[CI38090495112](https://github.com/bboytang/CAIdex/actions/runs/38090495112)三平台完整日志验证。详细要求/残余门槛见[要求级矩阵](CAIdex-FG-离线验收核对-V1.md)。真实模型商业条件保持用户项目最终自验，F/G不标商业整体通过；F/G-Offline关闭及H启动须经用户确认；获批后先核对既定Host与CLI持久化契约，不调整阶段顺序。
