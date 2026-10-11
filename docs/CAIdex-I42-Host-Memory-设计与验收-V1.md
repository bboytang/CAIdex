# I-4.2 Host Memory 接入设计与验收

2026-10-11：用户确认 I-4.1 独立审计通过并授权继续下一里程碑 I-4.2。保持 V3 正式 I 阶段及 I-4.1 不可变证据；自动记忆首次默认关闭。

目标：把现有 Memory 的整合/Embedding 接到明确授权的 Host，使用 Host 自有 Broker/Key，提供真实 Host RPC、来源摘要及离线/恢复/撤权证据。

有限任务：独立 `Memory` Host scope；Host 操作者配置模型/凭据 owner、每个配对客户端的外发许可与执行证据白名单；有界推理 DTO，客户端不能选择端点/Key/工具；与 Runtime journal 分离的操作账本（不存 Key/正文）；客户端显式绑定 Host、作用域及模型身份；离线等待、显式恢复、同操作查询/幂等、取消/撤权及重启未知不重放；完成任务摘要的实时权限核验；定向测试、实际合成 HTTP/Host RPC 演示及三平台精确 CI。

非目标：I-5/I-6 云 Chat/Memory 同步及云观察器、I-7 运维、正式 GUI/CLI/SSH/Relay、商业 API/真实用户 Key/邮件/生产部署、iOS 安装。现有 Host loopback 经后续 L/Q 接远端传输；本轮不能认领生产 SSH/Relay。没有新 Agent、工具执行器或审批链，不修改固定 Codex 0.160.1。

依赖：已通过的 H-3 Host 权限/Runtime/持久化与 I-4.1 Memory 核心，复用 ModelProvider/Router/Broker。原 Chat/Memory schema1/2 DDL、PostgreSQL迁移保持；本地 schema2→3 原子升级版本以保护新增目标固定字段，保留 Chat/Memory/许可/作业；Host journal 原子升级 schema3→4（新增 Memory scope，旧版拒读新版；保留原任务/事件/授权），Host 推理操作账本单独版本化。

通过条件：Execute/Observe/Approve 均不隐授 Memory，Memory 不授 Runtime 执行/审批/全 journal；账户身份不代配对；跨客户端/作用域/模型/空间及 Host 凭据 owner 拒绝；客户端和 Host 分别检查外发许可及已知秘密；模型/工具输出有界且只为数据；离线不发客户端/云模型请求，恢复重验来源/revision/权限/模型目标；已受理操作不重发，丢回应可按原操作查，Host 重启未知不重放；关闭/取消/撤权/删除/切账户拒迟到结果；执行来源真实来自授权的完成任务摘要并在每次读取重新核验；精确 CI/原始日志/实际演示通过，限制和复现步骤完整。

验证：本地只格式/源码/静态/差异检查，完整构建/定向及 workspace/固定 Runtime/数据库回归优先 GitHub Actions。使用合成 Key、loopback Provider 与真实 Host TCP；真实 Codex 完成任务提供执行来源。不调用商业 API。封存不可变功能 SHA、精确运行/job、原始日志哈希及源码指纹。

停止条件：通过后封存并停止功能开发，等待独立 I-4.2 审计及用户确认；不启动 I-5。阻断按当前通过条件最小修复；云观察器及生产传输未验如实记录。

当前：开发通过条件已满足，源码/精确 CI/证据封存；停止等待 I-4.2 独立审计，不授独立审计通过，不启动 I-5。

## 实现边界与调用路径

`runtime/host::serve_with_memory` 复用原 Host transport、配对与 Runtime 循环；原 `serve` 不隐式启用记忆。Host 操作者通过 `MemoryService::open` 提供 `MemoryProfile`（现有 LocalModel/ModelProvider/Router/Host Broker、独立整合/Embedding、完整空间）及 `MemoryGrant`（配对 client_id、逐范围/用途/类型/隐私外发许可、确切 task_id 来源白名单）。模型引用的 credential owner 必须是传入的 Host owner，带 Key 必须有 Broker guard；没有读取/导出 Key 的 RPC，也不接受客户端指定模型端点或认证参数。模型配置/策略在本次 Host 实例内，由应用启动端显式供应；不冒充生产配置页面或正式 CLI 已接好。

