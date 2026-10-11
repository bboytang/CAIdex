# CAIdex 开发交接

更新：2026-10-11。正式目录 `/root/projects/CAIdex-v1.0`；恢复依据为不可变源码、验收文档及精确CI，不依赖聊天或/tmp。

## 当前停止：I-4.1 已封存，等待独立审计

用户确认 I-3 独立审计通过并批准 I-4，自动记忆首次默认关闭；随后明确拆为 I-4.1 本地核心 / I-4.2 Host 接入：先封存、独立审计 I-4.1，再另行批准 I-4.2，不改变 V3 正式 I 阶段。范围/通过/停止/试用见 [I-4.1 设计与验收](docs/CAIdex-I4-Memory-设计与验收-V1.md)。当前 I-4.1 开发通过条件已满足，停止新增功能。**独立审计未由本实施会话授予；I-4.2/I-5 及后续未授权**。不使用 ponytail 技能，不读真实用户 Key/调用商业 API/邮件/生产部署。

用户最新指令再次确认先封存 I-4.1 后等待独立审计，不构成审计通过或 I-4.2 授权。本轮恢复时 main/origin/main/GitHub main 为5d0bb2a且工作区干净；只补充本交接与验收报告。GitHub API 复核 c17c5bd/38146076798 四 job completed/success，36个当前功能文件指纹及32份原始日志双SHA256一致，无ENOSPC；源码未变，复用既有完整CI。两个 Memory 模型配置入口对 Host owner 均通过共享校验明确返回 WrongCredentialOwner，本地没有 Host 任务调度入口，不回退客户端 Key、不借 Execute；共享边界测试与 Memory 入口源码检查有据，不冒充 Host Memory RPC 专项验证。

