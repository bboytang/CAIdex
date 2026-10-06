# CAIdex 实施计划 V3

执行基准：2026-10-06。本文件将已确认的 V3 调整固化为开发阶段；原 V2 保留需求背景，冲突处以本计划和已确认 UI 规范为准。

## 产品与架构

- 平台：Windows 11 x64、Linux x86_64 Host/CLI、iOS 17+；Windows/iOS 默认简体中文及可选英文，CLI 英文。当前不开发 Android、Web 客户端或 macOS 原生客户端。
- Windows：Tauri 2 + React/TypeScript/Rust。iOS：SwiftUI，先 Swift 5 模式；Rust 共享客户端/模型/同步/远程核心经 UniFFI 接入。
- Codex 使用固定 commit 的真实 Runtime/app-server，最小补丁；协议边界隔离上游。接口在验证后版本化，允许明确迁移，不实施原 V2 的永久冻结。
- 普通 Chat 在客户端独立调用模型核心，没有 Shell/Git/项目写权限；与 Codex 共用模型/凭据基础能力。模型和工具支持以实际兼容性报告为准。
- Codex Gateway 对外提供 Responses HTTP/SSE，向内适配提供商；保存不透明签名/推理信息，统一工具 JSON/文本消息，不执行工具。
- Provider 分阶段接入：OpenAI、Anthropic、自定义 Responses、Gemini、兼容 API（DeepSeek/Qwen/OpenRouter 等）及 Ollama。每个模型单独验证支持范围。
- 模型在轮次边界切换；同提供商仅支持已验证兼容组合，跨提供商使用关联分支/新线程及适配后的历史。
- 凭据绑定执行端，Windows 系统凭据、iOS Keychain、Linux 受保护存储；不进入 Git、日志和历史同步。手机 Remote 使用 Host 保存的凭据。
- Host 独立管理 app-server 与线程，GUI/SSH 退出不终止任务；进程/机器重启不盲目重跑工具。Runtime 是执行、队列、Steer 和审批的真源。
- 捕获事件按 Host/stream 编号，落盘后广播；快照恢复缺口，提交幂等，审批竞争首次有效处理生效。区分未确认请求和明确失败，不承诺外部操作恰好执行一次。
- Remote 先 SSH：Windows/CLI 系统 OpenSSH，iOS 经共享核心接入。后续 Relay 从生产首版加入成熟端到端加密方案（Noise/Snow），配对身份、撤销与重放处理一起验收。
- Chat 自动同步到自托管 VPS：SQLite 本地缓存、outbox、变更 cursor、不可变消息、并发分支、删除 tombstone、附件同步。同步完成/中断轮次，API Key 不同步；HTTPS 服务端属于信任边界。
- GitHub 新公共仓库 `bboytang/CAIdex`。公共 CI 最小权限，不向不可信 PR 提供秘密；iOS 先无签名构建，签名/TestFlight 与真机测试以后具备条件再执行。

## 阶段与验收

各阶段以可验证产物推进。依赖先完成；协议、Gateway、生命周期等高风险验证优先于大量 UI。

