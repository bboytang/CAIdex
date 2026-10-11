# I-4.2 Host Memory 接入设计与验收

2026-10-11：用户确认 I-4.1 独立审计通过并授权继续下一里程碑 I-4.2。保持 V3 正式 I 阶段及 I-4.1 不可变证据；自动记忆首次默认关闭。

目标：把现有 Memory 的整合/Embedding 接到明确授权的 Host，使用 Host 自有 Broker/Key，提供真实 Host RPC、来源摘要及离线/恢复/撤权证据。

有限任务：独立 `Memory` Host scope；Host 操作者配置模型/凭据 owner、每个配对客户端的外发许可与执行证据白名单；有界推理 DTO，客户端不能选择端点/Key/工具；与 Runtime journal 分离的操作账本（不存 Key/正文）；客户端显式绑定 Host、作用域及模型身份；离线等待、显式恢复、同操作查询/幂等、取消/撤权及重启未知不重放；完成任务摘要的实时权限核验；定向测试、实际合成 HTTP/Host RPC 演示及三平台精确 CI。

非目标：I-5/I-6 云 Chat/Memory 同步及云观察器、I-7 运维、正式 GUI/CLI/SSH/Relay、商业 API/真实用户 Key/邮件/生产部署、iOS 安装。现有 Host loopback 经后续 L/Q 接远端传输；本轮不能认领生产 SSH/Relay。没有新 Agent、工具执行器或审批链，不修改固定 Codex 0.160.1。

依赖：已通过的 H-3 Host 权限/Runtime/持久化与 I-4.1 Memory 核心，复用 ModelProvider/Router/Broker。原 Chat/Memory schema1/2 DDL、PostgreSQL迁移保持；本地 schema2→3 原子升级版本以保护新增目标固定字段，保留 Chat/Memory/许可/作业；Host journal 原子升级 schema3→4（新增 Memory scope，旧版拒读新版；保留原任务/事件/授权），Host 推理操作账本单独版本化。

通过条件：Execute/Observe/Approve 均不隐授 Memory，Memory 不授 Runtime 执行/审批/全 journal；账户身份不代配对；跨客户端/作用域/模型/空间及 Host 凭据 owner 拒绝；客户端和 Host 分别检查外发许可及已知秘密；模型/工具输出有界且只为数据；离线不发客户端/云模型请求，恢复重验来源/revision/权限/模型目标；已受理操作不重发，丢回应可按原操作查，Host 重启未知不重放；关闭/取消/撤权/删除/切账户拒迟到结果；执行来源真实来自授权的完成任务摘要并在每次读取重新核验；精确 CI/原始日志/实际演示通过，限制和复现步骤完整。

验证：本地只格式/源码/静态/差异检查，完整构建/定向及 workspace/固定 Runtime/数据库回归优先 GitHub Actions。使用合成 Key、loopback Provider 与真实 Host TCP；真实 Codex 完成任务提供执行来源。不调用商业 API。封存不可变功能 SHA、精确运行/job、原始日志哈希及源码指纹。

停止条件：通过后封存并停止功能开发，等待独立 I-4.2 审计及用户确认；不启动 I-5。阻断按当前通过条件最小修复；云观察器及生产传输未验如实记录。

当前：实施中；未授任何验证通过。
