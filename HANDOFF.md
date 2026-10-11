# CAIdex 开发交接

更新：2026-10-11。正式目录 `/root/projects/CAIdex-v1.0`；恢复依据为仓库/源码/验收文档/精确CI，不依赖聊天或/tmp。历史详见下列验收与证据，避免重复流水账。

## 当前批准：I-2认证/会话，待最终复验，未封存

用户已确认I-1独立审计通过并明确批准I-2；结论来自用户，本会话不冒充独立审计。范围与通过/停止条件见[设计与验收](docs/CAIdex-I2-认证会话-设计与验收-V1.md)。不使用ponytail技能。已批准范围内允许必要源码/迁移/测试/脚本/文档/commit/push main及精确CI，不需要重新批准小步骤。I-3及后续/商业API/真实用户Key/真实邮件/生产部署未授权。

用户最新要求：后续构建/测试尽量在GitHub，并继续当前I-2。已提交并push功能SHA `1a57cf9265ff3743b70f5dc4d216d239c028cf59`，[精确CI38135539590](https://github.com/bboytang/CAIdex/actions/runs/38135539590)正在运行四job，尚未取得最终结论。默认沙箱DNS不可用、.git只读，GitHub/提交命令使用已授权require_escalated通道。已补脚本独立服务进程的实际RSS/CPU测量（合成账户/私有测试邮箱），workflow补授权页JS语法检查。本地Python语法、实测/proc解析自检、JS语法、fmt、差异空白和既有锁文件版本/source/checksum不变检查通过；不在本机重建Rust/Docker或清理缓存。下方d007/未提交/ENOSPC段落为此前核查事实，最终封存时更新。

首轮CI认证Rust联合105组全通过，新增密码撤销并发断言实际通过；认证job随后在独立测量启动probe发生未捕获TimeoutError，清理时PG镜像把本次私有socket目录改为postgres-owned/sticky导致非root runner不能删除。Linux/macOS Runtime job已成功；不能以部分结果代四job通过。已最小修正启动probe在既有30秒循环内重试TimeoutError，容器删除前仅用自身容器root恢复本次socket bind mount的0777，保留0700父目录与无网络隔离。失败原日志/指纹见[首轮GitHub失败](docs/evidence/i2-ci-first-failed.json)，无ENOSPC；待提交修正并验新精确SHA。

上轮以git rev-parse/git ls-remote核对main/HEAD/origin/main/GitHub main均`d0075114aef5b2761c4c94c07c7edc0853087984`；本轮诊断复核本地main/HEAD/origin/main仍相同，未联网复核远端。I-2实施起点工作区干净，现保留未提交实现及证据；没有commit/push或I-2最终CI，不授开发验收或独立审计通过。

实现：新增`cloud/auth`共享信任/安全成对存储与feature-gated Linux认证服务、0002认证迁移、真实HTTP/PG/WebAuthn/PKCE/Device故障测试及`scripts/i2-auth.py`；修改workspace/锁文件/cloud-core版本2、CI数据库job、AGENTS/README/本设计和交接。0001/bootstrap、Runtime/Host/Provider/执行凭据源码未改；既有依赖版本/source不变，新增75包。全部未提交文件以git status为准；`docs/evidence/i2-*`为本轮证据。没有覆盖用户修改。

验证：I-1独立重建118组全通过。I-2中间完整联合108组（一个Rust集成测试）通过，含实际Argon2、软件Passkey签名/错origin/重放/撤销、两服务实例并发refresh、真实非owner角色/RLS与隔离、mature OAuth token form兑换、安全文件存储、取消/迟到清理、设备分页与邮件失败/超时。该108组在最后登录并发修复前，不能代最终源码。最后代码共享契约5/0/0、Clippy all-targets -Dwarnings、fmt、JS语法和tracked差异空白检查通过；Cargo因全局cache追踪数据库满发环境warning，未清理它。完整workspace/三平台最终CI未执行。

最后安全修复：静态确认login原先先验证密码、后锁用户，撤销/身份修改可能夹在中间。已移动用户锁到密码读取/验证前并重查邮箱身份，新增受控真实PG锁断言。尝试修复前复现时Docker创建snapshot即ENOSPC，故没有运行时失败复现或修复后DB通过证据；该断言及最终完整真实服务必须重跑。中间成功不掩盖此未验点。

环境：上轮39GiB持久盘0可用，默认沙箱挂载标记、Docker快照和日志/报告写入均实际ENOSPC。2026-10-11用户要求核查进度/磁盘，本轮`df -h`实测35GiB已用、3.7GiB可用（91%），`df -i`使用6%，项目目录与/tmp各94208字节写入/flush/fsync成功并移除探针；当前磁盘检查满足至少2GiB恢复前提，但未重跑Docker/构建/真实DB，不能据此认领最终验证。未查明空间恢复来源，本轮未清理用户/全局Codex/有效缓存。

本轮诊断：17份归档gzip完整解压及索引SHA256全部一致；`i2-failed-shm_build.log.gz`明确记录aws-lc编译临时文件ENOSPC，`i2-failed-disk-measurement.log.gz`记录日志写入Errno28。最后server/account测试源码SHA256仍与`i2-intermediate.json`待复验指纹一致。实际本地main/HEAD/origin/main仍d007；本轮未联网核对GitHub、未commit/push、未运行CI，只更新交接与设计当前状态。历史失败也含已修复的代码问题，不能全部归因于磁盘。

内存构建替代：本轮私有`/dev/shm/caidex-i2-build-d007-20261011`，单job/debug0/无incremental；TMPDIR必须设其tmp子目录。此目录只有require_escalated命令可见/执行，默认沙箱的/dev/shm不同；有效内存缓存保留，重启会丢失。运行脚本同时设`CAIDEX_I2_TEST_TMP_PARENT`到该tmp，使PG socket/凭据fixture也在私有内存目录。临时log/output可写内存目录，但Docker本身仍要持久盘空间。RSS/CPU测量运行因持久日志ENOSPC失败，没有完成测量报告；9.256秒中间注册/验证/登录时间及64MiB算法参数不冒充RSS/2核2GB压测。

恢复顺序：①最终差异/文档/证据检查、commit/push main；②核对精确功能SHA四job GitHub CI（Linux认证真实DB含新增密码撤销并发断言/独立服务RSS/CPU，及原三平台Runtime），失败则最小修正并验最终SHA；③封存原日志/指纹/试用步骤/限制，再次push核对Git实际状态，停止等待I-2独立审计及明确下一步批准。本地构建前仍须df，不清理有效缓存。

证据：[最后部分验证/源码指纹与待办](docs/evidence/i2-intermediate.json)、[失败与环境阻断](docs/evidence/i2-local-failed.json)、[中间联合运行索引](docs/evidence/i2-intermediate.json)。后者complete108源码指纹对应当时源码；final105边运行边改源码，明确不能使用其后置指纹；脚本已改为运行前指纹+运行后拒绝变化。早期日志只含合成身份且隔离库已移除，归档前检查/脱敏；不读真实邮箱/Key，不调用商业API。

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
