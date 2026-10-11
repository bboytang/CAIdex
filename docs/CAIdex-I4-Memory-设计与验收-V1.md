# I-4.1 本地 Memory Engine：设计与验收 V1

2026-10-11：用户确认 I-3 独立审计通过，并明确授权下一阶段 I-4。用户确认自动记忆首次默认关闭。实施会话不认领独立审计。用户随后确认拆分为 I-4.1 本地核心与 I-4.2 Host 接入：先封存并独立审计 I-4.1，再另行批准 I-4.2；不改变 V3 的正式 I 阶段或顺序。

目标：在 I-3 本地账户分区内实现个人/项目记忆、来源与不可变 revision、纠错与冲突、授权基础/语义检索；复用 ModelProvider/Router/Adapter/Broker，实现独立整合与 Embedding 角色、精确空间版本及 Provider 外发许可。本内部里程碑验证客户端执行端；Host 推理执行/独立授权恢复由 I-4.2 提供实际证据，不用来源读取 callback 或 Runtime 回归冒充 Host 记忆执行。

有限任务：版本化 SQLite 迁移；手工创建、查看、纠错、候选批准、删除及导出；来源新鲜度校验；CAS 冲突保留双方；删除来源墓碑防自动复活；自动开关与本地任务等待/取消/重启未知终态；按账户/项目/隐私授权检索；Embedding 契约和显式兼容端点；普通 Chat 有界记忆注入；合成模型/实际本地来源的复现及三平台精确 CI。

非目标：I-4.2 Host 记忆推理 RPC/独立 Memory 权限与远端执行（本轮未授权）、I-5 Chat 同步、I-6 Memory 同步、I-7 生产运维、正式 GUI/CLI、生产邮件/部署、商业模型与真实用户 Key、iOS 安装验收。项目本地授权不代云 ACL；Host 离线不能隐式转用客户端 Key 或云执行。本轮不新增 Host RPC 或权限，不修改固定 Codex 0.160.1 的工具/审批链。

依赖：I-1 数据定义与 RLS、I-2 身份、I-3 namespace/cache；Account/Memory/Cloud §§6–7 与 MEM-01～10；V3 I-4；固定 Facade/ModelProvider/Registry/Router/Gateway/Broker。旧 PostgreSQL 迁移与 I-3 schema 1 不修改，新本地 schema 2 事务迁移保留 Chat。

通过条件：无模型可手工保存/检索，整合与 Embedding 任务明确等待；来源与 AI 推断类型可见；原始 Chat/文件/Git/执行证据保持区分，新鲜度失效不能检索为事实；纠错 CAS 不最后写入覆盖；账户/项目撤权在读取和异步提交前生效；自动关闭停止提取/注入但手工 CRUD 可用；删除清理正文/向量/独占来源，旧来源不能自动复活；不同整合/Embedding/Chat 模型独立，精确空间版本与 revision 不混用；Key 归属和外发许可各自检查，已知凭据不入记忆/导出/模型正文；撤销/删除/切换/取消拒迟到结果，未知 POST 不重播；相关测试及精确 CI 通过，原始证据与限制可独立复现。

证据计划：定向 Rust 边界测试、合成 HTTP Embedding/整合及 Chat 注入复现；GitHub 三平台 workspace/fmt/Clippy、固定 Runtime/Host 及数据库认证回归。只本地静态/格式/差异检查，构建前检查磁盘，不清理有效缓存。只读独立审计前封存精确源码 SHA、CI 原始日志和试用步骤。

停止条件：达到上述条件后封存并停止功能开发，等待独立 I-4.1 审计及用户确认；I-4.2 须另行明确批准。发现当前条件内阻断则最小修复复验；不得启动 I-5。

当前状态：实施中，尚未验证通过。实现与实际证据将在封存时补充，计划不作为完成证据。

## 已确认的 Host 接口缺口与拆分

`runtime/host/src/service.rs:33` 的 Request 只包含观察/Runtime task/审批/客户端授权；`access.rs:13` 只有 Observe/Execute/Approve。普通记忆推理不应获得 Runtime 工具执行权限。用户已选择 I-4.1 / I-4.2 分别审计与批准；本轮不复用 Execute 偷授 Memory，不新增 Agent，不读/复制 Host Key。I-4.2 需提供独立 Memory 授权、Host 自有 Broker/Key、在线/离线/恢复/撤权、真实 Host RPC 与精确 CI。此次拆分保留 V3 I-4 的完整功能范围，I-4.1 完成不等于整个 I-4 完成。
