# CAIdex 开发交接

更新：2026-10-10。正式目录 `/root/projects/CAIdex-v1.0`；恢复依据为仓库、源码、验收文档及精确GitHub CI，不依赖旧聊天或/tmp日志。

## 当前批准里程碑：治理与计划调整（本次仅文档）

用户要求采用“已批准里程碑内自主执行，里程碑之间用户确认”。本次仅审查/修改治理、计划、验收与交接Markdown，允许commit/push main，禁止功能源码、依赖、商业API及生产部署。文档调整及相关检查已完成；本次治理提交/push封存后停止，等待用户决定。F/G-Offline建议关闭尚待独立只读审计及用户确认，H尚未获启动授权。

开始时实际main/HEAD=origin/main=GitHub main=`0e9004b6aad7a2adbad11b4fbb920a57cb8fd25a`，工作区干净；本次治理收尾提交以实际Git HEAD及提交说明`docs: adopt milestone gates and finite F/G exit criteria`为准；恢复时核实main/远端及工作区。最新已验源码`586199fb9655ccd8bff1830968b209aee3d6bf23`，与治理基线的所有非Markdown源码/依赖/workflow相同。

## F/G当前事实与证据

- 九Adapter已实现：OpenAI、Anthropic、Gemini、Ollama、DeepSeek、Qwen、OpenRouter、Custom Responses、Custom Chat Completions，复用六方法/Canonical/Registry/Router/Gateway/Broker；固定Runtime仍执行/审批真源。
- 本次重新读取[CI38090495112](https://github.com/bboytang/CAIdex/actions/runs/38090495112)精确head、三job各17steps及完整日志：整体completed/success。Linux/Windows/macOS workspace640/635/639（忽略83/81/81），固定Runtime81/80/80（无忽略），均0失败；Core29/Gateway24/Provider550各测试名逐平台一次，Linux原生凭据另1项、Secret compile-fail doctest保持。job114325748624/114325748645/114325748432；不重跑源码回归，复用有效证据。
- 已验证具体Responses/JSON/SSE/工具与结果/usage/reasoning/opaque/错误限流/取消超时边界，Classic/Lite、真实原生审批/隔离执行/磁盘恢复、明确模型切换与压缩路径。18个route/方言及4条切换route有版本化离线报告，仅ProtocolFixture/Experimental，不自动挂生产Registry。
- 旧Chat CI38089213569失败的fixture终态后写trace竞争及required无tools门控已由586199f修复/新三平台CI复验；历史详[Chat验收](docs/CAIdex-Custom-Chat-Completions-设计与验收.md)。不是当前源码阻断，不重复开发。
- 当前明确F/G-Offline源码/证据阻断清单为空；只提出关闭建议，不自动认定用户已验收。详细五类归属、拒绝边界及有限退出条件见[Offline/Live核对](docs/CAIdex-FG-离线验收核对-V1.md)。独立关键里程碑审计尚未执行，实施会话复核不替代。
- F/G-Live保持未验：真实商业模型/API/版本/签名/费用、实际Ollama daemon/模型与性能由用户全项目完成后自验。无用户Key读取/收费调用，不授LiveRuntime/Full；不能因Live等待无限阻止另行批准的H–R。

## 治理调整与准确下一步

已发现文档问题：README把已完成OpenRouter写为下一步、HANDOFF堆叠历史CI、V3/交接“进入H”缺用户批准门槛。本次修正文档状态及执行机制，保留全部V3架构和A–R顺序。

文档/链接/格式及最终diff已检查：213个本地目标、23个锚点通过；仅7份Markdown，所有非Markdown与治理基线相同，V3产品架构正文与A–R工作/顺序保持。未重新运行Rust/Runtime（源码未改，复用上列精确CI）；没有商业API、部署、H编码或独立退出审计。本次commit/push后核实远端相同和工作区干净即停止；不触发无意义Rust CI。

准确下一步只有用户决策：安排独立只读审计/确认F/G-Offline关闭，并明确是否批准建议H-1。此前没有下一开发里程碑授权，不能自行开始Host/账户/客户端或新增Provider工作。

H首个交付建议（未批准、未实现）：独立本地Host生命周期与SQLite事件journal，复用固定RuntimeFacade；内部客户端attach/detach及按seq恢复已落盘事件/快照，客户端退出不终止Host，Host重启不重发未知动作。以非收费真实Runtime初始化/线程事件形成可运行最小链路，不提前做正式GUI/完整CLI。H任务执行/审批多端竞争/取消/结果未知/Diff及纵向任务链另有后续内部里程碑，不能将H首项当H整体完成，详V3。

## 已确定的架构边界（不得擅自改动）

- 唯一Agent/工具执行器/审批真源：Codex **0.160.1 / d27764b82f7118f674371e6d6e76271d9d606edb**。复用Runtime facade、ModelProvider、Gateway、Credential Broker及Custom传输，不另造执行器、审批引擎或模型HTTP栈；未知wire保留，Classic/Lite分别验。
- Windows11x64 Tauri2+React/TypeScript/Rust；Linuxx86_64 Host/CLI；iOS17+ SwiftUI/Swift5+UniFFI。GUI默认简中、英文备选，CLI全英文。用户品牌原件为准，布局尽量参考同平台官方，Key入口在设置；不新增Android/Web/macOS原生GUI。
- Registry目录/配置/协议fixture不授LiveRuntime/Full；native载体绑定执行端/profile/endpoint/model/完整前缀/工具政策。不能自动升级、跨Provider复用签名或放宽历史绑定；JSON载体不是加密/真实性证明。
- Host管理线程/任务/真实审批；退出GUI/CLI/SSH只断连接，不等于取消。journal先落盘再广播，快照/seq补缺口，审批首次有效，不盲重跑未知结果、不承诺外部exactly-once。切Host不迁活动线程；模型轮次边界切换，跨Provider关联分支/新线程。
- 普通Chat独立，没有Shell/Git/项目写权限；API Key归实际执行端，Remote不读Host已存Key，不进Git/日志/Chat/Memory/普通同步。Account Token、Gateway token、CredentialRef.owner、SSH/Relay/Host ACL各自独立；登录账户不授执行/Key读取/代审批权。
- 官方统一Account，不可变user_id；Windows/iOS应用内注册/密码/Passkey/恢复。CLI只登录已有账户：浏览器Authorization Code+PKCE、VPS/SSH Device Grant；公开客户端无长期secret。auth_sessions独立撤销/refresh轮换，不强制设备绑定、旧设备批准或默认设备数量限制。
- 正式云端账户/Chat/Memory使用PostgreSQL+pgvector/RLS与版本化迁移；Windows/iOS/Linux CLI本地缓存/记忆、Host journal继续SQLite且逻辑分离。不退回用户自建身份或云SQLite临时路径。未登录/云故障不破坏已有本地权限能力。
- Memory模型无关，个人/项目/Chat原文/执行事件/临时上下文分开，来源/revision可纠错导出，Git/源码/实际测试优先；记忆和外部文本不能提升Runtime权限。Chat/整合/Embedding角色独立，空间版本化，无模型可基础检索；整合在授权执行端复用ModelProvider，不复制Key到云后台。
- 自动记忆、Memory Sync、Chat同步、Provider外发权限独立。新账户Memory Sync默认关；三平台登录继承同一服务端enabled/settings_version/cloud_epoch，Enabled不授权上传匿名/其他账户/未许可旧资料。首次范围确认；离线关本机立即停传/待服务器确认，确认后全账户拒绝新memory上传/下载/旧job提交；A保留云/B重认证删云，以epoch/tombstone/来源屏障防复活；账户切换隔离本地资料。
- 云内测2核2GB50GB单实例，无GPU/强制Redis/Kubernetes；可替换EmailSender（Brevo优先/Resend备用），实际额度/中国网络送达另验。只本机有限备份/恢复与用户导出，无异地自动备份；整VPS/磁盘损坏可能数据库+备份全损，本地缓存不是全量云备份，不承诺容量或全故障恢复。
- Remote先SSH，Q沿Noise/Snow E2EE配对/撤销；不与账户登录设备混用。exec报告实际解析的无头审批策略（通常Never、特定AutoReview例外），不代答人工请求；持久task允许独立Host授权的跨端审批，submit成功不等于任务成功。
- iOS测试/最终构建在GitHub macOS真正simulator/无签名archive；真机/签名另验，无签名archive不是可安装IPA。当前三平台Rust CI不代GUI或iOS验收。

## 未实现、未验证与风险

- H生产Host、持久任务/journal/seq/snapshot/审批竞争、活动轮次模型约束与跨Provider持久关联未实现。稳定thread/start无原始history；实验thread/resume.history为云专用禁止入口，未使用。
- I官方账户/PostgreSQL/Memory/同步/Email及J–O客户端、L SSH、P完整英文CLI、Q加密Relay、R实际平台/安装升级/权限/恢复仍待。当前CLI仅doctor、credentials status/set/remove、version/help；CLI-01～34不因底层协议存在而完成。
- 各Provider限制以专属验收/profile为准；未知Runtime扩展不保证类型化持久化全量往返。native载体为JSON一致性门控而非密码学真实性，backend配置不是实际endpoint证明；终态缓冲受idle/预算限制，完整前缀可能二次增长。
- compaction只验明确Classic/Lite本地/远端成功与失败/取消、Total采样前自动阈值/磁盘恢复；其他scope/轮末/TokenBudget/无限历史不认领。同步SecretStore开始后不能强停，保证取消后不POST；跨Gateway keepalive未承诺。
- jsonschema0.58.6有字节/regex限制但无硬CPU抢占；H须分析生产隔离/有界失败，此限制不写成已解决。当前未发现有实际证据的新F/G阻断；后续若出现影响权限/隔离/数据完整性/后续正确性的明确缺陷，重新纳入阻断分析。

## 恢复入口

- [AGENTS规则](AGENTS.md)、[V3阶段/里程碑/交付](docs/CAIdex-实施计划-V3.md)、[UI规范](docs/CAIdex-UI-规范-V1.md)、[Account/Memory/Cloud设计](docs/CAIdex-Account-Memory-Cloud-设计与验收-V1.md)、[CLI-01～34](docs/CAIdex-CLI-完整交互与验收规范-V1.md)。
- [F/G退出核对](docs/CAIdex-FG-离线验收核对-V1.md)、[Gateway](docs/CAIdex-Model-Gateway-设计与验收.md)、[Runtime能力](docs/CAIdex-Runtime-能力对照.md)、[18条Provider路由报告](docs/CAIdex-Provider-路由离线兼容性报告-V1.md)、[4条切换报告](docs/CAIdex-模型切换-离线兼容性报告-V1.md)。各Provider专属验收由上述索引进入，历史CI留原文/GitHub。
- 源码 `runtime/bridge/src/`、`model/core/src/`、`model/gateway/src/`、`model/providers/`、`credentials/core/`；固定Runtime/切换/Provider集成测试位于 `runtime/bridge/tests/`。当前没有生产Host/Windows/iOS工程，先检查实际文件再设计H。
