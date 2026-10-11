# 当前交接：正式部署进行中，邮件明确关闭

2026-10-11 用户明确要求“现在已备好服务器和域名，直接部署好”，随后提供目标 `43.128.156.73`、SSH 用户 `ubuntu`、端口 `22`，确认没有需保留的既有业务、可专用于 CAIdex。该新授权解除此前正式部署待授权状态；不等于确认 I-7 独立审计通过，不启动 J 或其他阶段，不调用商业模型。

有限目标：部署既有 I-7 账户/Chat/Memory 云服务、PostgreSQL/pgvector 与 HTTPS 域名入口、受保护的服务秘密、进程监管及每日 7天/7份/5,000,000,000字节本机备份/淘汰。复用既有权限与删除屏障，不更换架构，不建设 GUI、异地备份或新增产品模块。依据为下述不可变功能源码、I-7验收及操作说明；通过条件为目标机安装/配置可核对、HTTPS与服务/数据库权限验证、重启和备份/隔离恢复实测，未验邮件/容量或运营条件如实记录；完成后停止部署工作，独立审计与下一阶段仍由用户决定。

开始检查实际 main/HEAD ad589c3d70664d12dc08e7a6d4fb70fb319449eb，原工作区干净；此前未提交HANDOFF保留。用户为ubuntu添加当前公钥后SSH成功，sudo可用，无既有业务；caidex.cn/www实际解析到43.128.156.73。目标Ubuntu24.04.4/x86_64/2CPU/约1935MiB RAM+1987MiB swap/根盘40GB、起点可用33GB（不是50GB）。已安装Nginx/Certbot、PG17.11，官方pgvector0.8.2源码构建；真实HTTPS证书已取得，尚未发布应用。SSH主机ED25519指纹SHA256:XAq4ejpAoCZ0MM/uatDKk6baiEBLAlIw/3Grl95Pqjs按首次使用信任固定，尚未经独立渠道核对。cloud状态工具/网络策略文件缺失，不宣称托管网络就绪。

用户确认邮件“尚未配置”。新增显式mail_disabled=true，不读取虚构邮件秘密；注册/找回/邮箱变更在身份查询前拒绝，回调拒绝，不建账户/挑战/发送，不绕过验证。默认false保持旧启动契约。相关最小源码、真实服务入口负例、目标Nginx/systemd/监控配置及准确限制见 [部署记录](docs/CAIdex-云服务部署-20261011.md)。本机auth lib5通过、Clippy/fmt/diff通过，新增配置真实I-7隔离65项/0失败、39源码指纹全部匹配；报告原HEAD为ad589c3+明确工作区补丁，提交后以指纹核对，不篡改报告HEAD。初次新增用例同一合成邮箱重复请求触发既有60秒限流429，改独立测试地址后完整复验；未降低限流。PG17.11生产安装/备份/恢复、HTTPS/API与防火墙等仍待实际验证。

准确下一步：提交/push当前有限部署源码并核验精确CI，生产server-only构建（不发布test-auth版本），安装root拥有的不可变release、普通caidex用户/分离peer角色/PG禁TCP监听、HTTPS入口与备份/prune/监控任务；验证权限/重启/当前机备份与新建隔离恢复，保存脱敏原证据。邮件真实发信地址/域名/供应商秘密与真实送达保持未配置/未验，不把全部开户功能授可用。完成部署后停止，不启动J。当前修改限于本HANDOFF、cloud/auth邮件关闭及相关入口测试、操作说明/部署配置/部署记录；旧DDL/Runtime/Provider/Host与历史证据不改。
---

# I-7 开发验证与证据封存完成，独立审计仍待确认

2026-10-11 用户确认 I-6 独立审计通过并明确批准 I-7；本会话只开发验证，不自行授独立审计。备份规则已确认每日、7天/7份/5,000,000,000字节；先完成合成邮件与隔离运行/恢复。真实中国大陆VPS/邮箱送达及正式部署待另行授权，不用真实模型Key/商业模型，不启动J或其他阶段，不使用ponytail技能。

