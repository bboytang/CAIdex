# I-5 Chat 同步设计与验收 V1

2026-10-11 用户确认 I-4.2 独立审计通过，批准启动 I-5；本机满足条件时优先验证，否则 GitHub。正式 I 阶段及 I-1～I-7 顺序不变。

## 有限范围及验收

目标：复用 I-2 真实账户/会话与 PostgreSQL/RLS、I-3 隔离 SQLite，实现共享 Rust Chat 同步核心与真实 HTTP 数据 API。两个同账户测试客户端可恢复完成/中断消息、不可变父关系/并发分支与附件；第三个不同账户验证隔离。独立 Chat 控制默认关闭，不改变 Memory 开关、自动记忆、Provider 外发或 Host 权限。

任务与通过条件：

1. 版本化 PostgreSQL/本地迁移保留原数据和待同步队列；失败事务回滚，旧程序拒读新库。
2. 真实认证派生 owner，独立控制 CAS 和幂等；先读取权威控制后处理数据，离线关闭本机立即停传并持久等待确认。每个数据请求核对会话、当前 Chat 开关/版本和项目 ACL，不上传游客或其他账户内容。
3. 显式确认本机待上传 conversation 范围；允许字段 DTO、有界单实体上传/下载页、持久 outbox/游标与稳定 operation ID。丢回应重试原操作；同 ID 异载荷拒绝，数据/事件/幂等结果同事务。下载落盘与 cursor 同事务，无下载回声上传。
4. 消息保持不可变 ID、父关系及完成/中断原因；并发编辑为独立分支。附件字节/内容 hash/元数据与 conversation 归属一致，不上传模型载体、草稿或 Host journal。
5. 删除 conversation 清理正文/附件/相关待同步载荷，保留最小永久墓碑；晚到写入/旧设备不得复活。删除事件不保留敏感正文。切账户、登出、撤销、断网、重启及迟到回应不泄漏或丢队列。
6. 提供真实 PostgreSQL+HTTP+两个共享核心测试客户端的可复现流程、负例与原始证据；定向测试、本机可行检查、精确提交三平台及数据库 CI；最终差异检查并封存。

依赖：I-1/I-2/I-3/I-4.2 已由用户确认独立审计通过；固定 Codex0.160.1、Broker/Provider/Host 原约束继续有效。依据：[V3](CAIdex-实施计划-V3.md)、[Account/Memory/Cloud](CAIdex-Account-Memory-Cloud-设计与验收-V1.md) SYN-11/SEC/REC/OPS 中对应 Chat 核心项。正式平台交互由 J–R 补验。

非目标：I-6 Memory 同步/删除 A/B、I-7 邮件/备份/容量、正式 GUI/CLI、SSH/Relay、商业模型/真实 Key、公开部署。项目标记不新建云项目或授予 ACL；既有云项目由明确授权设置，不能用本地 ID 越权。

停止：通过上述条件后提交/push main、核对精确 CI 与原证据，等待独立审计及用户确认；不自行授审计、不启动 I-6。阻断以失败证据为准，当前范围内最小修复；其他边界如实记录。

## 实施与实际证据

当前：实现/开发验证/精确CI与证据封存完成，等待独立审计；实施会话未授独立审计。起点 main `4efdde0a6ec9f5580e0651ec60d10e5b9d3875ef`，本地/origin/GitHub 一致，工作区干净。Rust1.99.0/cache/pgvector 镜像在本机；盘可用约3.6GiB、总内存1.9GiB，构建前逐次检查；不清理有效缓存。网络及 Docker 需要执行沙箱提升，既有授权涵盖测试和 main/CI。


## 最终实现契约与试用

- 共享协议在 `cloud/core/src/chat.rs`，账户 transport 在 `ConfirmedSession::chat_request`，仅向既定 issuer 的 `/v1/chat/status|control|push|pull` 发 Bearer，不跟随 redirect。Chat 数据接收前使用既有单 worker 配额，不排队大载荷，10秒读 body、12MiB wire、30秒客户端 deadline。纯账户认证仍32KiB。
- 服务端 Chat 默认关、独立 `chat_settings_version`；control 使用 operation ID/base version CAS。数据哈希覆盖稳定 operation+允许字段实体，版本每次取当前控制状态重验；回应未知后原 ID/同内容可以跨控制版本恢复已提交收据，但不能重应用或复活。数据/事件/收据同普通 RLS 事务；认证事务的用户/会话非键更新锁覆盖数据提交，仍与关闭/撤销互斥且允许对不可变 ID 的外键检查。
- 云 schema2→3、SQLite3→4；原 Chat/Memory/Host 编码不重写。当前传输当前账户拥有的个人/项目 Chat；本地项目标记不创建项目、不授其他所有者或协作授权。无项目协作新入口。Memory 云控制及源来源确认属于 I-6。
- 客户端 `connect_sync(Arc<ConfirmedSession>)` 必须匹配当前 namespace；`set_chat_sync(enabled, &[conversation UUID])` 明确批准本机历史范围，未选的本机记录继续保留、不上传。新 conversation 仍需明确选择；云恢复 conversation 的未来分支在已确认范围中。每次 `sync_once()` 至多发送一个实体、下载一个事件；可循环到 Round 无上传/下载且无控制待确认，不内建常驻消费者。
- SQLite intent/outbox/cursor 独立于 Memory；离线关闭立即停传，原开启操作未知时保留原操作并记录 stop_requested，迟到开启成功后仍先确认关闭。CAS 冲突留存意图，只有已确认冲突可通过 `discard_chat_control_conflict()` 显式处理；它不能丢弃未知回应或解除待关闭意图。切账户/重配 session/本机关闭后拒绝迟到数据。
- 消息/父关系/完成或中断原因不可变，附件先于消息传输，单附件8MiB、每消息≤4附件/1MiB内容。下载不回声入 outbox，实体落盘与 cursor 同事务；库容量失败保留旧 cursor、队列和正文。
- `delete_conversation(UUID)` 要求先取消该会话的未完成本地轮次。清本地正文/附件/turn/outbox，隐藏 conversation、保存墓碑及已授权删除引用。服务器同事务清 title/附件/消息正文，保留独立 Memory 来源引用所需不可变消息标识和分支元数据；专用非 owner/NOBYPASSRLS guard 只允许清已删除且本人可写 Chat 的正文，普通 app 仍无正文 UPDATE。不会连带删除经过独立批准的 Memory。永久墓碑/最小事件及 payload hash 阻止旧来源 ID 恢复，无事件正文历史、无墓碑 GC。

