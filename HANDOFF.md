# CAIdex 开发交接

更新：2026-10-11。正式目录 `/root/projects/CAIdex-v1.0`；恢复依据为仓库、源码、验收文档及精确GitHub CI，不依赖旧聊天或/tmp日志。

## 当前批准工作：I-1 账户/权限与数据库（开发验证完成，停止待独立审计）

2026-10-11用户确认H-3独立审计通过并批准下一步，按V3既定顺序实施I-1，范围及通过/停止条件见[设计与验收](docs/CAIdex-I1-账户权限数据库-设计与验收-V1.md)。起点main/HEAD/origin/main/GitHub main均db854bbe6b87edfda7dcd733a9c7f960032d5a65，初始工作区干净、磁盘1.7GiB。只实施不可变user_id/项目ACL/17实体/PostgreSQL+pgvector迁移/最小权限RLS及相关共享DTO，未改固定Runtime/Host/Provider/凭据生产逻辑。用户要求不使用ponytail技能；I-2及后续/商业API/用户Key/生产部署未授权。

最终功能源码[`bb9c0a18493b7e55debcb961d187b7a196a52eef`](https://github.com/bboytang/CAIdex/commit/bb9c0a18493b7e55debcb961d187b7a196a52eef)已提交push main；[精确CI38109361127](https://github.com/bboytang/CAIdex/actions/runs/38109361127)四job全部成功，原始checkout均完整功能SHA。Linux job114381512936 workspace684/0/83、Host42/0/0、固定Runtime81/0/0；Windows job114381512776 workspace678/0/81、Host41/0/0、Runtime80/0/0；macOS job114381512862 workspace683/0/81、Host42/0/0、Runtime80/0/0。各平台cloud/core2/0/0，fmt/Clippy/schema/doctor/H-1/2/3演示全部通过。Host已计入workspace，ignored不计通过；Linux凭据附加1及Windows额外Host lib4另记。数据库job114381512893与精确SHA本地均118/0/0，真实PostgreSQL17.10/pgvector0.8.2，镜像digest锁定。全仓本地684/0/83在最后SQL游标/约束修改前运行，最终定向core/数据库及完整三平台CI补足，不冒称旧本地结果是最终完整回归。

已实现17逻辑实体另加附件/Embedding空间、19私有表全部FORCE RLS/20表事务创建；scope组合FK防跨账户/项目，项目数据所有者与作者分开、共享游标独立绑定会话用户。非owner/non-superuser/NOBYPASSRLS app不能DDL/TRUNCATE/提升角色/读写认证材料/延长会话/改同步权威状态。窄guard函数从未撤销未过期会话摘要验证actor，不信任user_id GUC，READ COMMITTED事务COMMIT/ROLLBACK清绑定；假上下文/旧快照/过期撤销/删除账户拒绝。项目读者/写者/撤销、个人/project来源FK、不可变消息/revision/来源、作者冒用、无权限SQL关键词/向量与失效向量、新账户同步默认关及禁读写云记忆均实测。

证据入口：[精确CI逐job/步骤/计数/源码哈希及原日志](docs/evidence/i1-ci.json)、[本地精确数据库118条断言](docs/evidence/i1-database-local.json)、[本地日志/最终源码指纹/17实体覆盖](docs/evidence/i1-local.json)、[开发失败及修正](docs/evidence/i1-local-failed.json)。凭据测试因/tmp/.git触发保护，改用新建Git外私有TMPDIR完整复验；DB runner曾误识初始化临时服务，修正为PID1正式postgres及ready后完整复验。没有放宽凭据/RLS或用静态SQL代验。所有gzip/原始SHA256已复核，验收文档有独立重建及只读证据检查步骤。

限制：没有实际认证/Passkey/密码验证/refresh/Email/HTTP服务，合成session只验证DB授权；没有本地SQLite缓存/Memory Engine/同步状态机/真实Embedding/对象存储/生产部署/容量/备份。I-2须安全参数绑定与会话认证/连接池事务清理，SQL约束错误由API安全分类；I-4/5/6须完成来源验证、业务CAS/epoch/首次上传授权/A/B删除/防复活及数据-event原子业务。已有DML/字段不冒充这些功能。已开始的语句和已交付数据不能回收；生产服务须短事务和有界超时。共享DTO不授Host权限、不携带Token/Key。

本次封存只追加证据/验收/状态文档，功能/测试/依赖/脚本/workflow与上述功能SHA一致。推送后核实工作区、HEAD/main/origin/main/GitHub main；封存HEAD用git rev-parse HEAD查询，不当功能CI SHA。本轮只移除自己创建的私有临时DB容器，保留镜像/用户文件/全局Codex/有效缓存；磁盘约1.1GiB。开发验证完成，无当前明确阻断，停止等待I-1独立新会话只读审计，不自行授审计通过或启动I-2。F/G-Live保持待验。

## H-3 历史交付（用户已确认独立审计通过）

2026-10-11用户确认H-2独立审计通过并批准H-3。起点main/HEAD/origin/main/GitHub main均0151b488127f7357df3523176e5e2c24be63bdd8，工作区干净，构建前2.4GiB可用。已按AGENTS/V3/审批契约记录目标、有限计划、依赖、非目标、通过/停止条件，见[H-3设计与验收](docs/CAIdex-H3-Host-设计与验收-V1.md)。仅复用固定Codex0.160.1/原Facade的持久多端审批、Host独立授权撤销、原生Diff/Review恢复与有界纵向链；未读真实Key/调商业模型，不使用ponytail插件，不清理用户/全局Codex/有效缓存。

最终功能源码[`3a63c0f932477387fd8ccdb62ca401917e6a2637`](https://github.com/bboytang/CAIdex/commit/3a63c0f932477387fd8ccdb62ca401917e6a2637)已直接提交push main；[精确CI38107842905](https://github.com/bboytang/CAIdex/actions/runs/38107842905)三平台全部成功，各job实际checkout均为完整功能SHA。Linux job114377025918 workspace682/0/83、Host42/0/0、固定Runtime81/0/0；Windows job114377025755 workspace676/0/81、Host41/0/0、Runtime80/0/0；macOS job114377025910 workspace681/0/81、Host42/0/0、Runtime80/0/0。Host计入workspace，忽略不当通过；Linux凭据附加1/Windows额外Host lib4另记。fmt/Clippy/schema/doctor/H-1/2/3真实进程步骤全成功，本地全仓及固定Runtime同样完整通过，最终源码非root演示也通过。

已实现schema v3及v1/v2事务升级、最多64个独立Host委托令牌摘要/权限/撤销、审批持久首次claim/120秒请求期限/单调时钟/显式撤销、原生Diff/Review与工具artifact明确归属恢复；旧operation hash不变。9项新增Host回归及增强原期限/迁移/容量测试通过。取消RPC确认无终态的真实缺口先失败复现0/1/0，再增加15秒确认截止/重复取消不延长/UTC回拨上限，逾期持久unknown并停止Runtime；修复后定向1/0/0及最终三平台完整通过。H-3双端实测claim1，异常重启模型请求增量0，文件/原生逐文件Diff/Review/请求和授权撤销/快照缺口断言通过。

证据入口：[最终CI逐job/步骤/原始日志与实际计数](docs/evidence/h3-ci.json)、[本地日志/最终源码指纹](docs/evidence/h3-local.json)、[Host42测试名](docs/evidence/h3-host-tests.log)、[HTTP trace](docs/evidence/h3-local-http-trace.json.gz)、[v3快照](docs/evidence/h3-local-snapshot.json.gz)、[取消期限修复前后](docs/evidence/h3-cancel-bound-regression.json)、[开发失败及修正](docs/evidence/h3-local-failed.json)、[原生Diff能力边界/最终非root验证](docs/evidence/h3-runtime-diff-boundary.json)。失败两轮CI及被取消修复取代的中间成功CI留H-3文档，不代最终源码验证。所有证据仓库内封存，gzip/原始SHA256及实际checkout已复核，文档含独立运行步骤和只读证据核对代码。

限制：固定Runtime在sandbox失败尝试后可能不发有效聚合Diff；非root实际0，原生逐文件Diff完整保存/恢复，有发出的聚合仍强制核对；不自行重算或声称无条件聚合。Review合成文字不代模型质量或独立审计。期限是配置截止，实际停止受每秒tick、调度及SQLite影响；未验生产吞吐/备份容灾/正式GUI与完整CLI/远程配对/商业兼容。模型HTTP计数不是全部网络遥测，原生辅助无认证featured-plugin缓存401另记，不使用真实用户凭据。

封存仅H-3证据与验收/交接文档，功能/测试/依赖/脚本/workflow与上述功能SHA无差异。main流程保留，推送后核实工作区干净及HEAD/main/origin/main/GitHub main一致；封存HEAD用git rev-parse HEAD查询，不将其当功能CI SHA。磁盘约1.7GiB，本轮未删除文件或有效缓存，无用户修改被覆盖。开发验证与封存完成；2026-10-11用户另行确认H-3独立审计通过并批准I-1，此为用户告知，不是本实施会话自授审计结论。I-2及后续、商业API/真实Key/生产部署未授权。

## H-2 历史交付（用户已确认独立审计通过）

2026-10-11用户明确确认H-1独立审计通过并批准H-2；该审计通过是用户告知，不是本实施会话自授。H-2起点main/HEAD/origin/main/GitHub main为34ba5a636a5773ff6b7114c87fcc9fea935e5146，初始工作区干净，构建前3.6GiB可用。

目标/有限计划/非目标/依赖/通过与停止条件见[H-2设计与验收](docs/CAIdex-H2-Host-设计与验收-V1.md)。只实施V3 H-2：schema v2任务/operation幂等/hash/查询取消、原生执行审批转交与实际策略、未知恢复、同Provider轮次边界及新profile独立线程父子关联；复用固定Codex0.160.1与Facade，不读真实Key或调用商业API。main直接提交推送及最终功能SHA三平台精确CI；封存后停止，等待独立审计，H-3/I未授权。

最终功能源码[`a6c22072b622a6dc2a77e06951c5b72c0554ca20`](https://github.com/bboytang/CAIdex/commit/a6c22072b622a6dc2a77e06951c5b72c0554ca20)已push main，[精确CI38102729830](https://github.com/bboytang/CAIdex/actions/runs/38102729830)三平台全部成功；三个job原日志实际checkout均为该完整SHA。Linux job114361833844 workspace673/0/83、Host33/0/0、固定Runtime81/0/0；Windows job114361833807 workspace667/0/81、Host32/0/0、Runtime80/0/0；macOS job114361833653 workspace672/0/81、Host33/0/0、Runtime80/0/0。Host包含在workspace，ignored不计通过；附加Linux凭据1项/Windows Host lib4项另记。已完成本地fmt/Clippy/workspace673通过/0失败/83忽略、Host33/0/0、固定Runtime81/0/0、schema/doctor/构建、Linux原生凭据附加1项、H-1/H-2真实独立进程演示。演示包含独立提交者退出且丢回应、双端恢复、原生批准后实际工具写入、取消、工具已执行但终态未知强杀重启；实测5任务/2标记写入/7次本机HTTP请求/重启增量0。声明字段与独立计数/代码断言分开记录。

[本地日志/源码指纹](docs/evidence/h2-local.json)、[Host实际测试名](docs/evidence/h2-host-tests.log)、[HTTP合成请求证据](docs/evidence/h2-local-http-trace.json.gz)、[失败CI原始日志及原因](docs/evidence/h2-failed-ci.json)、[开发失败/环境修正](docs/evidence/h2-local-failed.json)、[配置拒绝已知ID持久化修复前回归](docs/evidence/h2-policy-binding-regression.json)均随文档封存提交。Windows替身LF、演示保留自己的证据目录、macOS journal测试目录追加原子序号均最小修复并完整重跑；不降低测试门槛或用旧成功代新SHA。[最终CI逐job/步骤/实际计数/完整原日志与哈希](docs/evidence/h2-ci.json)已下载核验并封存；开发验证完成，无仍需修复的明确阻断，未授H-2独立审计通过。

本地使用新建Git外/var/tmp私有测试目录，未删除/tmp/.git或修改凭据保护；原生凭据依赖按既有CI安装后复验。磁盘约2.4GiB可用，未清理用户文件/全局Codex/有效缓存。本次封存仅验收/交接文档与H-2证据，功能/测试/依赖/workflow与最终源码无差异；main流程推送后核实工作区及HEAD/main/origin/main/GitHub main一致，封存HEAD用git rev-parse HEAD查询，不将其当功能CI SHA。2026-10-11用户另行确认H-2独立审计通过并批准H-3；旧交付与日志保留，不由本实施会话重授审计结论。

## H-1 与磁盘维护（历史证据）

H-1 AUD-001/AUD-002修复功能[`9df99f71eb04381260d66a0ab0db95fa6344a438`](https://github.com/bboytang/CAIdex/commit/9df99f71eb04381260d66a0ab0db95fa6344a438)/[CI38098407943](https://github.com/bboytang/CAIdex/actions/runs/38098407943)三平台成功，随后用户于2026-10-11明确确认独立审计通过。本次未重授审计结论。实际计数/失败修复/复现/限制详见[H-1验收](docs/CAIdex-H1-Host-设计与验收-V1.md)、[精确CI原始证据](docs/evidence/h1-audit-fix-ci.json)、[本地证据](docs/evidence/h1-audit-fix-local.json)、[失败CI](docs/evidence/h1-audit-fix-failed-ci.json)。初次交付dec3a374/CI38095166217只作历史，不代最终审计修复SHA。AUD-003固定场景字段不是独立遥测，已用evidence_basis区分。

用户授权的磁盘清理已在H-2开始前完成：只删无活动进程/打开句柄引用的157处私有Runtime临时plugins-clone及2处未完成增量working目录，释放771772416字节（约736MiB），2.9GiB增至3.6GiB；保留用户/全局Codex、有效构建缓存、固定Runtime、journal与证据。24个原证据逐项SHA256不变，cargo build -p caidex-host --locked --offline复用缓存0.19秒通过；记录提交34ba5a636a5773ff6b7114c87fcc9fea935e5146。H-2未追加清理，后续可用空间按df核实。

## F/G 封存状态

F/G-Offline已由用户确认正式关闭。九Adapter/18条route/4条切换证据复用[退出核对](docs/CAIdex-FG-离线验收核对-V1.md)，功能586199f/[CI38090495112](https://github.com/bboytang/CAIdex/actions/runs/38090495112)三平台成功。H-2只给既有Facade ApprovalDecision补Deserialize，未改Provider/凭据执行逻辑。

F/G-Live仍待用户最终真实模型/费用/实际Ollama等实测；没有用户Key读取或商业调用，不授LiveRuntime/Full或生产Host通过。

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

- H-1已由用户确认审计通过；H-2有限任务/幂等/原生执行链/轮次约束与父子关联已实现并验证，用户确认独立审计通过；H-3开发验证与精确CI/证据封存完成，用户已确认独立审计通过，完整生产Host仍未验。稳定thread/start无原始history；实验thread/resume.history为云专用禁止入口，未使用。
- I-1账户数据库/RLS开发验证完成、待独立审计，I-2及后续官方认证/Memory/同步/Email及J–O客户端、L SSH、P完整英文CLI、Q加密Relay、R实际平台/安装升级/权限/恢复仍待。当前CLI仅doctor、credentials status/set/remove、version/help；CLI-01～34不因底层协议存在而完成。
- 各Provider限制以专属验收/profile为准；未知Runtime扩展不保证类型化持久化全量往返。native载体为JSON一致性门控而非密码学真实性，backend配置不是实际endpoint证明；终态缓冲受idle/预算限制，完整前缀可能二次增长。
- compaction只验明确Classic/Lite本地/远端成功与失败/取消、Total采样前自动阈值/磁盘恢复；其他scope/轮末/TokenBudget/无限历史不认领。同步SecretStore开始后不能强停，保证取消后不POST；跨Gateway keepalive未承诺。
- H-1无journal裁剪/高负载吞吐/备份恢复承诺；SQLite提交在服务任务同步执行、同OS用户授权域，Runtime丢事件无法凭已捕获日志重建完整真源。jsonschema0.58.6有字节/regex限制但无硬CPU抢占；后续H须分析生产隔离/有界失败，此限制不写成已解决。当前未发现有实际证据的新F/G阻断；后续若出现影响权限/隔离/数据完整性/后续正确性的明确缺陷，重新纳入阻断分析。

## 恢复入口

- [AGENTS规则](AGENTS.md)、[V3阶段/里程碑/交付](docs/CAIdex-实施计划-V3.md)、[UI规范](docs/CAIdex-UI-规范-V1.md)、[Account/Memory/Cloud设计](docs/CAIdex-Account-Memory-Cloud-设计与验收-V1.md)、[CLI-01～34](docs/CAIdex-CLI-完整交互与验收规范-V1.md)。
- [F/G退出核对](docs/CAIdex-FG-离线验收核对-V1.md)、[Gateway](docs/CAIdex-Model-Gateway-设计与验收.md)、[Runtime能力](docs/CAIdex-Runtime-能力对照.md)、[18条Provider路由报告](docs/CAIdex-Provider-路由离线兼容性报告-V1.md)、[4条切换报告](docs/CAIdex-模型切换-离线兼容性报告-V1.md)。各Provider专属验收由上述索引进入，历史CI留原文/GitHub。
- H-1源码/测试 `runtime/host/`、真实进程演示 `scripts/h1-demo.py`；既有源码 `runtime/bridge/src/`、`model/core/src/`、`model/gateway/src/`、`model/providers/`、`credentials/core/`；固定Runtime/切换/Provider集成测试位于 `runtime/bridge/tests/`。当前H-2有限本地任务Host已实现，入口/演示见H-2文档与scripts/h2-demo.py；无完整生产Host/Windows/iOS工程，H-3演示见scripts/h3-demo.py及验收证据，用户已确认H-3独立审计通过，当前仅I-1获批。