新增 scope `memory` 独立于 observe/execute/approve。Memory 客户端不能 snapshot/attach 全 journal、submit Runtime task、probe、批准或给自己授权；其他三个 scope 均不隐授 Memory。owner 管理配对、撤销和完成任务白名单，普通账户令牌不能代配对。推理的 client_id 来自 Host 认证，不从正文读取账户或 owner。每个身份的操作键为 `(client_id, operation_id)`，其他客户端不能 status/cancel/release 其结果。Host 再按完整目标、用途、scope、kind、source type、local-only 许可检查，客户端自身的细许可独立存在。

RPC：`memory/models` 仅返回该配对身份获准的模型描述；`memory/submit` 接收有界 `InferenceRequest`（固定 profile、完整目标、Draft、可选精确 Space）；`memory/status` 返回原状态/请求 SHA256/有界结果；`memory/cancel` 停止模型工作并清暂存结果；`memory/release` 释放已完成的结果，不重发/删除操作身份。`memory/evidence` 只读取被授权的完成任务摘要；`memory/evidence-grant` 是 owner-only 的精确白名单入口，不给 Memory 授 Runtime 工具权限。无云端 Agent 或模型后台代执行。

共享 `chat/core/memory/inference.rs` 从原本地实现抽出有界、无工具的整合/Embedding 调用，客户端和 Host 共用严格候选/向量校验；工具/reasoning 不能伪装正文。Host 模型输出不会执行指令/提升审批权限。Key 和已知配对令牌的 guard 在请求、输出、描述和证据边界生效。每次成功认证重新登记当前配对令牌，重启后仅持有摘要的旧授权也不能把其令牌发给模型；共享 guard 去重，不按每次连接无限新增对象。只防已知值/认证结构，不声称能识别所有未知秘密。

`MemoryEngine::configure_host_integration/configure_host_embedding/configure_host_resolver` 为显式应用接口；绑定 Namespace、Host ID、作用域、完整真实 Provider 端点/native model/Host profile 组合身份和空间。Host 身份作为目标 endpoint 的前缀，真实 Provider 端点仍完整可核对；别名/Host/端点/native model/空间改变不能继承旧许可。角色绑定只在当前会话，重启客户端必须重新配置；Host 模型完全替换对应本地角色，没有客户端 Key 回退路径。普通 Chat 仍只使用自己的本地角色。

## 离线、持久化与结果交付

本地等待作业经自动开关、来源新鲜度、项目许可和外发许可检查后才尝试 Host。连接前离线或只读预检不可达时恢复 waiting，未发模型请求；恢复由应用明确 run 原 waiting ID，重新核验全部边界。Host 受理前事务写 `memory-operations.sqlite3` 的 client/UUID/hash/state，数据库与 Runtime journal/客户端 cache 分开，不存正文、结果或 Key。相同操作/相同请求返回原状态；相同操作/不同请求拒绝，不启动第二次模型工作。

开始发送提交后连接/响应丢失视为 unknown；客户端保留原引用和固定目标，`recover_host` 只查原 ID，不调用 submit。收到 done 结果还核验完整请求 hash，并再次校验来源/revision/空间/权限/epoch。客户端保存后显式 release；结果最多暂存120秒。过期、Host 重启或客户端落盘失败都不能承诺结果可交付，只允许 unknown/取消，不重播。没有“模型 POST/本地 SQLite/Host 账本跨进程恰好一次”承诺。

Host 重启将 running/done 封存 unknown；旧 ID 重提也只返回原 unknown。取消/撤权先取消 token/清暂存结果，再拒迟到完成；本地关闭、删除、纠错、切账户、撤销外发/项目权限沿用 I-4.1 generation/epoch 与 Flight，同时通知 Host 取消。未知 POST 不能直接重发。账户切换清模型/来源绑定；旧 HostModel/HostConnection 不能配置到新 Namespace。丢失网络的撤销/取消不能收回第三方已收到的正文，但本地和 Host 均不提交迟到结果。