可复现（Linux、固定Rust1.99.0、Docker既有 pinned pgvector 镜像；使用Git外私有临时目录）：

```sh
cargo test -p caidex-chat -p caidex-cloud-core --locked
cargo clippy -p caidex-chat --features sync-test --all-targets --locked -- -D warnings
python3 scripts/i5-chat-sync.py --output /absolute/new/i5-chat-sync.json
```

脚本复用 I-2 私有 PostgreSQL 装配，测试容器 network none/512MiB/2CPU/tmpfs256MiB，创建本轮 synthetic 账户/会话并经可信后端 me 验证。实际 Rust HTTP server + A/D1、A/D2、B/D3 三个隔离 Cache 实例；不是三个独立客户端进程或正式终端。输出具名检查、PG/vector版本、源码 SHA256 和真实 test result；目标文件已存在则拒写。测试 middleware 在服务端实际提交之后将回应替为503/暂缓，证明未知结果恢复与迟到拒绝，不冒充实际物理断线压测。已保存 Broker/session 凭据与明显秘密字段阻断同步；未登记任意内容秘密仍须用户范围选择，不承诺通用秘密识别。

环境与限制：本机定向26项及真实PG/HTTP场景已验；末次完整证据在封存时登记。开始盘3.6GiB，经复用缓存的编译后约1.8GiB，无 ENOSPC，不清有效缓存；完整跨模块构建/回归转精确 GitHub。云事件/墓碑永久保留，当前没有生产容量、物理满盘、备份净化或 tombstone GC 承诺；两客户端不是 GUI/CLI/iOS/真实多设备安装包验收。服务器串行账户请求，未授吞吐或2核2GB生产支持容量。


## 封存与实际结果

不可变功能源码 [`1db9de95e1f770f4a160748b1fcd263175e7071d`](https://github.com/bboytang/CAIdex/commit/1db9de95e1f770f4a160748b1fcd263175e7071d)，[CI38155435916](https://github.com/bboytang/CAIdex/actions/runs/38155435916) 四job全部 completed/success。每job原始checkout完整SHA、每份源码指纹与Git不可变内容核对；封存提交只补文档/证据，不替功能CI SHA。

| 环境 | 实际结果 | 原始证据 |
| --- | --- | --- |
| 本机Linux/必要定向构建 | 26 Rust测试/0；最终Clippy/fmt/Python compile/链接/diff通过；精确I-5 46具名组/0，精确认证112具名组/0，真实PG17.10/vector0.8.2 | [本机/开发证据](evidence/i5-local.json)，36个I-5源码指纹；10份原日志 |
| GitHub DB job114517333457 | I-1 118具名组/0；I-2 112具名组/0；I-5 46具名组/0；认证lib5/0；两项真实HTTP Rust集成各1/0 | [最终CI](evidence/i5-ci.json)及对应原日志 |
| GitHub Linux114517333757 | workspace723/0/83 ignored；固定Runtime81/0；原生凭据附加1/0；全部演示/静态检查通过 | 同上 |
| GitHub macOS114517333630 | workspace722/0/81 ignored；固定Runtime80/0；全部演示/静态检查通过 | 同上 |
| GitHub Windows114517333629 | workspace716/0/81 ignored；固定Runtime80/0；额外Host lib4/0；全部演示/静态检查通过 | 同上 |

workspace已含4项新增同步单元及原Chat/Memory/Host回归；具名HTTP检查组不是Rust测试数，ignored不算通过。本机实际保留记录：单进程独立账户服务注册/验证/登录4.833秒、CPU4.81秒、idle RSS12640KiB/峰值78616KiB，2逻辑CPU/约1.9GiB机器；不是同期用户负载/生产容量验收。14份本机/CI原日志均校验gzip/raw SHA256，无ENOSPC；最终本机盘约1.1GiB/98%，不重跑完整workspace、不清有效缓存。原证据和固定Runtime/Provider/upstream未改，外部依赖版本/source/checksum未变。

开发中的/tmp Git保护拒建、生成列FK删除方案不可用和两轮外键锁55P03失败完整保留；分别修复测试TMPDIR、受限正文擦除及用户/会话非键锁。后两轮本机I-5、最终精确功能I-5/I-2及四jobCI通过，失败不替通过、未提交失败源快照不冒充最终SHA。后续只有相关变化/新失败证据才补测，不扩展功能。

停止条件已达到：本轮封存停止，等待独立只读审计及用户确认；I-6/后续仍未授权。独立审计须复查未知控制/上传恢复、事务与会话锁、RLS/guard擦除、墓碑与来源引用，以及实际日志/限制。不得把实施自查或上述成功测试当成独立审计。