起点main 5af833a9a76dde005c92160b738c790a3c3473f2，最初工作区干净且本地/origin/GitHub一致；最终不可变功能源码 [235e3e4d4c1917eafe55282426f41edaa0b6ec9d](https://github.com/bboytang/CAIdex/commit/235e3e4d4c1917eafe55282426f41edaa0b6ec9d)，[精确CI38174971743](https://github.com/bboytang/CAIdex/actions/runs/38174971743) 四job completed/success。本封存仅HANDOFF/I-7验收文档/新i7证据，docs-only封存HEAD不替代功能CI SHA；实际branch/HEAD/工作区/远端恢复时用Git核实。有限范围/通过/停止/试用见 [I-7设计验收](docs/CAIdex-I7-运行与恢复-设计与验收-V1.md) 和 [操作说明](cloud/postgres/operations/README.md)。

实现：云schema4→5事务迁移，旧DDL不改、本地schema5/protocol1不变；Brevo/Resend固定HTTPS适配、持久全局预算/稳定任务hash/unknown不重发/认证回调/硬退信投诉抑制；Broker私有秘密+loopback服务入口/磁盘预留。PG管理员有限一致备份/私有清单/hash/7天7份5GB/硬预算/过期中断partial淘汰；同源最新控制/身份/ACL/epoch/墓碑检查点恢复，旧认证/同步世代全部作废，B旧正文/来源/向量不复活且保留其他owner记忆与Chat。恢复库创建即禁止连接，撤CONNECT后才让管理员恢复，始终不自动发布；缺最新记录则账户暂停/凭据撤销/资料隔离。选定本机Chat JSON导出含分支/附件/未发送附件/进行中草稿，原文与JSON双重secret guard，无session/outbox/重放。

性能最小修复：1000向量的逐行嵌套授权查询超登记SLO，新增既有非owner/NOBYPASSRLS guard的语句内授权scope集合，双方active/enabled/范围/pending/ACL规则与原写权限/锁保持；补四个普通FK索引保障大批B清理。完整release密码登录仍Argon2id 64MiB/t3/p1，未降参数或事后放宽SLO。

精确本机235：I-7隔离63项/0失败、39源码指纹全部匹配、含所有权清理的整个进程exit0；Chat/Memory Rust26/0（ignored另记）、auth lib5/0、cloud lib2/0。真实PG+HTTP本机账户116/Chat50/Memory58具名检查；Chat/Memory相关指纹匹配最终源码，账户报告早于磁盘停止准入修正差异明记，最终CI重新验证116。2CPU/2GiB容器同时运行PG与Rust服务；1000条3维精确向量、3附件、顺序登录8/竞争4/查询20，最终本机登录p95约201ms、检索p95约4.91ms、峰值约145MiB、OOM0；物理16MiB ENOSPC保留已提交Chat/outbox，B旧备份/缺检查点/legacy4→5/ACL撤销/认证作废/预算与淘汰/DB与应用进程中断均实际验证。起点盘余6.2GB，收尾约2.8GB/93%；可行定向优先本机，完整workspace/跨平台由GitHub，不清有效缓存。

精确CI：DB114575329029 I-1 118、账户116、Chat50、Memory58、I-7 63具名检查/0失败、auth lib5/0。Linux114575329207 workspace725/0/84及Runtime81/0；macOS114575329223 workspace724/0/81及Runtime80/0；Windows114575329189 workspace718/0/81及Runtime80/0。ignored不算通过，Linux原生凭据1/Windows Host lib4另记；fmt/Clippy/JS/schema/doctor及I-3/I-4.1/I-4.2/H-1/2/3演示实际成功。CI登录p95约86ms/检索约3.04ms/峰值约152MiB/OOM0；恢复约1.269秒（仅小库实测）。每份原始checkout完整SHA与报告指纹核对。

证据：[最终CI](docs/evidence/i7-ci.json)、[本机/开发过程](docs/evidence/i7-local.json)、[初始CI](docs/evidence/i7-ci-initial.json)、[第二轮CI](docs/evidence/i7-ci-second.json)。38本机原日志/12报告、12CI原日志共62份压缩及原文SHA256核验；旧证据不改。初始dcd2967/38174081672及ef87af8/38174431345三平台成功、DB功能62/63项0失败，但非root清理私有PG绑定目录失败，整体failure，不能授通过；235仅测试绑定目录在停止容器后归还调用者uid/gid并恢复私有权限，最终CI完整成功。开发中的目录沙箱、fixture/schema/启动、reqwest已有feature与Clippy/断言问题保留原日志；不把原失败或超SLO当最终通过。

限制：测试是隔离PG/HTTP/内部Rust API，不是正式GUI/CLI/iOS或物理多设备安装。真实送达/地域/域名、公开部署/运营义务、生产50GB长周期/1536维ANN/人数、强制OOM杀死及全VPS/同盘全损恢复未验；同机库与备份可能全损。邮件无持久明文验证码队列/自动unknown对账/供应商自动切换；当前单worker和实际peer限流的公开入口配置仍须部署前核验。事件/收据/墓碑/邮件元数据未GC，增长需监控；维护不运行/失败时不能宣称7天备份已淘汰。最新检查点只证明所标时点，恢复均隔离，不承诺其后操作或自动发布。F/G-Live与真实模型保持待用户最终验。固定Runtime/Provider/Host/upstream/旧DDL/历史证据保持，外部依赖版本/source/checksum不变，只新增既有libc的目标依赖用途；cloud状态工具/策略未提供，不宣称其就绪。

准确下一步：停止I-7新增功能及跨阶段开发，等待独立新会话/其他模型只读审计235源码、上述原始证据与操作步骤；本范围明确阻断才最小修复复验。须用户确认I-7独立审计通过并明确批准后续阶段才可继续，不把V3整体目标或历史授权当跨阶段授权。

---

# CAIdex 开发交接

更新：2026-10-11。正式目录 `/root/projects/CAIdex-v1.0`；恢复依据为不可变源码、验收文档及精确CI，不依赖聊天或/tmp。

## 已关闭 I-6（用户确认独立审计通过）

2026-10-11 用户确认I-5独立审计通过并批准I-6，随后确认“同步暂停，ACL保留”的七条正式规则并批准编码/测试/验证。授权只I-6必要源码/迁移/测试/文档/commit/push main/精确CI；I-7及后续、商业API/真实Key/邮件/部署未授权，不使用ponytail技能。有限目标/任务/非目标/通过/停止/契约/复现见 [I-6设计验收](docs/CAIdex-I6-Memory同步-设计与验收-V1.md)。本会话仅开发验证，未自行授I-6独立审计。

main起点56ff013，文档启动2a06455，初始实现cfbf540；最终不可变源码 [`04d770a789d6f528e2b4e7cf87b929bb9f026e09`](https://github.com/bboytang/CAIdex/commit/04d770a789d6f528e2b4e7cf87b929bb9f026e09)，[精确CI38163486201](https://github.com/bboytang/CAIdex/actions/runs/38163486201) 四job completed/success，原始checkout完整SHA及报告源码指纹逐份核对。最后封存仅本交接/本设计/新i6证据；恢复实际HEAD/工作区/远端仍用Git核实，不把docs-only封存HEAD当功能CI SHA。

实现：PG4/SQLite5原子迁移保留旧Chat/Memory/队列，复用I-2 issuer/session及I-4 payload。Memory控制CAS/稳定收据/持久离线stop与未知回应恢复、精确本机上传许可、独立outbox/cursor和双方有效scope permit；每次RLS重验双方active/enabled/范围/pending/ACL，锁持至提交并与关闭/撤权排序。所有者关闭暂停成员、保留ACL，恢复须当前条件；reader只读/writer可写。immutable revision保留离线分支/disputed，显式head+generation选择；下载+cursor同事务无echo，切账户/撤销/迟到拒绝。

A保留云数据；B需新鲜会话绑定proof（不落盘），先独立提交disabled/epoch/pending，再每批最多32条实际删除本人云Memory正文/revision/来源摘录/向量/job，永久ID/来源hash墓碑防复活，全部清理完才清pending。故障可恢复，旧epoch操作收据也拒绝。成员独立Memory/其他owner项目/本地Chat保留。回连本地须显式epoch保留/丢弃，保留副本解除云关联/许可/队列且冻结派生任务，不自动回流；不明owner的未提交项目副本保守保留并冻结，避免误删。明确recreate须新ID/新授权；自动旧来源拒绝。

本机：Rust定向25/0（lib7/Chat存储6/Memory12，2项数据库ignored用独立入口）；真实PG17.10/vector0.8.2+HTTP Memory56、账户114、Chat48具名检查/0失败，各为1项Rust集成、重复迁移检查不累加。Memory本机报告实际HEAD6526d0a，44指纹与最终04d一致；账户报告提交前修正测试记录旧HEAD，24指纹与最终一致；Chat同步报告精确04d。有效证据复用，Clippy/fmt/Pythoncompile/链接/diff通过。固定Runtime/Provider/upstream/旧DDL/过往证据逐字保留，外部依赖version/source/checksum不变。

精确CI：DB114541106864 I-1 118、账户114、Chat48、Memory56/0，认证lib5/0；Linux114541106961 workspace724/0/83及Runtime81/0；macOS114541106871 workspace723/0/81及Runtime80/0；Windows114541106913 workspace717/0/81及Runtime80/0。ignored不计通过，Linux原生凭据1/Windows Host lib4另记；fmt/Clippy/JS/schema/doctor及I-3/I-4.1/I-4.2/H-1/2/3演示实际成功。证据 [最终CI](docs/evidence/i6-ci.json)、[本机/开发原日志](docs/evidence/i6-local.json)、[首轮失败](docs/evidence/i6-ci-first-failed.json)、[第二轮部分成功/失败](docs/evidence/i6-ci-second-failed.json)；44份原日志压缩/原文SHA256核验，无ENOSPC，原始报告HEAD不重写。

开发失败已修并复验：私有fixture根/granted_by及SQL infinity显式类型；新增跨epoch旧收据负例揭示误确认（正文未复活）后加epoch检查；首轮旧未来schema5/auth不能控制Memory开关断言过时，改schema6及最小受信任权限断言，保留正文/用户ID拒绝；第二轮DB成功、平台旧迁移schema4断言失败，改5后本机12项及最终精确CI通过。早期清理注入因42804不能当触发器验证，最终P0001故障回滚/恢复、分批进度实际通过。额外非缓存feature编译主动中断不授通过，改缓存组合。

限制/环境：一个Rust进程内四份隔离Cache、真实HTTP/PG，关闭竞争两独立Service worker；不是正式GUI/CLI/iOS/物理多设备安装。SQLite明文/HTTPS云信任边界，来源引用不授额外ACL/Runtime权；事件/收据/墓碑永久留存，无GC/生产吞吐/备份净化承诺。真实模型及F/G-Live待用户最终验。起点盘余1.1GiB/98%、RAM1.9GiB/Swap4GiB，I-6封存时余约0.5GiB/99%；本机可行定向验证优先，完整workspace/跨平台由GitHub，未清理有效缓存。Git外私有TMPDIR保留目录安全规则；cloud状态工具/策略文件缺失不宣称就绪，GitHub/Docker经沙箱提升实际访问，无商业API/真实Key/邮件/部署。

2026-10-11 用户明确授权清理磁盘：只删除target/debug/deps内按包/目标/profile/features/compiler分组的107个较早重复ELF构建产物，保留每组最新142个；实际释放6,027,395,072字节（约5.61GiB），可用空间约6.15GiB/使用率85%。清理前无构建/测试进程；658份Git跟踪文件逐份SHA256未变。依赖/增量缓存、工具链/.tools固定Runtime、源码/历史/用户数据/全部审计证据及临时日志保留。记录见 [磁盘清理证据](docs/evidence/disk-cleanup-20261011.json)。未重跑构建；后续若请求较早产物可能需重新链接。此维护不授I-6独立审计、不启动I-7。

I-6 的历史停止已由用户确认独立审计通过及 I-7 新授权解除；原不可变源码与全部证据保持。当前准确下一步以本文件顶部 I-7 为准。

## 已关闭 I-5（用户确认独立审计通过）

2026-10-11 用户明确确认 I-4.2 独立审计通过并批准 I-5；本机满足编译/测试/构建条件时优先本机，否则 GitHub。有限目标/任务/非目标/通过/停止、实际契约与试用见 [I-5设计验收](docs/CAIdex-I5-Chat同步-设计与验收-V1.md)。允许必要实现/迁移/测试/文档/commit/push main/精确CI，本轮开发验证已完成；I-6及后续、商业API/真实Key/邮件/部署未授权。不使用ponytail技能。

起点 main `4efdde0a6ec9f5580e0651ec60d10e5b9d3875ef`，本地/origin/GitHub一致且工作区干净。不可变功能源码 [`1db9de95e1f770f4a160748b1fcd263175e7071d`](https://github.com/bboytang/CAIdex/commit/1db9de95e1f770f4a160748b1fcd263175e7071d)，[精确CI38155435916](https://github.com/bboytang/CAIdex/actions/runs/38155435916) 四job completed/success，各原始checkout完整SHA核对。封存提交只HANDOFF/设计验收/i5证据，实际HEAD恢复时核实，不当功能CI SHA。

实现：云schema2→3/SQLite3→4保留旧Chat/Memory及待同步队列；独立Chat默认关闭/CAS版本/显式本机conversation范围，Memory/自动记忆/Provider/Host权限不变。固定账户issuer、无redirect的窄HTTP transport；大载荷读取前复用单worker配额，限时/有界单实体。稳定operation+实体hash、原操作收据、数据/事件/幂等同RLS事务；不可变消息父关系/完成及中断原因、附件；下载落盘+cursor同事务且无回声outbox。持久离线stop/未知开启先收据后关闭、CAS冲突显式处理、切账户/重配session迟到拒绝。删除清本地正文/附件/turn/outbox并隐藏；云清title/正文/附件，永久墓碑防旧设备复活；保留独立Memory来源所需不可变消息元数据，专用非owner/NOBYPASSRLS guard只清已删除且本人可写Chat，app仍不能UPDATE消息正文。认证用户/会话非键锁与关闭/撤销互斥，并允许数据事务检查不可变ID外键。

本机实际：定向26 Rust测试/0失败；精确功能PG17.10/vector0.8.2 +真实HTTP I-5为46具名检查/0失败（1 Rust集成），I-2认证为112具名检查/0失败（1 Rust集成），Clippy/fmt/Python compile/链接/diff/36源码指纹通过。I-5真实HTTP server与A/D1、A/D2、B/D3三个隔离Cache实例在一个Rust进程；提交后503/延迟fixture证明未知收据恢复、重启、并发分支、92KB附件、独立Memory控制、账户/项目/跨scope隔离、CAS与关闭、撤销、删除/Memory来源FK不误删。不是独立客户端进程、正式CLI/GUI或物理网络/磁盘容量验收。

CI实际：Linux114517333757 workspace723/0/83、固定Runtime81/0；macOS114517333630 workspace722/0/81、Runtime80/0；Windows114517333629 workspace716/0/81、Runtime80/0。4项新增同步单元已计入workspace，ignored不计通过；Linux原生凭据1/Windows额外Host lib4另记。数据库114517333457真实I-1 118/0、I-2 112具名组/0、I-5 46具名组/0、认证lib5/0。三平台fmt/Clippy/JS/schema/doctor及I-3/I-4.1/I-4.2/H-1/2/3演示均成功。证据：[最终四job及原日志](docs/evidence/i5-ci.json)、[本机最终/开发失败/原日志](docs/evidence/i5-local.json)；14份日志压缩/原文SHA256核对，无ENOSPC。旧证据/固定Runtime/Provider/Host/upstream及旧DDL不变，外部依赖版本/source/checksum不变，仅内部依赖元数据/既有依赖用途调整。

开发失败：默认/tmp有.git保护标记导致缓存拒建，改Git外私有TMPDIR且保留安全规则；最初生成列FK不支持ON DELETE SET NULL，最终保留最小消息标识并受限清正文；跨auth/data的FOR UPDATE自阻塞55P03修为不可变ID的非键更新锁，实际I-2竞争回归通过。原未提交开发失败日志保留，不标最终源码通过。本机起点盘3.6GiB，复用缓存完成必要构建/定向测试后余约1.1GiB/98%，完整workspace及跨平台回归转GitHub，不清理有效缓存。cloud环境状态工具/策略文件未提供，不宣称其就绪；Git/Docker/CI经沙箱提升实际成功。

限制：当前只同步账户自身拥有的个人/项目Chat，无项目协作新入口；新本机conversation需显式选择。服务器单worker串行请求，事件/收据/墓碑永久留存，无生产容量/备份净化/GC承诺；SQLite明文与HTTPS云信任边界，未登记任意秘密仍须用户范围选择。Memory同步/A/B删除归I-6，运营/备份/物理资源归I-7。正式GUI/CLI/iOS、真实模型及F/G-Live未验。

I-5 的历史停止已由用户确认独立审计通过及 I-6 新授权解除；上述功能源码、精确证据及限制保持。当前准确下一步以本文件顶部 I-7 为准，实施会话不授独立审计。

## 已关闭 I-4.2（用户确认独立审计通过）

2026-10-11 用户明确确认 I-4.1 独立审计通过并批准继续下一阶段，按既定拆分续接 I-4.2；这是用户确认，不由实施会话自行授审计。main 起点 ca65343d31decb914d60fdd3d38e7af87742f713，工作区起点干净；当前实际 HEAD/未提交文件恢复时用 Git 核实。有限目标/非目标/通过/停止见 [I-4.2 设计与验收](docs/CAIdex-I42-Host-Memory-设计与验收-V1.md)。允许必要实现/修复/测试/文档/commit/push main/精确 CI；I-5 及后续仍未授权。自动记忆首次默认关闭；不读真实 Key/调用商业 API/邮件/部署；不使用 ponytail 技能。完整构建回归优先 GitHub；本地盘约3.6GiB可用，不清理有效缓存。

已实现且开发验证通过（不代独立审计）：独立 Memory scope；Host 操作者显式配置 Broker/模型/owner、每客户端外发许可和完成任务证据白名单；Host 推理 DTO 不收 Key/端点/工具，账本只存 client/UUID/hash/state；客户端绑定 namespace/Host/完整目标/空间，离线 waiting、原操作 unknown 查询及 hash 重验、取消/撤权/迟到拒绝。完成结果有界临时留 Host，保存后释放/120秒过期/重启 unknown 不重放。同步、正式 SSH/Relay/GUI/CLI 均未实现。Host journal3→4 保留任务/事件/授权，本地 schema2→3 保留原 DDL/数据并保护新 job codec；旧版拒读新版。

不可变功能源码 [`6d4f79af39a75b5ab7b772f9ca4729cc96ff4a5b`](https://github.com/bboytang/CAIdex/commit/6d4f79af39a75b5ab7b772f9ca4729cc96ff4a5b)，[精确 CI38151897766](https://github.com/bboytang/CAIdex/actions/runs/38151897766) 四 job completed/success，原始 checkout 完整 SHA 均核对。Linux114506890169 workspace719/0/83、固定Runtime81/0；macOS114506890261 workspace718/0/81、Runtime80/0；Windows114506890218 workspace712/0/81、Runtime80/0。Host Memory6项、原缓存/journal迁移及Chat/Memory回归计入workspace，ignored不计通过；Linux原生凭据1/Windows附加Host lib4另记。数据库114506890146真实PG17.10/vector0.8.2，I-1 118/0、I-2 110具名组/0、认证lib5/0。三平台fmt/Clippy/JS/schema/doctor与I-3/I-4.1/H-1/2/3演示均成功。

每平台 I-4.2 独立Host/client+Broker演示成功：Memory合成HTTP8（整合6/Embedding2）、固定Runtime HTTP2、真实工具审批/marker1行、丢回应原操作恢复、离线明确恢复、取消、kill/restart unknown不重放、撤权拒绝、上传0、自动首次关闭。内部例子/脚本不是正式CLI/GUI。验收/试用/边界见 [设计与验收](docs/CAIdex-I42-Host-Memory-设计与验收-V1.md)；[最终四job/23个源码指纹/原日志](docs/evidence/i42-ci.json)、[11轮开发/44份原日志](docs/evidence/i42-development.json)。共48份日志压缩/原文SHA256与checkout核对，无ENOSPC；原225份证据、外部依赖完整元数据、固定Runtime/bridge、云源码和旧DDL不变。本地仅fmt/Python compile/文档链接/diff/指纹检查，无完整Cargo构建。

开发中的Clippy/借用、清空引用JSON比较及旧迁移fixture错误均已修；补齐重启后的配对令牌guard。最后两轮Windows失败为显式取消时HTTP连接重置和自有临时目录残留子进程，最终修复且整轮成功；失败原证据保留，不替最终通过。封存提交仅HANDOFF/验收文档/i42证据；实际封存HEAD由Git核实，不当功能CI SHA。AGENTS.md保持原文，本次用户授权记在本交接及验收文档。

限制：仅loopback实际Host进程，不授生产SSH/Relay；Runtime来源仅精确授权的完成状态摘要，不代完整stdout/journal；同步/云观察器归I-5/I-6、未上传；正式GUI/CLI/iOS、真实模型及F/G-Live未验。来源同步RPC可能阻塞Memory锁，≤8暂存结果/4096操作/约4MiB账本，结果120秒过期或重启unknown不保证交付、不重放；不授生产容量/物理满盘/同OS用户隔离。

历史停止已由用户本次确认独立审计通过及 I-5 授权解除；上述证据和限制保持，不由实施会话自行授审计。

## 已关闭 I-4.1（用户确认独立审计通过）

I-4 拆为 I-4.1 本地核心 / I-4.2 Host 接入，不改变 V3 正式 I 阶段。I-4.1 的实施验证与封存已完成，用户本次确认独立审计通过，其历史等待状态解除；本节只保留不可变证据和限制，不把实施自查当独立审计。范围/通过/停止/试用见 [I-4.1 设计与验收](docs/CAIdex-I4-Memory-设计与验收-V1.md)。恢复时 ca65343 main/origin/main/GitHub main 一致且工作区干净；c17c5bd/38146076798 四 job completed/success、36个源码指纹及32份日志双SHA256均核对。I-4.1 对 Host owner 明确返回 WrongCredentialOwner，未实现 Host Memory RPC；本轮 I-4.2 的实际新入口/证据见当前节。

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

[AGENTS](AGENTS.md)、[V3](docs/CAIdex-实施计划-V3.md)、[UI](docs/CAIdex-UI-规范-V1.md)、[Account/Memory/Cloud](docs/CAIdex-Account-Memory-Cloud-设计与验收-V1.md)、[CLI规范](docs/CAIdex-CLI-完整交互与验收规范-V1.md)为既定边界，当前 I-6 不得更改 A–R 顺序或整产品范围。

源码：`chat/core`（内部演示`scripts/i3-chat.py`）；`cloud/core`、`cloud/postgres`、`cloud/auth`；认证复现`scripts/i2-auth.py`，I-1数据库`scripts/i1-database.py`。固定Runtime/Host在`runtime/bridge`/`runtime/host`，H-1/2/3演示`scripts/h1-demo.py`/`h2-demo.py`/`h3-demo.py`；既有凭据/Provider无需重建改写。未解生产吞吐/备份/边界以各验收报告为准；新有证据权限/隔离/完整性缺陷须按批准通过条件修复复验。