不可变功能源码 [`c17c5bde51b79c2ec7e1ca5df9c5c3b584847b17`](https://github.com/bboytang/CAIdex/commit/c17c5bde51b79c2ec7e1ca5df9c5c3b584847b17)，[精确 CI38146076798](https://github.com/bboytang/CAIdex/actions/runs/38146076798) 四 job 全部成功，各原始 checkout 完整 SHA 已核对。实际 main 流程，起点67edfe8；封存提交只文档/证据，实际封存 HEAD 以 git rev-parse HEAD 为准，不当功能 CI SHA。封存内容：本 HANDOFF/AGENTS、I-4.1 设计验收与默认关闭对应 Account/UI 三行，以及 docs/evidence/i41-*；源码已在上述功能提交。恢复先核实 Git 状态，不假定以后工作区仍干净。

实现：chat/core/src/memory 共享本地核心，schema2 原子迁移保留 I-3 Chat；个人/项目、来源/不可变 revision、CAS 冲突与人工批准、删除清正文/向量/匹配作业与来源墓碑防复活、导出；独立 Chat/整合/Embedding、实际端点/native model/用途/范围/类型/隐私许可、精确空间/旧版本隔离、无模型/离线基础搜索；默认关闭/等待/撤权/取消/重启 unknown 不重播。ConfirmedSession/Broker 只读凭据 guard、新识别敏感载荷可授权删除；低可信有界临时 Chat 注入，自动队列满不影响已完成 Chat。模型绑定仅当前会话，重启应用需重新配置并明确 run waiting 作业，没有正式 UI/CLI/常驻消费者。

最终实际结果：Linux114489598304 workspace711/0/83、原Chat7/Host42、固定Runtime81；macOS114489598375 workspace710/0/81、Chat7/Host42、Runtime80；Windows114489598108 workspace704/0/81、Chat6/Host41、Runtime80。每端 Memory12（11集成+1容量单元）及3项新 Provider HTTP/Embedding测试已计入workspace；Host也计入，ignored不计通过。Linux原生凭据附加1/Windows额外Host lib4另记。fmt/Clippy/JS/schema/doctor、Chat/Memory/H-1/2/3演示全成功。数据库114489598283真实PG17.10/vector0.8.2，I-1 118/0、I-2 110具名组/0、共享认证5/0。Memory每端5次真实合成HTTP（整合1/Embedding2/Chat2）、上传0，不授商业 Live/正式平台验收。

证据：[最终四 job/36个源码指纹/原日志](docs/evidence/i41-ci.json)、[同提交认证回归](docs/evidence/i41-auth-regression.json)、[七轮开发/28份原日志](docs/evidence/i41-development.json)。开发 Clippy/终态空输入失败已修，两次过时源码主动取消；成功中间结果不替最终提交。39f6b57 Windows Memory全通过，未改动Host service.rs:129等待超时（20/21），根因未定且未放宽断言；最终完整Host/Runtime已成功，历史失败保留。32份日志均校验压缩/原文SHA256，无ENOSPC；原190份证据/Cargo.lock/PostgreSQL/I-3 schema1/固定Runtime/Host/upstream字节不变。

限制：本地 SQLite 明文/OS owner可读；本地 project grant不是云ACL，可信 EvidenceResolver必须新鲜核对实际来源权限，Execution本轮是合成摘要而非真实Host记忆RPC。MEM-10离线/恢复、Host独立Memory权限/自有Broker/Key必须I-4.2；Request当前无记忆RPC，Scope只有Observe/Execute/Approve，不得复用Execute授记忆工具权。云Chat/Memory同步、B删除/epoch数据协议归I-5/I-6。物理VPS满盘/容量、GUI/正式CLI/iOS、商业Live未验；SQLite容量注入只证明事务回滚。盘约3.6GiB可用/91%，本地仅格式/脚本/差异/链接/指纹检查，构建测试均GitHub，未清理用户/全局缓存。

准确下一步：独立新会话/其他模型只读审计上述不可变功能源码、完整调用路径、实际证据和限制；当前范围阻断则最小修复/精确复验。等待用户确认 I-4.1 审计通过并明确批准 I-4.2，才能启动后者。整个 I-4 尚未完成，不自行授审计或跨阶段开发。

## 已关闭 I-3（用户确认独立审计通过）

用户纠正阶段编号后，明确确认I-2独立审计通过并批准I-3；这是用户确认，不是本实施会话自行审计。有限范围/通过/停止/试用见[设计与验收](docs/CAIdex-I3-独立Chat缓存-设计与验收-V1.md)。不使用ponytail技能。允许main流程中的实现/修复/commit/push/精确CI；I-4及后续、商业API/真实用户Key/真实邮件/生产部署未授权。构建/完整测试优先GitHub，未本地重编译。

不可变功能源码[`bc421f01993531c378da852b7a63be207658b91c`](https://github.com/bboytang/CAIdex/commit/bc421f01993531c378da852b7a63be207658b91c)，[精确CI38138552078](https://github.com/bboytang/CAIdex/actions/runs/38138552078)四job全部成功，原始checkout完整SHA逐一核对。实现6585761，b8af139修接口/时间戳/边界，bc421f0修测试导入；封存提交仅追加文档/证据，不当功能CI SHA。实际封存HEAD用git rev-parse HEAD；封存push后核实main/origin/main/GitHub main与工作区，不改历史。

实现：`chat/core`共享Rust普通Chat，复用ModelProvider+Broker/Router，不接Runtime/Host。按可信issuer/user_id/测试域或游客独立SQLite，独占锁/身份/schema检查，conversation项目标记、不可变消息DAG/并发编辑分支、事务附件BLOB/outbox、流式草稿/终态/取消/断流/重启封存；不重发未知模型POST。切账户取消旧任务/封存原归属/拒迟到结果/清模型，登出只开游客。Host Key owner被拒，模型Key不入缓存/云端。Provider载体/推理/工具不写入正文，工具永不执行。附件/文本/历史/页/库/并发/deadline有界。outbox仅本地待同步引用，无云上传/控制/ack；I-4 Memory/I-5同步不在本轮。

GitHub实际结果：Linux114467405810 workspace696/0/83、新Chat7、Host42、固定Runtime81；macOS114467405812 workspace695/0/81、Chat7、Host42、Runtime80；Windows114467405820 workspace689/0/81、Chat6、Host41、Runtime80。Host与Chat计入workspace，ignored不计通过；Windows额外Host lib4/Linux原生凭据附加1另记。三平台fmt/Clippy/JS/schema/doctor与Chat/H-1/2/3演示均成功。数据库114467405723真实PG17.10/vector0.8.2，I-1 118/0、I-2 110具名组/0、共享认证5测试；110组不是Rust测试数。每端Chat演示4次实际loopback HTTP SSE请求，6条游客/2条A消息、附件/分支、切B隔离/清模型、重启恢复，上传0；不是商业Live或正式GUI/CLI/iOS验收。

证据：[最终四job/步骤/原日志/源码SHA256/演示](docs/evidence/i3-ci.json)、[同提交真实认证回归](docs/evidence/i3-auth-regression.json)、[两轮开发失败/八份原日志](docs/evidence/i3-development-failed.json)。38138193270错误使用云Id/SQLite整数接口，38138337723测试Error glob歧义；均已修，未当通过。最终/失败日志gzip及原文SHA256核对。既有所有证据逐字保留，外部依赖版本/source/checksum、Provider Adapter/固定Runtime/PostgreSQL源码不变。共享私有目录抽至credentials/core，Host仍原调用契约；Windows生产ACL脚本与基线逐字相同。

限制：SQLite明文受Unix0700/Windows单SID ACL保护，不能防同一OS用户/管理员。项目标记是本地分区，不授云ACL。磁盘失败回滚旧数据，pending封存失败留待空间恢复；max_page_count注入验证SQLITE容量失败，不授I-7物理VPS满盘/性能压测。真实模型/附件兼容性仍F/G-Live待用户最终验；macOS Rust不代iOS Simulator/Archive/真机，内部例子不代正式CLI/GUI。删除/同步/Memory语义留对应批准阶段。

环境：盘39GiB/已用35GiB/可用3.7GiB（91%），inode此前6%；本轮没有本地完整构建/用户或全局缓存清理，最终及失败CI原日志未见ENOSPC/No space left on device。历史I-2确有持久盘满/ENOSPC，见旧失败证据。默认沙箱DNS不可用/.git只读，已授权GitHub/提交经require_escalated成功；恢复不依赖/tmp/易失缓存，后续本地构建前仍查df。

I-3 历史停止点已由用户确认独立审计通过及 I-4 新授权解除；上述源码和证据继续保留，不由实施会话补授审计。

## 已关闭I-2（用户确认独立审计通过）

功能源码[951f92e092aec87b25838daf00959c8520588701](https://github.com/bboytang/CAIdex/commit/951f92e092aec87b25838daf00959c8520588701)，[CI38135955674](https://github.com/bboytang/CAIdex/actions/runs/38135955674)四job成功，原封存6176529。认证/会话、邮箱验证/密码/恢复/身份/Passkey、refresh轮换/重放撤销、CLI公开客户端PKCE/Device和取消清理、账户信任与成对安全存储已验。登录先锁用户再校验，受控真实PG撤销竞争断言通过；旧缺陷的修复前运行复现因ENOSPC失败，未伪称复现。CLI正式交互/原生GUI/平台Passkey/真实邮件仍后续。

I-2原精确workspace Linux689/0/83、mac688/0/81、Windows683/0/81；Host42/42/41计入workspace，固定Runtime81/80/80另计。真实PG I-1 118/0、I-2 110具名检查/0；独立服务流程4.274秒/CPU4.25秒、idle RSS11824KiB/结束12824/峰值77696KiB，GitHub4逻辑CPU/约16GiB内存，非2核2GB容量验收。首轮CI测量初始化TimeoutError/socket清理问题已修并保留失败；完整证据与试用见[I-2设计验收](docs/CAIdex-I2-认证会话-设计与验收-V1.md)、[最终四job](docs/evidence/i2-ci.json)、[数据库/测量](docs/evidence/i2-database-ci.json)、[首次GitHub失败](docs/evidence/i2-ci-first-failed.json)、[历史中间](docs/evidence/i2-intermediate.json)、[本地失败/ENOSPC](docs/evidence/i2-local-failed.json)。本轮只新增非秘密身份getter并精确CI复验，不重授I-2审计。

## 已关闭I-1（用户确认独立审计通过）

功能源码[bb9c0a18493b7e55debcb961d187b7a196a52eef](https://github.com/bboytang/CAIdex/commit/bb9c0a18493b7e55debcb961d187b7a196a52eef)，[精确CI38109361127](https://github.com/bboytang/CAIdex/actions/runs/38109361127)四job实际checkout均功能SHA并成功。Linux workspace684/0/83、Host42、Runtime81；Windows678/0/81、Host41、Runtime80；macOS683/0/81、Host42、Runtime80；Host计入workspace，ignored不计通过。DB118/0，PG17.10/vector0.8.2。封存d007已核实main一致；2026-10-11用户接受独立审计并授权I-2。

17逻辑实体+附件/Embedding空间，19私有表FORCE RLS/20表事务创建，scope组合FK及最小app权限、会话摘要guard/事务隔离/项目ACL、来源/revision/向量失效/同步默认关已验。I-1本身没有认证服务或Memory/sync业务，本轮I-3不代I-4/5/6来源、CAS/epoch、首次上传、A/B删除与防复活，也不认领真实部署。

[验收与复现](docs/CAIdex-I1-账户权限数据库-设计与验收-V1.md)、[精确CI](docs/evidence/i1-ci.json)、[本地与边界](docs/evidence/i1-local.json)、[数据库](docs/evidence/i1-database-local.json)、[开发失败](docs/evidence/i1-local-failed.json)保留不可变。全仓旧本地684在最终SQL修改前，最终定向DB/core+精确三平台CI补足。凭据/tmp/.git保护未放宽，使用新私有Git外TMPDIR；只删自身临时DB容器，保留镜像/缓存。

## H/F/G历史与架构约束

H-1/H-2/H-3均经用户确认独立审计通过，不在本轮重授。H-3最终3a63c0f/CI38107842905（Linux682/Host42/Runtime81，Win676/41/80，mac681/42/80）及原生Diff限制/取消期限修正见[验收](docs/CAIdex-H3-Host-设计与验收-V1.md)/[证据](docs/evidence/h3-ci.json)。H-2 a6c22072/CI38102729830见[验收](docs/CAIdex-H2-Host-设计与验收-V1.md)/[证据](docs/evidence/h2-ci.json)；H-1 9df99f71/CI38098407943见[验收](docs/CAIdex-H1-Host-设计与验收-V1.md)/[证据](docs/evidence/h1-audit-fix-ci.json)。旧历史“后续未授权”仅是当时范围，不覆盖当前I-3授权。

F/G-Offline用户已正式关闭，九Adapter/18route/4切换复用[退出矩阵](docs/CAIdex-FG-离线验收核对-V1.md)。F/G-Live留用户最终真实模型/费用/Ollama实测，未读用户Key/调商业API、不授LiveRuntime/Full。H-2前用户授权的磁盘维护只删除无引用的157个私有临时plugin-clone和2个未完成working，释放736MiB并保留证据/缓存；本轮没有追加此类清理。

唯一Agent/工具/审批真源固定Codex **0.160.1/d27764b82f7118f674371e6d6e76271d9d606edb**；复用Facade/ModelProvider/Registry/Router/Gateway/Broker及两类Custom。全局CLI版本不改变项目锁定。普通Chat没有Shell/Git/项目写权限；Account/执行Key/Gateway/Host SSH/Relay授权独立，账户登录不能授执行、读取Host Key或代审批权。后台Host持久任务/审批与退出客户端分离，未知终态不能重跑；原生逐文件Diff有效，聚合缺失限制不冒充已解决。

正式云端PostgreSQL+pgvector/RLS，客户端本地SQLite与Host journal分离。不可变user_id；新账户Memory/Chat同步关，权威enabled/settings_version/cloud_epoch全账户继承；后续来源范围、离线关闭意图、A/B删除/epoch/tombstone防复活不可降低。自动记忆、Memory Sync、Chat同步、Provider外发各自独立；记忆不提升Runtime权限。Embedding版本化、来源/revision及权限检索等仍留I后续。

Windows11x64 Tauri2/React/Rust、iOS17 SwiftUI/UniFFI、Linux Host/英文CLI不改；无Android/Web/macOS原生GUI/视频模块。当前CLI仍doctor、credentials status/set/remove、version/help，CLI-01～34不因底层协议认领。GUI简中/英文备选，正式CLI全英文。三平台Rust CI不等于iOS Simulator/Archive/真机或安装包验收；R须实际平台证据。初期2核2GB50GB无GPU/强制Redis/Kubernetes；本机有限备份不能承诺整VPS/盘故障恢复，真实EmailSender提供商/中国送达留I-7。

## 恢复索引

[AGENTS](AGENTS.md)、[V3](docs/CAIdex-实施计划-V3.md)、[UI](docs/CAIdex-UI-规范-V1.md)、[Account/Memory/Cloud](docs/CAIdex-Account-Memory-Cloud-设计与验收-V1.md)、[CLI规范](docs/CAIdex-CLI-完整交互与验收规范-V1.md)为既定边界，I-3本轮不得更改A–R顺序或整产品范围。

源码：`chat/core`（内部演示`scripts/i3-chat.py`）；`cloud/core`、`cloud/postgres`、`cloud/auth`；认证复现`scripts/i2-auth.py`，I-1数据库`scripts/i1-database.py`。固定Runtime/Host在`runtime/bridge`/`runtime/host`，H-1/2/3演示`scripts/h1-demo.py`/`h2-demo.py`/`h3-demo.py`；既有凭据/Provider无需重建改写。未解生产吞吐/备份/边界以各验收报告为准；新有证据权限/隔离/完整性缺陷须按批准通过条件修复复验。