现有 Host journal schema3→4 是事务版本升级，保留原 clients/任务/artifacts/操作/事件；新 Memory scope 不被旧二进制误读。客户端 schema2→3 保留原 schema1/schema2 DDL、Chat/Memory/许可/作业，升级版本保护新增固定 target 字段，旧版拒读新版，不提供数据降级。Host 操作账本 schema1 有独占锁、owner 文件/链接检查、未来版本拒绝与 SQLite容量上限。

## 来源与有界限制

执行来源的 reference 是 `host:<host_id>:<task_id>`。每次读取建立授权 RPC，核对 Memory scope 与 owner 配置的 task/scope 白名单，只有当前 journal 中 completed 的任务才能返回 Host/task/thread/turn/seq/Codex版本摘要。该摘要证明任务完成状态，**不证明完整命令 stdout、文件/Git事实或全部原始 Runtime journal 已导入**；后者继续使用实际对应来源及权限。撤权、离线、重启未重新供应证据白名单、摘要变化都使旧证据不可用/非当前，不能当已验证事实检索。

可信 `EvidenceResolver` 仍是同步接口：Host resolver 用标准 TCP、每次超时5秒重新检查，不缓存“已验证”布尔值；应用应在有工作线程的环境调用，可能暂时阻塞本地 Memory 锁，不授 iOS/UI 响应性能通过。Host 对外仍是原 numeric loopback 协议，本轮独立进程验证真实 RPC；生产跨机器 SSH/Relay、安全隧道与实际公网网络故障归 L/Q/R。

Host ≤16模型、≤64绑定、每绑定≤1024许可/来源，≤8未释放推理/结果，模型 deadline120秒；入站 frame≤64KiB，Memory 描述/结果 JSON≤60KiB，单正文≤16KiB，源/标签沿用 I-4.1 有界约束。Embedding维度仍1～16000，但大向量若 JSON 超过 frame 上限明确拒绝；本轮不证明所有维度/数字表示组合兼容。账本≤4096操作、约4MiB容量，容量达到后拒新操作，不删旧身份让操作复活；不授生产吞吐/长期运维。结果释放/过期不会删除最少操作身份。SQLite是 OS owner 保护的明文，不防同OS用户/管理员。

## 可复现试用

在有足够空间的独立 checkout 中安装固定 Codex0.160.1并设置 `CAIDEX_CODEX_BIN`；只需临时合成端点/Key，不配置真实账户或商业 Key：

```sh
cargo test -p caidex-chat -p caidex-host --locked
python3 scripts/i42-host-memory.py --output i42-host-memory.json
```

取得→启动→配置→操作→判定：checkout 最终功能 SHA；脚本构建两个内部例子并启动独立 Host/client、两个 loopback模型服务与可控丢回应代理；自动生成配对令牌、只在 Host 注入专用合成 Broker Key，客户端没有模型 Key；脚本操作默认关闭/明确开启、整合与Embedding、丢回应只查原操作、离线等待恢复、固定 Runtime审批/真实文件副作用/授权来源摘要、取消、杀 Host/重启 unknown、撤权；只有所有断言成功才输出 `status=ok`，并检查账本无 Key/正文、没有模型重播或云上传。失败输出原错误，不能用已推送或排队代通过。脚本清理自己的临时目录/进程，不清理用户缓存。内部例子不是正式 GUI/CLI；完整 CI 实际结果见下方封存证据。

## 验收映射（精确 CI 验证通过，限下表边界）

