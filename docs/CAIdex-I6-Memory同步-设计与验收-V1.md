# I-6 Memory 同步状态机设计与验收 V1

2026-10-11 用户确认 I-5 独立审计通过并批准启动 I-6。此确认来自用户，不是实施会话自行授予审计。正式 V3 I-1～I-7 顺序与完整产品范围不变。

## 目标、有限任务与依赖

目标：复用 I-2 账户认证、I-4 本地/Host Memory 和 I-5 同步基础，实现账户权威 Memory 同步控制、共享客户端核心及真实 PostgreSQL/HTTP API；保持自动记忆、Chat 同步、Provider 外发和 Host 权限独立。

依赖：I-1～I-5 已由用户确认独立审计通过。起点 main `56ff013bee88b1183865d6af5f702c467fa6783d`，本地/origin/GitHub 一致，工作区干净。I-5 功能源码及实际证据继续复用，不把封存提交当作功能 CI SHA。

有限任务及通过条件：

1. 新账户默认关闭；登录读取并继承 `enabled/settings_version/cloud_epoch/scope_version` 和授权范围。设备首次上传许可单独持久化，游客、其他账户及未确认范围的历史 Memory 不自动转属或上传。
2. 控制请求使用稳定 operation、base version CAS 和幂等收据。离线关闭立即暂停本机数据传输，持久保存待服务器确认的意图；重连/重登录控制优先，未知开启回应不能解除停传。
3. 每次上传、下载页和派生结果提交原子重验账户状态、版本/epoch、会话和项目 ACL。确认关闭之后拒绝全账户的新数据请求及旧 job 提交；控制面不带 Memory 正文，关闭时无云导出绕行。
4. A 停止并保留云端正文；B 复用新鲜的会话绑定重认证证明，明确本机保存选择。先提交关闭/epoch 屏障，再幂等清理正文、revision、专属来源摘录、向量、敏感作业及事件/队列载荷；准确报告实际进度，不误删 Chat、Host journal 或别人的私有资料。
5. 稳定实体/operation、scope、base revision、payload hash；数据、事件和幂等结果同事务。丢提交回应查询/重试原操作，同 operation 异载荷拒绝。不可变 revision 父关系保留离线双方修改；冲突及重新开启须显式选择/合并，不依客户端时间覆盖。
6. 永久墓碑、来源删除标记及 epoch 屏障阻止旧 ID、旧设备、旧 job 和新 ID/旧来源自动复活；明确重新创建须重新授权。B 后其他设备回连收到删除状态，本地保留副本脱离旧云关联，不自动回流。
7. Memory outbox/cursor/错误状态与 Chat 分离；下载落盘和 cursor 同事务、不回声上传。切账户/登出/撤销/迟到结果保持隔离及原账户队列归属，执行端离线继续等待，不复制 Key 或重新执行未知任务。
8. 版本化 PostgreSQL/SQLite 迁移保留旧资料及待同步队列，失败回滚；旧客户端/过期 cursor 不绕删除屏障。若本轮永久保留墓碑和事件，明确无 GC，不能宣称已验证墓碑回收策略。
9. 提供真实 PostgreSQL+HTTP+隔离客户端的复现入口、定向负例、具名检查、不可变源码指纹和原始日志；必要本机检查及精确提交的数据库/三平台 CI，检查最终 diff 后封存。

验收依据：[V3 的 I-6](CAIdex-实施计划-V3.md)、[Account/Memory/Cloud](CAIdex-Account-Memory-Cloud-设计与验收-V1.md) 第 8～9 节、SYN-01～13 及相关 SEC/MEM/REC 项。底层协议/合成账户证据不代正式 GUI/CLI/iOS/真设备安装验收。

非目标：I-7 邮件提供商、备份/恢复演练、容量和生产运营；J～R 正式界面/CLI/平台打包；商业 API、真实用户 Key、公开部署、新账户/同步架构或新增 Host 执行权限。

停止条件：达到上述有限通过条件后提交/push main、核对精确 CI 与原始证据，停止新增功能，等待独立只读审计及用户确认；不自行授独立审计、不启动 I-7。当前范围内的权限、隔离、数据完整性阻断最小修复并相关复验；非阻断及限制记明所属阶段。

## 已确认的项目协作规则

2026-10-11 用户明确采用第一项并批准继续 I-6 编码/测试/验证：

