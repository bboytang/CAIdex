# CAIdex 开发交接

更新：2026-10-11。正式目录 `/root/projects/CAIdex-v1.0`；恢复依据为不可变源码、验收文档及精确CI。历史见下列验收/证据，不依赖聊天或/tmp。

## 当前批准：I-2认证/会话，开发验证封存，停止待独立审计

用户已确认I-1独立审计通过并明确批准I-2；范围/通过与停止条件见[设计与验收](docs/CAIdex-I2-认证会话-设计与验收-V1.md)。不使用ponytail技能。用户最新要求后续构建和测试尽量在GitHub，已写入AGENTS；本地只做必要静态/语法/差异/证据检查，完整构建/真实数据库/三平台回归在GitHub执行。本会话不授独立审计通过。I-3及后续/商业API/真实用户Key/真实邮件/生产部署未授权。

不可变功能源码[`951f92e092aec87b25838daf00959c8520588701`](https://github.com/bboytang/CAIdex/commit/951f92e092aec87b25838daf00959c8520588701)，[精确CI38135955674](https://github.com/bboytang/CAIdex/actions/runs/38135955674)四job全部成功；各job原始checkout均完整功能SHA，I-1/I-2源码SHA256逐文件与该提交核对一致。实现提交1a57cf9，随后951f92e仅修正测量启动/自身fixture清理。封存提交只追加证据/文档，功能/测试/依赖/脚本/workflow保持上述CI源码；封存HEAD按`git rev-parse HEAD`查询，不当功能CI SHA。保持main流程，完成封存push后核对实际HEAD/main/origin/main/GitHub main与工作区，不改历史。

实现：`cloud/auth`默认跨平台共享账户信任/会话与成对安全存储契约；Linux feature-gated真实HTTP认证服务，0002认证迁移，邮箱验证/密码/恢复/身份变更/Passkey、独立设备会话/refresh轮换重放撤销、CLI公开客户端PKCE/Device与取消迟到清理。认证角色与数据角色隔离，固定Codex0.160.1/Runtime/Host/Provider/执行凭据源码未改，0001/bootstrap未改；锁文件新增75包，既有版本/source/checksum不变。正式GUI/CLI/平台Passkey及同步后续能力未认领。

最终GitHub：Linux job114459743624 workspace689/0/83、Host42、固定Runtime81；Windows job114459743521 workspace683/0/81、Host41、固定Runtime80；macOS job114459743474 workspace688/0/81、Host42、固定Runtime80。Host计入workspace，ignored不计通过；Windows额外Host lib4、Linux原生凭据附加1另记。三平台共享账户5项、fmt/Clippy/授权页JS语法、schema/doctor/H-1/2/3均通过。数据库job114459743364真实PG17.10/vector0.8.2：I-1独立重建118/0；I-2共110组具名检查/0，联合Rust集成只有一个测试，不能把具名断言组当Rust测试数量。

最后登录修复：先锁用户，再读取/验证密码并重查邮箱身份，阻止撤销/身份修改夹在校验与发会话之间。最终真实PG受控锁断言`password_revocation_committed_before_login_cannot_issue_session`实际通过。旧源码缺陷依据静态顺序；上轮尝试修复前复现时Docker ENOSPC，未声称原缺陷已运行复现。中间108组仅对应当时源码，不能代最终110组；final105曾边跑边改源码的无效指纹保留为历史，最终脚本运行前后指纹一致。

独立服务实测：新的私有本地测试邮箱/合成账户，启动真实`caidex-auth-local`进程，注册→验证→登录→me。GitHub 4逻辑CPU/16372440KiB内存环境，流程4.274秒/CPU4.25秒，单核占用99.43%；idle RSS11824KiB，结束RSS12824KiB，峰值RSS77696KiB（约75.9MiB，含服务启动）。CPU是该PID的utime+stime差值，RSS为/proc/PID/status，排除构建/测试客户端/PostgreSQL。算法65536KiB与实测不同；不授2核2GB压测、生产吞吐或容量通过。

首轮GitHub38135539590认证Rust105组及三平台Runtime均成功，job随后因新增测量probe未捕获初始化期间TimeoutError失败；PG镜像把本次私有socket目录改为postgres-owned/sticky又使非root runner清理失败。已在既有启动重试中处理TimeoutError，删除自身容器前仅由其root恢复本次socket bind mount的0777，外层0700/无网络隔离不变。失败原日志及修复后精确证据见下方，不隐藏失败，不以首轮部分通过授四job成功。

环境：上轮持久盘0可用并实际ENOSPC；本轮诊断39GiB盘35GiB已用/3.7GiB可用（91%），inode6%，项目与/tmp写入/fsync通过。未查明空间恢复来源；本轮不本地重编译、不清理用户/全局Codex/有效缓存。默认沙箱DNS不可用、.git只读，已授权GitHub/提交命令经require_escalated通道成功。原私有内存构建缓存保留，重启会丢失；恢复与验收已依赖仓库/GitHub，不依赖该缓存或/tmp。后续本地构建前仍检查df。

证据：[最终精确四job/逐步骤/原日志及指纹](docs/evidence/i2-ci.json)、[最终真实认证110组/逐文件SHA256/资源测量](docs/evidence/i2-database-ci.json)、[首轮GitHub失败与修正](docs/evidence/i2-ci-first-failed.json)、[历史中间成功/待验索引](docs/evidence/i2-intermediate.json)、[历史开发失败与ENOSPC](docs/evidence/i2-local-failed.json)。原日志gzip及SHA256已核对，试用/独立复现/证据检查见设计与验收。测试身份、邮件和认证器均为隔离合成fixture，无真实邮件、用户Key或商业API调用。

当前无已知未解决的I-2开发通过条件阻断；独立审计尚未执行。准确下一步：由独立新会话/其他模型只读审计上述不可变源码与实际证据；如有阻断在已批准I-2范围内最小修复并复验。用户确认I-2通过且明确批准下一里程碑后，才可启动I-3。等待期间停止新增功能/跨里程碑开发。F/G-Live继续待用户全项目完成后真实验收。

## 已关闭I-1（用户确认独立审计通过）

功能源码[bb9c0a18493b7e55debcb961d187b7a196a52eef](https://github.com/bboytang/CAIdex/commit/bb9c0a18493b7e55debcb961d187b7a196a52eef)，[精确CI38109361127](https://github.com/bboytang/CAIdex/actions/runs/38109361127)四job实际checkout均功能SHA并成功。Linux workspace684/0/83、Host42、Runtime81；Windows678/0/81、Host41、Runtime80；macOS683/0/81、Host42、Runtime80；Host计入workspace，ignored不计通过。DB118/0，PG17.10/vector0.8.2。封存d007已核实main一致；2026-10-11用户接受独立审计并授权I-2。

17逻辑实体+附件/Embedding空间，19私有表FORCE RLS/20表事务创建，scope组合FK及最小app权限、会话摘要guard/事务隔离/项目ACL、来源/revision/向量失效/同步默认关已验。I-1本身没有认证服务或Memory/sync业务，本轮I-2不代I-4/5/6来源、CAS/epoch、首次上传、A/B删除与防复活，也不认领真实部署。

[验收与复现](docs/CAIdex-I1-账户权限数据库-设计与验收-V1.md)、[精确CI](docs/evidence/i1-ci.json)、[本地与边界](docs/evidence/i1-local.json)、[数据库](docs/evidence/i1-database-local.json)、[开发失败](docs/evidence/i1-local-failed.json)保留不可变。全仓旧本地684在最终SQL修改前，最终定向DB/core+精确三平台CI补足。凭据/tmp/.git保护未放宽，使用新私有Git外TMPDIR；只删自身临时DB容器，保留镜像/缓存。

## H/F/G历史与架构约束

H-1/H-2/H-3均经用户确认独立审计通过，不在本轮重授。H-3最终3a63c0f/CI38107842905（Linux682/Host42/Runtime81，Win676/41/80，mac681/42/80）及原生Diff限制/取消期限修正见[验收](docs/CAIdex-H3-Host-设计与验收-V1.md)/[证据](docs/evidence/h3-ci.json)。H-2 a6c22072/CI38102729830见[验收](docs/CAIdex-H2-Host-设计与验收-V1.md)/[证据](docs/evidence/h2-ci.json)；H-1 9df99f71/CI38098407943见[验收](docs/CAIdex-H1-Host-设计与验收-V1.md)/[证据](docs/evidence/h1-audit-fix-ci.json)。旧历史“后续未授权”仅是当时范围，不覆盖当前I-2授权。

F/G-Offline用户已正式关闭，九Adapter/18route/4切换复用[退出矩阵](docs/CAIdex-FG-离线验收核对-V1.md)。F/G-Live留用户最终真实模型/费用/Ollama实测，未读用户Key/调商业API、不授LiveRuntime/Full。H-2前用户授权的磁盘维护只删除无引用的157个私有临时plugin-clone和2个未完成working，释放736MiB并保留证据/缓存；本轮没有追加此类清理。

唯一Agent/工具/审批真源固定Codex **0.160.1/d27764b82f7118f674371e6d6e76271d9d606edb**；复用Facade/ModelProvider/Registry/Router/Gateway/Broker及两类Custom。全局CLI版本不改变项目锁定。普通Chat没有Shell/Git/项目写权限；Account/执行Key/Gateway/Host SSH/Relay授权独立，账户登录不能授执行、读取Host Key或代审批权。后台Host持久任务/审批与退出客户端分离，未知终态不能重跑；原生逐文件Diff有效，聚合缺失限制不冒充已解决。

正式云端PostgreSQL+pgvector/RLS，客户端本地SQLite与Host journal分离。不可变user_id；新账户Memory/Chat同步关，权威enabled/settings_version/cloud_epoch全账户继承；后续来源范围、离线关闭意图、A/B删除/epoch/tombstone防复活不可降低。自动记忆、Memory Sync、Chat同步、Provider外发各自独立；记忆不提升Runtime权限。Embedding版本化、来源/revision及权限检索等仍留I后续。

Windows11x64 Tauri2/React/Rust、iOS17 SwiftUI/UniFFI、Linux Host/英文CLI不改；无Android/Web/macOS原生GUI/视频模块。当前CLI仍doctor、credentials status/set/remove、version/help，CLI-01～34不因底层协议认领。GUI简中/英文备选，正式CLI全英文。三平台Rust CI不等于iOS Simulator/Archive/真机或安装包验收；R须实际平台证据。初期2核2GB50GB无GPU/强制Redis/Kubernetes；本机有限备份不能承诺整VPS/盘故障恢复，真实EmailSender提供商/中国送达留I-7。

## 恢复索引

[AGENTS](AGENTS.md)、[V3](docs/CAIdex-实施计划-V3.md)、[UI](docs/CAIdex-UI-规范-V1.md)、[Account/Memory/Cloud](docs/CAIdex-Account-Memory-Cloud-设计与验收-V1.md)、[CLI规范](docs/CAIdex-CLI-完整交互与验收规范-V1.md)为既定边界，I-2本轮设计不得更改A–R顺序或整产品范围。

源码：`cloud/core`、`cloud/postgres`、`cloud/auth`；认证复现`scripts/i2-auth.py`，I-1数据库`scripts/i1-database.py`。固定Runtime/Host在`runtime/bridge`/`runtime/host`，H-1/2/3演示`scripts/h1-demo.py`/`h2-demo.py`/`h3-demo.py`；既有凭据/Provider无需重建改写。未解生产吞吐/备份/边界以各验收报告为准；新有证据权限/隔离/完整性缺陷须按批准通过条件修复复验。
