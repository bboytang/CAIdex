# I-3 独立 Chat 与本地缓存：设计与验收

2026-10-11：用户确认 I-2 独立审计通过并明确批准开发 I-3。基线 main 6176529dc8fca874db878a881028f1e49953881b；I-2 功能 951f92e、精确 CI 38135955674。确认来源为用户，本实施会话不冒充独立审计。

## 目标与有限范围

新增独立共享 Rust Chat 核心：复用 ModelProvider/Router 的 Responses 流，不启动 Runtime/Host，不声明或执行工具。提供 SQLite 版本化本地缓存，游客与可信 issuer/user_id 各自分库，项目在账户库内分 conversation；正文、附件、草稿、待完成轮次和 outbox 均随原归属。模型绑定在切账户时清空，Bearer/ApiKey 必须属于当前本机 Chat namespace，拒绝 Host owner。

消息按父消息形成不可变 DAG：并发请求/编辑产生兄弟分支，不覆盖旧消息；Provider 原生签名、推理 carrier、工具输出不进入可同步正文。流式文本暂存轮次草稿，终态原子追加 assistant 消息及本地 outbox；取消、断流、切账户或进程重启封为 interrupted，不重发未知模型 POST。完成状态复用云端 complete/interrupted，具体结束原因单独表达。

附件作为有界 SQLite BLOB 与元数据，绑定 conversation，与消息一起事务保存引用；模型发送只包含调用者明确选择的同 conversation 附件。请求/响应文本、历史长度、分页、单附件和库大小有界。建立本地变更 outbox，不启动网络同步、不把 pending 当上传许可。

复用已有 Unix/Windows 私有目录保护实现，必要时抽到共享凭据基础设施供 Host 和 Chat 同用；保持 Host 行为与审批架构。账户确认仅增加非秘密身份 getter，不改变 token 信任/存储。

## 非目标与依赖

I-4 Memory Engine/检索/Embedding、I-5 云 Chat 同步/控制/cursor/删除协议、I-6 Memory 同步、I-7 运维，以及 J/M/P 正式客户端不属于本轮。不给账户登录增加项目执行权限；不复制 Host Key、不读取真实用户 Key、不调用商业 API/真实邮件、不生产部署。PostgreSQL 0001/0002 不重写，本轮没有云端数据通道，因此不添加假同步 API。

依赖 I-1 scope/不可变消息契约、I-2 信任身份与账户隔离、F/G ModelProvider/Broker、H 私有目录模式；SQLite 为本地缓存，独立于 Host journal。普通 Chat 的项目标记仅用于本地分区，不宣称服务器项目 ACL 授权。

## 验收与停止

GitHub 三平台执行 workspace fmt/Clippy/测试及既有固定 Runtime 回归；新测试覆盖不可变分支、附件跨会话拒绝、账户/游客隔离、错误与取消/迟到回复、重启不重发、事务失败不丢旧数据、持久 outbox、Host 凭据拒绝及输出 carrier/工具隔离。内部可运行演示使用本地合成 Responses HTTP 服务与既有 Custom Adapter，经真实流收发/附件/切账户/重启，输出可核对 JSON；不当商业 Live/正式 CLI/GUI/iOS 平台验收。

通过条件：上述功能、相关测试和精确提交 CI 实际成功，最小 diff 已检查，源码/原始日志/可复现演示与限制封存。达到后停止功能开发，等待 I-3 独立审计及用户下一里程碑批准；失败须修复或明确记录阻断，不以排队/推送当通过。

## 当前状态

实施前范围已记录；源码、测试和运行证据尚未完成。
