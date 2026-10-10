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
| F 模型核心与 Gateway | Responses 流及 Custom Chat Completions 转换、取消、工具 normalization、opaque 信息、错误/限流 | 协议测试服务验证后，用授权凭据做真实模型兼容性验证 |
| G 多模型 Codex | 各 Provider（含两类 Custom）工具/推理/切换与模型注册 | 每模型有版本化兼容性报告，未支持能力清楚展示 |
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

2026-10-10用户执行顺序确认：真实模型验收在全项目完成后由用户自行执行；当前先完成开发端能够实现/离线验证的既定任务。F/G商业模型通过条件仍保留为未验，离线fixture不替代真实证据，不授LiveRuntime/Full；不得因等待商业授权停止可独立推进的开发。

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

- 每次先读 `HANDOFF.md` 与 Git 状态；阶段完成/暂停/中断前更新恢复点。
- 只创建当前阶段用到的模块，保留整体路径方向；不提前填满空抽象或假功能。
- 对接口和运行风险做有意义测试，按真实结果记录。原始 V2 的 CLI/Provider/Remote/工具能力需求仍需逐项验收。
- 实际付费 API 调用与凭据复用/创建需先明确授权；在此之前使用离线元数据与协议测试服务，不读取用户密钥。

## F/G Custom 正式范围补充（2026-10-10）

用户确认V2两类Custom全部保留；V3未逐字列出通用Chat Completions不构成取消。新增独立标准`/chat/completions` Adapter，复用现有ModelProvider、Canonical Protocol、共享传输、Credential Broker、ModelRouter、Responses Gateway及固定Codex Runtime原生执行/审批，不改变A–R顺序。既有DeepSeek/Qwen/OpenRouter/Custom Responses不重写，只有具体测试证明必要时最小扩展共享接口。

请求、JSON/SSE、工具调用与结果、usage、reasoning能力边界、错误/限流、取消/超时及历史安全必须逐项离线验收；不支持能力明确拒绝，不宣称完整原生Responses。新增Adapter定向测试、整体回归、Linux/Windows/macOS精确CI均为门槛；现阶段范围已确认、实现与上述离线验证已完成，精确证据见下段。新增Adapter不自动完成F/G，[要求级验收矩阵](CAIdex-FG-离线验收核对-V1.md)继续核实全部剩余项；商业真实验收由用户在全项目完成后执行，fixture不授商业兼容性或生产Host证据。

2026-10-10 F/G开发端离线复核：两类Custom均已实现；新Chat Adapter及共享传输、Registry/Router/九Provider/固定Runtime完整回归以源码586199fb9655ccd8bff1830968b209aee3d6bf23/[CI38090495112](https://github.com/bboytang/CAIdex/actions/runs/38090495112)三平台完整日志验证。详细要求/残余门槛见[要求级矩阵](CAIdex-FG-离线验收核对-V1.md)。真实模型商业条件保持用户项目最终自验，F/G不标商业整体通过；后续按A–R进入H，先核对既定Host与CLI持久化契约，不调整阶段顺序。
