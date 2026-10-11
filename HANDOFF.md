# CAIdex 开发交接

更新：2026-10-11。正式目录 `/root/projects/CAIdex-v1.0`；恢复依据为仓库、源码、验收文档及精确GitHub CI，不依赖旧聊天或/tmp日志。

## 当前批准工作：H-2 持久任务与原生执行链（本地开发验证完成，待精确CI）

2026-10-11用户明确确认H-1独立审计通过，并批准H-2；该通过是用户告知的独立审计结果，不是本实施会话自授。H-1功能9df99f71eb04381260d66a0ab0db95fa6344a438及原证据保留。H-2起点main/HEAD/origin/main/GitHub main为34ba5a636a5773ff6b7114c87fcc9fea935e5146，初始工作区干净，构建前3.6GiB可用。

目标/有限计划/非目标/依赖/通过与停止条件见[H-2设计与验收](docs/CAIdex-H2-Host-设计与验收-V1.md)。仅H-2持久任务/operation幂等/查询取消/原生执行审批转交/实际策略/未知恢复/轮次边界与持久关联；复用Codex0.160.1与既有Facade，不读真实Key/不调用商业API。main直接提交推送及最终功能SHA三平台精确CI，封存后停止等待独立审计；H-3/I未授权。

已实现schema v2事务升级及任务/operation投影、幂等/hash与受理/终态分离、原生执行/审批/取消、300秒任务上限、同live stream已确认边界的模型轮次及独立profile新线程父子关联。H-1命名空间隔离/环境/凭据边界保持；rateLimits仅无载荷收据。真实双端+独立提交者退出/丢回应、原生实际标记写入、取消、已执行但终态未确认强杀重启演示通过，HTTP测量7次请求/重启增量0，5任务、2实际标记写入。全部本地检查通过，最终功能源码[5282ac49cc3f1802d578de9123baa8d772f5244c](https://github.com/bboytang/CAIdex/commit/5282ac49cc3f1802d578de9123baa8d772f5244c)已commit/push main，[新精确CI38101361081](https://github.com/bboytang/CAIdex/actions/runs/38101361081)三平台运行中，该轮已结束：Linux/macOS成功；Windows实际H-2断言/报告通过，但自动删除私有Runtime plugins-clone pack文件WinError5令步骤退出1。仅改演示为保留自身证据目录/报告路径，不强删或忽略失败，随后重新提交新最终SHA及完整三平台CI。当前只必要验证修正/封存，不新增功能。新最终本地workspace673/0/83、Host33/0/0（新增13项）、固定Runtime81/0/0、Linux原生凭据另1项通过；[本地证据](docs/evidence/h2-local.json)及完整日志/源码指纹已归档。尚不认领新CI或H-2独立审计通过。

初轮功能f674f4b/CI38100999321：Windows job114356728434失败于新增替身默认CRLF与严格LF断言；明确LF写入修复，生产执行/审批和断言未降低。初轮Linux/macOS全部成功仍不代新最终源码。[失败CI原始证据](docs/evidence/h2-failed-ci.json)、[本地开发失败/环境修正](docs/evidence/h2-local-failed.json)、[初轮本地证据](docs/evidence/h2-initial-local.json)保留历史。最终还补了明确Thread/Turn ID边界及缺失/异ID回归，不准无ID通知推进未关联任务。


本地Git外新建私有/var/tmp测试目录，未删除/tmp/.git或弱化凭据保护；原生凭据测试依赖缺失已按既有CI安装测试服务并复验通过。磁盘约2.5GiB可用，未清理用户文件/全局Codex/有效缓存。待提交文件均属本轮Host/必要Facade serde/测试/脚本/workflow及授权与验收文档，无其他用户修改；最终以Git核实。

## 2026-10-11 用户授权磁盘清理（已完成）

在不影响后续开发的前提下，仅删除已退出且无打开文件引用的157处私有Runtime测试临时plugins-clone目录及2处未完成增量编译working目录，释放771772416字节（约736MiB），可用空间由约2.9GiB增至3.6GiB。保留成功构建/增量缓存、固定项目Codex0.160.1、所有Host journal与审计证据、用户文件及全局Codex；24个已封存证据文件逐项SHA256核对不变；`cargo build -p caidex-host --locked --offline`复用现有缓存，0.19秒通过。源码与H-1功能SHA/CI结论不变，当时独立审计仍待重新发起；该维护操作没有启动下一阶段。本次只记录维护，不重复全仓Runtime回归。

## H-1 独立审计阻断修复历史（已由用户确认审计通过）

用户本轮仅授权修复AUD-001版本子进程环境继承、AUD-002Runtime通知污染Host命名空间；AUD-003仅最小证据标注。开始时审计基线/main/HEAD/origin/main/GitHub main均已核实为`b516dbae41b252d365d8fad8848c197f2f6ccd51`，初始工作区干净；旧已验源码`dec3a374228a75b9a6600715b0d56bf5047b12b0`。构建前约3.1GB空闲，保留有效缓存及全部用户/全局Codex文件。

有限计划：①复现两阻断并明确共享边界；②版本与Runtime共用原OS白名单，外部host/*安全记录；③合成凭据/伪造事件回归、合法恢复/完整workspace/固定Runtime/schema/doctor/真实双端演示；④最小diff/提交push main/最终源码精确三平台CI/证据封存。非目标：H-2/H-3/I、商业调用/真实Key、全量journal重构或协议改版。通过条件：原路径不再泄漏敏感环境、不接受外部Host控制事件，原契约无回归，新增回归及最终源码三平台全部通过。停止条件：封存证据后停止，等待用户重新发起H-1独立审计；开发验证不授独立审计通过。

当前进度：AUD-001/AUD-002修复与开发验证完成并停止；最终功能源码[`9df99f71eb04381260d66a0ab0db95fa6344a438`](https://github.com/bboytang/CAIdex/commit/9df99f71eb04381260d66a0ab0db95fa6344a438)已提交push main，[精确CI38098407943](https://github.com/bboytang/CAIdex/actions/runs/38098407943)三平台完整成功。每个job实际checkout均为该完整SHA；Linux job114349055503 workspace660/0/83、H-1含20/0/0、固定Runtime81/0/0（另原生凭据1项）；Windows job114349055486 workspace654/0/81、H-1含19/0/0、Runtime80/0/0；macOS job114349055314 workspace659/0/81、H-1含20/0/0、Runtime80/0/0。ignored不计通过。fmt/全仓Clippy/schema/doctor/真实独立Host双端强杀重启与恢复步骤全部成功；SQLite计数测量0重发。新增5项安全/恢复回归各平台通过。

本地最终定向20/0/0、完整workspace660/0/83、固定Runtime81/0/0及其余检查全通过。三轮失败CI、平台替身/目录唯一性修正和复制句柄保持锁的确定复现/最小显式unlock修复见[验收文档](docs/CAIdex-H1-Host-设计与验收-V1.md)。旧复制句柄关闭不释放新所有者锁由新增回归验证。当前无仍需修复的明确阻断；该修复交付时尚未重新独立审计；2026-10-11用户已明确确认审计通过并授权H-2，当前续接以上H-2范围，H-3/I仍未授权。

封存入口：[最终CI摘要/完整原始日志与哈希](docs/evidence/h1-audit-fix-ci.json)、[最终本地日志/文件指纹](docs/evidence/h1-audit-fix-local.json)、[失败CI归档](docs/evidence/h1-audit-fix-failed-ci.json)、验收文档复现/失败/限制/归档核验命令。证据不依赖/tmp。当前branch为main；本次封存前仅本轮文档/证据未提交，源码/依赖/workflow与功能SHA无差异；文档封存HEAD与最终工作区/远端状态须按Git核实，不能将文档HEAD误作功能CI SHA。磁盘约2.9GB可用，无用户/全局Codex/有效缓存清理；workspace改用新建Git外私有/var/tmp目录，没有删除原.git或修改凭据保护。

历史开发验证使用fix-finding只读边界调查/候选审查，不冒充H-1独立验收；用户于2026-10-11另行明确告知H-1独立审计通过。AUD-003：model_turns/commercial_calls/user_keys_read为固定场景声明，非独立遥测；新演示以evidence_basis区分声明、代码断言和SQLite计数测量。旧归档保留历史，不作为这些指标的独立测量证明。

## H-1 初次交付历史（不代本轮修复验证）

2026-10-10批准H-1并完成初次交付；原功能源码`dec3a374228a75b9a6600715b0d56bf5047b12b0`/[CI38095166217](https://github.com/bboytang/CAIdex/actions/runs/38095166217)成功，文档封存至本轮审计基线`b516dbae41b252d365d8fad8848c197f2f6ccd51`。用户随后提供AUD-001/AUD-002阻断，本轮已修复，独立审计需重新发起，不能继续沿用旧交付的通过结论。

初次功能、逐平台计数、故障CI修正、复现步骤和限制保留在[H-1验收文档历史](docs/CAIdex-H1-Host-设计与验收-V1.md#初次交付历史与独立审计不代本轮修复验证)、[原CI证据](docs/evidence/h1-ci.json)、[原本地证据](docs/evidence/h1-final-local.json)及Git历史；只作历史依据，不代最终修复SHA验证。Facade/Provider/凭据源码、本次必要变更以Git diff为准；当前恢复只依据上节最终源码/CI/证据入口。

## F/G 封存状态

F/G-Offline已由用户正式确认关闭。九Adapter/18条route/4条切换的原证据复用[退出核对](docs/CAIdex-FG-离线验收核对-V1.md)，固定源码586199f/[CI38090495112](https://github.com/bboytang/CAIdex/actions/runs/38090495112)三平台全成功（workspace640/635/639，Runtime81/80/80）。本次未改Facade/Provider/凭据源码，最新功能变更仅H-1及必要workspace/依赖/workflow。

F/G-Live仍待真实模型版本/签名/费用、实际Ollama等用户最终实测；没有用户Key读取或商业调用，不授LiveRuntime/Full，不认领生产Host已通过。前次治理b8dfec4已结束，不是当前待办；独立审计未新增执行记录。

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

- H-1已由用户确认审计通过；H-2有限任务/幂等/原生执行链/轮次约束与父子关联已实现并本地验证，待本轮精确CI/独立审计；H-3审批竞争/Diff及完整生产Host未实现。稳定thread/start无原始history；实验thread/resume.history为云专用禁止入口，未使用。
- I官方账户/PostgreSQL/Memory/同步/Email及J–O客户端、L SSH、P完整英文CLI、Q加密Relay、R实际平台/安装升级/权限/恢复仍待。当前CLI仅doctor、credentials status/set/remove、version/help；CLI-01～34不因底层协议存在而完成。
- 各Provider限制以专属验收/profile为准；未知Runtime扩展不保证类型化持久化全量往返。native载体为JSON一致性门控而非密码学真实性，backend配置不是实际endpoint证明；终态缓冲受idle/预算限制，完整前缀可能二次增长。
- compaction只验明确Classic/Lite本地/远端成功与失败/取消、Total采样前自动阈值/磁盘恢复；其他scope/轮末/TokenBudget/无限历史不认领。同步SecretStore开始后不能强停，保证取消后不POST；跨Gateway keepalive未承诺。
- H-1无journal裁剪/高负载吞吐/备份恢复承诺；SQLite提交在服务任务同步执行、同OS用户授权域，Runtime丢事件无法凭已捕获日志重建完整真源。jsonschema0.58.6有字节/regex限制但无硬CPU抢占；后续H须分析生产隔离/有界失败，此限制不写成已解决。当前未发现有实际证据的新F/G阻断；后续若出现影响权限/隔离/数据完整性/后续正确性的明确缺陷，重新纳入阻断分析。

## 恢复入口

- [AGENTS规则](AGENTS.md)、[V3阶段/里程碑/交付](docs/CAIdex-实施计划-V3.md)、[UI规范](docs/CAIdex-UI-规范-V1.md)、[Account/Memory/Cloud设计](docs/CAIdex-Account-Memory-Cloud-设计与验收-V1.md)、[CLI-01～34](docs/CAIdex-CLI-完整交互与验收规范-V1.md)。
- [F/G退出核对](docs/CAIdex-FG-离线验收核对-V1.md)、[Gateway](docs/CAIdex-Model-Gateway-设计与验收.md)、[Runtime能力](docs/CAIdex-Runtime-能力对照.md)、[18条Provider路由报告](docs/CAIdex-Provider-路由离线兼容性报告-V1.md)、[4条切换报告](docs/CAIdex-模型切换-离线兼容性报告-V1.md)。各Provider专属验收由上述索引进入，历史CI留原文/GitHub。
- H-1源码/测试 `runtime/host/`、真实进程演示 `scripts/h1-demo.py`；既有源码 `runtime/bridge/src/`、`model/core/src/`、`model/gateway/src/`、`model/providers/`、`credentials/core/`；固定Runtime/切换/Provider集成测试位于 `runtime/bridge/tests/`。当前H-2有限本地任务Host已实现，入口/演示见H-2文档与scripts/h2-demo.py；无完整生产Host/Windows/iOS工程，完成H-2后不自行开始H-3/I。