| 阶段 | 工作 | 通过条件 |
| --- | --- | --- |
| A 环境与基准 | 仓库、工具盘点、上游 commit 与许可证锁定、计划/UI/品牌归档 | 状态可续接、来源可核对、工具版本可重现 |
| B 协议风险验证 | 真实 app-server 初始化/线程/事件；双向请求、超时/乱序/断开；扩展能力清单 | 本机 doctor 与协议回归通过；未验证的能力明确列出 |
| C 工程与 CI | 按既定 monorepo 路径建立所需工程，锁依赖和 action SHA；Linux/Windows/macOS 基础检查 | 各 runner 实际运行通过；iOS 工程建立后追加真实 iOS 检查 |
| D Runtime 边界 | 完整线程/turn facade，审批、用户输入、工具事件、能力协商与 CLI 对照 | 不吞事件/请求；与固定上游行为一致，未知能力不冒充支持 |
| E 凭据 | 执行端凭据存储、配置归属、统一脱敏 | 秘密不出现在代码、日志、快照和同步；跨设备归属正确 |
| F 模型核心与 Gateway | Responses 流、取消、工具 normalization、opaque 信息、错误/限流 | 协议测试服务验证后，用授权凭据做真实模型兼容性验证 |
| G 多模型 Codex | 各 Provider 工具/推理/切换与模型注册 | 每模型有版本化兼容性报告，未支持能力清楚展示 |
| H Host 与持久化 | 背景 Host、SQLite、事件 journal、快照、请求幂等/审批竞争、线程恢复 | GUI/连接退出任务继续；断连/重启恢复符合运行事实 |
| I Chat/同步核心 | 独立普通 Chat、增量输出/附件、VPS 同步、冲突/删除/设备管理 | 无执行工具权限；离线编辑/同步恢复/附件与隔离测试通过 |
| J Windows 框架 | 应用壳、会话/Composer、模型/Key 设置、主题/i18n、自有品牌 | 本机可做的检查通过，Windows 环境运行/构建检查通过 |
| K Windows Codex | Host/项目/Worktree、执行、审批、Queue/Steer、Diff/Review | 真实 Runtime 端到端流程通过，UI 状态可对照 |
| L SSH Remote | Host 服务、SSH 建连/转发、配对、连接恢复、多 Host | SSH 断开不终止任务；同一线程多端接管与竞争处理正确 |
| M iOS 基础 | SwiftUI 项目、UniFFI/XCFramework、原生导航/状态、品牌与 i18n | GitHub macOS runner 的 simulator 构建/测试、无签名 archive 通过 |
| N iOS Chat | 本机模型配置/Chat、自动同步、草稿/附件 | 不混用 Host Key；与 Windows 历史正确同步 |
| O iOS Codex | Host/任务、审批、Queue/Steer、Changes/Diff/评论 | 远程真实任务与审批有效；恢复前台后状态重新核对 |
| P CLI 整合 | 上游 TUI/exec、CAIdex 配置入口、共享 Host/Remote attach | 保持原生英文行为；明确 attach，不暗中回退为独立 Runtime |
| Q Relay | Noise/Snow、配对身份、设备撤销、重放、转发恢复 | 首次生产发布即有 E2EE；中继无法读取任务载荷 |
| R 最终验收 | UI 截图/交互精修、回归、打包/升级、真机与真实 Windows 环境 | 平台检查及限制可核对；无未验证能力被标成完成 |

UI 实现内部顺序参考 UI 规范，整个项目顺序以上表的 Runtime/模型/Host 前置条件为准。

## 当前协议基准与限制

- 固定 Codex 0.160.1 / `d27764b82f7118f674371e6d6e76271d9d606edb`，通过 JSONL stdio 初始化和收发；不依赖 UI 专有源码。
- 官方 [app-server 文档](https://learn.chatgpt.com/docs/app-server) 标记该接口及 WebSocket 传输为实验性。固定版本、回归与升级评估是发布前置条件，不将其稳定性视为已获保证。
- `initialize` 成功后发送 `initialized`；请求/响应通过 ID 关联；带 ID 的服务端方法是请求，需要明确回应，不是普通通知。
- 上游 wire 不要求 `jsonrpc` 头。未知方法/扩展字段保留原样；后续应用协议在独立 facade 中映射。
- 当前 doctor 是离线元数据检查，不验证模型推理、工具执行、真实审批、存储恢复或生产 Host。
- request 超时/取消或事件溢出时连接进入不可复用状态；调用方需通过 Host 状态恢复确认结果，不自动重试动作。当前生产恢复流程待 H 阶段实现。

## 构建环境分工

- 当前环境先完成 Linux Rust、CLI、共享核心、可执行的前端与协议检查。
- Windows 原生应用、MSVC、安装器及本机缺失依赖的检查使用 GitHub Windows runner；真实 Windows 11 桌面/UAC/sandbox 差异单独验收。
- iOS 的测试与最终构建均在 GitHub macOS runner；无签名 app/archive/XCFramework/xcresult 不是可直接安装的 IPA。
- iOS 键盘/手势/后台连接与通知等真机行为，在签名和设备条件具备后验证；不以模拟器或静态图代替。

## 开发纪律

- 每次先读 `HANDOFF.md` 与 Git 状态；阶段完成/暂停/中断前更新恢复点。
- 只创建当前阶段用到的模块，保留整体路径方向；不提前填满空抽象或假功能。
- 对接口和运行风险做有意义测试，按真实结果记录。原始 V2 的 CLI/Provider/Remote/工具能力需求仍需逐项验收。
- 实际付费 API 调用与凭据复用/创建需先明确授权；在此之前使用离线元数据与协议测试服务，不读取用户密钥。
