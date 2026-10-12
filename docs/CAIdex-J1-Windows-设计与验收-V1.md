# J-1 Windows 应用壳与本地 Chat：设计与验收 V1

2026-10-12 用户确认 I-7 独立审计通过，并正式选择 J-1。授权包括应用壳、启动/导航/异常处理、本地 Chat 会话及历史、模型/API Key、深浅主题/中英文、真实可安装下载产物及 ChatGPT 参考视觉；本机优先，环境不足使用 GitHub。本轮不接原生账户、设备管理、云端 Memory/同步，不改变生产及已验证邮件/账户安全流程。

## 目标、依赖与停止条件

目标是可实际安装和运行的 Windows 11 x64 本地 Chat 应用，复用已验 Rust 能力，而非纯前端演示或网页产品。实施单位以[批准范围](CAIdex-J-Windows-实施范围-V1.md)为准，不把 J-1 完成视为整个 J 或项目通过。

既有依赖：`chat/core` 的本地 SQLite/消息 DAG/流式终态、Provider/Registry/ModelRouter、Broker/Windows SystemStore、品牌资产、[V3](CAIdex-实施计划-V3.md)、[UI 规范](CAIdex-UI-规范-V1.md)。固定 Runtime 0.160.1 不变，本轮普通 Chat 不访问 Host 或执行工具。将来的账户与记忆仍使用现有共享核心契约，不另造认证/同步实现或提前添加无用接口。

通过条件：精确源码的前端构建与交互测试、Windows Rust 格式/Clippy/后端测试、真实应用构建、原生 WebView/IPC 合成 Chat 与异常恢复、安装包取得/安装/启动验证，以及共享核心相关回归通过；交付不可变 SHA、实际 CI、原始证据、产物 hash 和可复现试用步骤。Windows runner 证据与 Windows 11 人工/真实模型证据分开。

停止条件：达到本轮条件后封存等待独立审计；不得自行宣布独立审计、完整 J 或整个项目验收通过，不推进账户/Memory/同步 UI 或 K 及后续。

## 实现边界

| 入口 | 实际实现 | 权限/数据边界 |
| --- | --- | --- |
| 启动/应用壳 | Tauri 2.12.3 + React 19.3.0/TypeScript；窄侧栏、顶部上下文、独立滚动对话与 Composer、设置弹窗、启动失败/界面错误恢复 | Windows 原生工程，不提供 Web 客户端；本轮 Codex 只显示未接入状态，无执行入口 |
| 本地 Chat | 创建会话、完整分页历史/搜索、增量 SSE、停止、输入草稿、消息 DAG/选择分支、附件入口、复制、选定会话 JSON 导出 | 复用 Chat/Core；本机 guest 隔离区；不创建账户、不上传云；未知或中断 POST 不自动重发 |
| 模型/Key | 每个配置分别指定提供商、native model ID、API 地址；添加/修改、选择、Key 保存/替换/移除 | 复用 Broker/SystemStore；无 Key 读出 IPC；界面只返回配置状态；更换带 Key 的地址需要显式确认或新 Key |
| Provider | OpenAI、Anthropic、Google、DeepSeek、Qwen、OpenRouter、Ollama、Custom Responses、Custom Chat Completions | 复用九个既有 Adapter 与 ModelRouter，不推测 Live/Full；模型 ID 手动配置，不以名称推断能力；模型列表不是商业调用验证 |
| 外观/语言 | 系统/浅色/深色、默认简中/可选英文、设置和配置本机持久化 | 不存浏览器 localStorage 或云；IME 组合输入不提交，Enter 提交/Shift+Enter 换行 |
| 输入与异常 | 活动生成不允许换模型/改 Key；单活动本地 turn；错误不回显原平台/服务载荷；跨会话查阅保留生成归属 | 本机数据、Key 和网络状态不冒充云同步/Host 权限；生产服务无请求或变更 |

本地根为 Windows `AppLocalData/org.caidex.desktop`（实际路径由 Tauri 解析）；`desktop.sqlite3` 只保存非秘密模型配置、外观/语言和输入草稿，`chat/guest` 复用 Chat 私有目录与独占锁。凭据保存到 Windows Credential Manager 服务 `org.caidex.credentials.v1`，owner=`chat-guest`、每个模型 profile 为独立 UUID；不进入两个 SQLite 库、日志或导出。本机数据明文，由 OS 用户目录权限保护，不能防同一 OS 用户或管理员读取。

共享 Chat 仅增加 `draft(operation)` 只读接口供 UI 观察，以及持久化前拒绝当前 Broker 已登记的凭据输出；不新增 HTTP/认证/邮件接口，不改云 schema/生产迁移或 Runtime。应用 IPC 13项逐命令 allow、仅开放原生菜单事件的listen/unlisten，仅本地 main 窗口；CSP 阻止任意页面网络、脚本/远程 frame，模型网络在 Rust Adapter 内完成，HTTPS/回环 HTTP、无嵌入凭据/查询参数、无隐式环境 Key。Markdown 不执行原始 HTML，不加载模型给出的远程图片；外链只显示文本与地址，不在 WebView 导航。

## 视觉参考与来源清单

参考核对日期 2026-10-12，用户随后提供四张桌面截图：照片1/4作为Windows Chat左上模式下拉、Chat/Work顶部位置、暖灰侧栏/圆角主面板/居中欢迎语与输入区的直接基准；照片2补Codex壳，照片3仅参考下拉和会话层级，不新增macOS客户端。客户端版本未知，不声称逐像素完美复刻。截图中的图片/定时任务/插件/Work/审批等不扩为J-1功能；Work显示不可用，Codex仍不执行。原图仅作为用户提供的参考，不把水印、账户姓名、桌面壁纸或官方品牌复制进CAIdex。实际截图在本轮 CI 中生成并逐图检查；截图证明对应合成状态的实现，不能替代其他页面或真实模型兼容性。