| 通过条件 | 实际验证入口 | 明确限制 |
| --- | --- | --- |
| 独立授权/身份/隔离 | Host TCP scope/目标/范围/类型/local-only/空间/跨client/owner边界测试 | 不授账户认证或生产 SSH/Relay |
| 默认关闭/无Key回退/离线恢复 | 共享 Memory waiting、离线恢复测试及独立进程脚本 | 模型质量/费用仍 F/G-Live 未验 |
| 幂等/丢回应/unknown | 原操作 query+hash、实际代理丢回应、kill Host重启 | unknown不保证交付，不重发 POST |
| 取消/撤权/删除/切账户 | Host待完成任务、共享epoch/Flight测试、真实HTTP取消 | 第三方已收到正文不可收回 |
| Runtime来源/审批不提升 | 固定Runtime真实工具/审批、完成任务白名单摘要；Memory恶意工具输出拒绝 | 摘要不是完整 stdout/journal导入 |
| 迁移/数据完整性 | 原DDL构造schema2缓存；v3 journal客户端/artifact保留及注入回滚；账本锁/未来版拒绝 | 不授物理VPS满盘/灾难恢复 |
| MEM-10 Host执行端子项 | 实际离线/恢复/撤权/重启原操作链 | 云观察器与云作业协议尚归 I-5/I-6，未执行、同步关闭上传0 |

## 不可变验收证据与停止点

功能源码 [`6d4f79af39a75b5ab7b772f9ca4729cc96ff4a5b`](https://github.com/bboytang/CAIdex/commit/6d4f79af39a75b5ab7b772f9ca4729cc96ff4a5b)，[精确 CI38151897766](https://github.com/bboytang/CAIdex/actions/runs/38151897766) 四 job 均 completed/success；每份原始日志 checkout 完整 SHA 核对。封存仅文档/证据，不把封存 HEAD 代成功能 CI SHA。

| 实际 job | workspace passed/failed/ignored | 固定 Runtime passed/failed | 其他结果 |
| --- | --- | --- | --- |
| Linux114506890169 | 719/0/83 | 81/0 | 原生凭据1/0；Host Memory演示成功 |
| macOS114506890261 | 718/0/81 | 80/0 | Host Memory演示成功 |
| Windows114506890218 | 712/0/81 | 80/0 | 附加Host lib4/0；Host Memory演示及进程/临时清理成功 |
| 数据库114506890146 | 不混入workspace | 不执行Runtime | PG17.10/vector0.8.2，I-1 118/0，I-2 110具名组/0，认证lib5/0 |

Host Memory6项、原schema2缓存/journal3迁移和原Chat/Memory回归已计入workspace，ignored不计通过。fmt/Clippy/JS/固定schema/doctor及I-3/I-4.1/H-1/2/3演示均成功。每平台实际I-4.2链：Memory HTTP8=整合6+Embedding2，Runtime HTTP2，原生批准后真实marker1行，原操作丢回应恢复、离线waiting明确恢复、取消/撤权拒绝、kill/restart unknown不重放（重播0）、上传0、首次自动关闭。合成端点/Key不授商业Live。

[最终证据索引](evidence/i42-ci.json) 保存4个job/实际步骤/演示/数据库摘要、23个功能文件SHA256和4份完整原始日志；[开发证据](evidence/i42-development.json) 保存11轮/44份原始日志，包含Clippy/借用、清空引用JSON比较、旧迁移fixture、Windows取消时连接重置及自有临时目录子进程清理失败，均已修且不当通过。共48份日志压缩/原文SHA256与checkout核对，无ENOSPC；原225份证据未变。固定Runtime/bridge、云源码、旧DDL以及外部依赖版本/source/checksum/依赖元数据不变。仅Host/chat调用路径和本轮测试/脚本/CI必要改动；最终功能diff已检查。本地fmt/Python compile/文档链接/diff/源码指纹检查通过，完整Cargo构建回归由上述精确GitHub CI执行。

没有独立审计结果由本实施会话授予。到此停止新增功能，等待独立I-4.2审计及用户确认/下一里程碑新授权；I-5/I-6观察器及同步、正式远端传输/GUI/CLI/iOS、商业模型与生产容量均未认领。