1. 所有者关闭 Memory Sync 后，所有成员暂停访问及同步该所有者的云端项目 Memory。
2. 原项目 ACL、成员身份和 reader/writer 权限保留，不自动撤销。
3. 所有者重新开启后，当前同步范围、双方账户状态和项目 ACL 均有效才能恢复；reader 只读、writer 可写。
4. 成员自身 Memory Sync 必须开启，不绕任一账户关闭。
5. A 保留正文；B 执行删除、epoch/tombstone 屏障，成员不能绕过。
6. 不误删成员独立 Memory、其他项目或本地 Chat。
7. 沿用现有 RLS，真实数据库验证关闭/重新开启、成员读写、撤权及 B 删除。

这项产品决策已解除；不是实施会话默选默认。旧 DDL 保持不可变，在新迁移补充授权范围和删除屏障。

## 实施计划与修改路径

1. 核实控制/项目/删除契约，记录已确认的协作选择，再细化允许字段协议及原子边界。
2. 最小实现云端迁移/控制/数据路径，再实现本地持久状态、许可、队列与 epoch/删除处理。
3. 定向复现默认关闭、首次范围、控制 CAS/未知回应、A/B、多设备 revision 冲突、来源/旧 job 防复活、会话/项目撤销和账户切换；补迁移失败/容量回滚负例。
4. 执行适当本机检查；资源不足部分及跨平台回归在 GitHub 核验精确源码提交；检查最终 diff，归档原始日志和限制后封存。

实际修改的既有调用链：`cloud/core` 协议/schema 常量与测试；新增 PostgreSQL 迁移（不改写旧 DDL）；`cloud/auth/src/server` 控制与普通 RLS 数据事务、router、`ConfirmedSession` 窄 transport；`chat/core/src/cache.rs` 迁移及 `memory` 管理/作业/outbox、共用账户切换状态；真实 HTTP fixture、`scripts/i2-auth.py` 的私有 PG 装配与源码指纹、I-6 复现入口、CI 及本验收/HANDOFF。不改固定 Runtime、Provider wire 或 Host Key 所有权。

B 采用先提交持久停止/epoch 屏障、后清理的顺序；清理失败必须保持拒绝数据请求并可恢复，不把两个独立数据库连接的提交冒充一个原子事务。普通 app 不授全局绕过 RLS；关闭后的清理权限必须窄化到已确认删除的 owner/scope。协议/API 与本地状态机已实现，契约见下节。

## 实际契约与试用

共享 Memory payload 复用 I-4 的 Scope/Kind/Privacy/Origin/Source/Draft，serde 与本地 API 路径保留；协议1，PG schema4、SQLite5，旧迁移不可变。服务器 `/v1/memory/{status,control,scope,push,pull,cleanup}` 复用已确认账户 issuer、会话和无重定向 transport。单请求1MiB、正文16KiB、序列化 draft64KiB、最多64 scope、每scope4096记忆/每记忆1024 revision；每轮一个scope、一个上传/下载事件，持久公平轮询与cursor。服务仍使用既有单worker入口配额/有界读体，不认领生产吞吐。

控制 CAS 使用请求方 settings_version，项目数据另持 owner permit（version/epoch/scope_version/双方有效enabled）。实际提交持请求账户和owner非键锁、项目/成员行锁至数据事务完成，与关闭及撤销排序。RLS 同时检查两账户活跃/enabled/范围、pending和ACL；reader只读。scope控制面可返回有权成员的关闭/删除metadata，不返回正文；关闭不改ACL。实际 HTTP 内两个独立服务 worker 的锁竞争测试验证成员提交先于关闭确认，后续访问拒绝。

数据及事件/收据同普通app RLS事务；operation+scope+payload hash 幂等，submitted_by一致、epoch须当前，旧epoch收据也拒绝。immutable revision 保留两条离线分支，冲突标disputed，不用时间覆盖；显式选择校验head与generation。Chat来源未同步时允许来源引用及摘录，不虚构来源验证或云端Chat读取权。向量不跨wire上传，沿用本地重算；云端既有向量/job受版本和删除屏障保护并在B实删。