| 页面/状态 | 参考来源 | 实现性质 |
| --- | --- | --- |
| 窄侧栏/历史/顶部模型与会话 Composer | [ChatGPT Windows 官方资料](https://learn.chatgpt.com/docs/windows/windows-app)及既定 UI 规范 | 用户照片1/4直接布局参考，CAIdex 补齐本地模型/Key配置提示；模型选择放Composer右侧 |
| 浅深主题、圆角设置弹窗/分类导航、输入/错误状态 | 同上及既定 UI 状态矩阵 | 沿用截图视觉语法；原生文件/编辑/视图/帮助菜单与窗口主题随语言/外观变化；灰度/字体/DPI差别按实际Windows截图记录 |
| 品牌符号与 Windows 图标 | `assets/brand/CAIDEX_symbol_color_transparent_1024.png`、`CAIDEX_app_icon_windows.ico` | 原文件复制，没有生成或重画 Logo |
| Key 与模型、账户/Host 分离 | UI 规范的“配置归属必须可见” | CAIdex 既定授权模型；本轮仅此设备 Chat |

## 构建与验证

当前仍在开发验证，以下为验证入口，最终精确通过结果封存后补入本节；未运行或失败的检查不算通过。

- 本机：源/接口/依赖检查、Rust fmt、Python 语法、文档链接和 diff。启动盘余227MB；实际 npm 安装触发 ENOSPC，只删除本轮未完成 `apps/windows/node_modules`，原有效缓存不清。完整本机前端/Rust/Windows 构建未完成。
- `.github/workflows/windows.yml` 前端 job：锁定 npm 安装、TypeScript/Vite 构建、Playwright IPC 模拟交互及截图；模拟 IPC 不计为原生 Windows 验证。
- Windows job：锁定工具链/Rust依赖、严格 Clippy、后端测试（含实际 Windows SystemStore 合成 Key）、Tauri release/NSIS/MSI，原生 WebDriver/真实共享核心/合成 HTTP 流、停止/重启/强杀 unknown 不重发、NSIS 安装/启动/卸载及产物 hash。
- 既有 Foundation：共享 Chat 改动相关的三平台/数据库/认证/邮件合成回归；不连生产、不发真实邮件、不读取商业 Key。

[Tauri 能力配置](https://v2.tauri.app/security/capabilities/)与[原生 WebDriver](https://v2.tauri.app/develop/tests/webdriver/)为 IPC 权限和 Windows 验证的官方依据；安装包使用[Tauri Windows installer](https://v2.tauri.app/distribute/windows-installer/)既有 NSIS/MSI 路径。

## 取得与五步试用

精确下载链接待最终成功 CI 后填入；不得以构建计划代替实际可下载产物。

1. **取得**：从精确 Windows CI 的 `CAIdex-Windows-x64-<完整SHA>` artifact 下载，按 `build-manifest.json` 核对 NSIS/MSI 的 SHA256。Actions 产物保留30天，下载通常需要 GitHub 登录；不是已发布/签名正式版。
2. **安装/启动**：Windows 11 x64 运行 NSIS `.exe`，按安装向导启动 CAIdex。Windows WebView2 若缺失，安装器使用官方 bootstrapper，需要联网。未签名安装包可能触发 SmartScreen；按组织策略处理，不关闭系统保护。MSI 为备用包，其具体安装证据以最终结果为准。
3. **配置**：设置→模型与提供商→添加模型；选择协议/提供商、填真实可用 model ID 与正确 API 地址，保存此设备 Key（Ollama可无Key）。从顶部模型菜单选择配置。Key 默认遮蔽，保存后不回显。
4. **操作**：新建 Chat，输入/换行/发送，观察输出；生成中停止；查阅/搜索历史，保留未发送草稿；设置中切换主题/语言，退出并重新启动。附件功能受模型能力约束，不以本轮合成文本测试授真实视觉/文件通过。
5. **通过判定**：新消息与实际模型输出出现、停止后不自动重发、重启后已保存历史/草稿/外观恢复、Key只显示配置状态；网络/认证/格式错误必须明确显示未完成。实际商业模型由用户后续自行验证，测试夹具响应不当真实模型输出。

## 限制与后续归属

- 原生账户、设备管理、自动 Memory/整合/Embedding 设置、云同步/A/B删除不在 J-1；本轮无伪开关或模拟账户。后续 J 内部里程碑须另行批准。
- Codex 执行/审批/Queue/Steer/Diff、Host/SSH/Relay、iOS和正式CLI整合属于后续阶段；本轮不授这些功能通过。
- Windows runner不是 Windows 11 人工交互、UAC/SmartScreen/多显示器/真实IME硬件/安装升级验收；没有签名证书或长期分发发布。
- 模型手动配置；高级推理/附件/能力/真实目录及各模型版本兼容性仍按 F/G-Live 保留未验，不把九Adapter构造测试当九商业模型通过。
- 草稿自动保存有400ms节流，强杀可能丢失尚未落库的末尾输入；未发送附件的UI选择在重启后不自动恢复，原BLOB可随选定会话导出。模型输出的中断/未知终态不会盲目重放。
- API Key的临时输入会经本地 Tauri IPC 交给 Broker；已保存 Key 无读出接口。系统凭据与本机SQLite不防同一OS用户/管理员，非云端或历史 E2EE。
- 导出、复制、模型附件兼容性及 MSI 安装是否实测，按最终结果单独列出，不用仅有入口声称通过。

完成开发验证后提交本文件、精确证据与下载入口，进入独立只读审计。实施会话的 diff 检查和测试均不代替独立审计。