B 必须提供 I-2 新鲜会话绑定重认证proof，proof仅用于请求，不持久到SQLite或收据。先提交 disabled/epoch/pending，后清理每次最多32条记忆；每批独立原子清除正文/revision/来源摘录/向量/敏感job并写永久ID/来源hash墓碑，全部完成才清pending。失败保留拒绝屏障，重试不重复开启；最后清剩余本人job/embedding space。事件只有最小ID/操作hash/提交者，不存正文，永久保留事件/收据/墓碑，无GC承诺。

本地继承全账户范围不自动选择旧记忆上传。精确上传许可按本机记忆ID持久保存，游客/LocalOnly/未确认范围拒绝；新修改冻结完整载荷和operation，同SQLite事务入队。下载+cursor同事务，无echo；失败回滚，切账户/重配session迟到拒绝。离线Keep/Delete立即冻结本机，未知Enable必须先查原收据再提交stop。B回连须显式epoch选择，保留副本解除云关联/许可/队列并冻结派生任务；旧ID不能再次选择，明确手动recreate产生新ID并重新授权，自动旧来源仍拒绝。未获云owner确认的未提交项目副本在discard选择下仍保留并冻结，避免误删其他所有者项目；此保守边界不允许自动回流。成员自己的其他scope/私人Memory与本地Chat保留。

内部核心试用（不是正式GUI/CLI）：

1. 用已有 Account 信任流程得到 `Arc<ConfirmedSession>`，`Chat::connect_sync` 连接；它不自动开启Chat或Memory。已有本地project grant只控制本地分区，不授云ACL。
2. `chat.memory().set_sync(Mode::Enable, &scopes, &memory_ids)` 显式范围及Shareable旧记忆；`sync_once(None)` 循环处理控制/队列直到 Round 无活动且 `more=false`。继承Enabled后可用 `select_uploads(&ids)` 显式选择当前范围内本机历史。
3. `set_sync(Mode::Keep, &[], &[])` 为A；`set_sync(Mode::Delete, &[], &[])` 为B，首次 `sync_once(Some(&fresh_proof))`，之后用None重试原操作/cleanup。用 `sync_status` 的control.delete_pending观察真实清理状态。
4. `needs_epoch_choice` 时显式 `reconcile_deleted_epoch(retain)` 或按 `sync_scopes` 的项目状态 `reconcile_project_epoch(&scope, retain)`；选择后仍按当前控制/双方ACL才可传输。`sync_operations(after,limit)`查看稳定operation/blocked，只有已确认冲突能明确discard。
5. `python3 scripts/i6-memory-sync.py --output /absolute/new-report.json` 实际创建受限隔离PG17.10/vector0.8.2容器、普通非owner/NOBYPASS角色、合成账户和四份独立Cache，经真实HTTP执行规则/负例。需要Docker/固定镜像/Rust环境，脚本临时目录在Git外且私有；不读取真实Key或发送邮件。

## 当前实际验证

功能已实现，最终本机PG/HTTP为56具名检查/0失败，cached单元7/0（2项数据库ignored用独立入口），Clippy sync-test全targets通过；首轮精确CI已执行，旧未来schema5及auth不可更新Memory开关断言过时，测试已最小修正，本机真实账户114具名检查/0失败、Chat存储6/0；首轮不授通过，下一精确提交CI仍待核验。早期54具名检查不替最终源码；补充跨epoch收据负例失败已最小修复并重跑通过。原始开发日志位于 `/tmp/caidex-i6-verification`，封存时归档到evidence并绑定完整功能SHA。

本机盘起点39GiB/余1.1GiB/98%，RAM1.9GiB、Swap4GiB；当前余约0.7GiB，保留Rust/PG缓存。按用户要求完成可行的cached Clippy/单元/真实PG检查，完整workspace及跨平台由GitHub验证。默认/tmp有.git保护标记，使用Git外私有TMPDIR，不放宽目录安全规则。云环境状态工具及策略文件未提供，不宣称就绪；Git/Docker经执行沙箱提升实际访问。

限制：合成身份/一个Rust测试进程内隔离客户端与实际HTTP，不是物理多设备或正式GUI/CLI/iOS安装；SQLite明文/HTTPS云信任边界，未登记任意秘密仍需用户授权范围。没有商业模型/真实Key/F-G Live、生产容量/GC/备份净化/EmailSender部署证据；上述留既定阶段。实施验证不代独立审计，I-6完成后停止等待，不启动I-7。
