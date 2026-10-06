  
## CAIdex 最终完整方案与执行计划 V2  
## 1. 产品定义  
CAIdex 是一个面向个人及朋友使用的多模型 AI 客户端，核心基于 OpenAI Codex 开源 Runtime，并在此基础上扩展：  
* ChatGPT 风格聊天体验  
* Codex 原生 Work / Coding Agent 能力  
* 第三方大模型自由接入  
* 用户自行配置 API Key  
* Windows 本地 Codex  
* Linux/VPS/服务器远程 Codex  
* iOS 远程接管 Codex  
* CLI  
* Plugins / MCP / Skills  
* 多 Host  
* Remote Thread  
* 审批  
* requestUserInput  
* Queue / Steer / Resume  
* Diff / Code Review  
* 后续 Remote Relay  
CAIdex 当前不作为商业产品设计。  
产品名称固定：  
**CAIdex**  
   
⸻  
   
## 2. 最终产品原则  
CAIdex 从第一天遵循：  
**允许功能暂时未完成，不允许架构是临时的。**  
也就是：  
```
可以：
某模块接口已经完成，但具体能力暂未实现。

不可以：
先写一个简易实现，以后确定要推翻重做。
```
整体开发原则：  
```
Final Architecture First
+
Incremental Implementation
```
而不是：  
```
MVP Architecture
↓
第二套 Architecture
↓
最终 Architecture
```
   
⸻  
   
## 3. 当前明确支持的平台  
只开发：  
```
Windows Desktop
CLI
iOS
```
当前明确不开发：  
```
Android
Web Client
macOS 原生客户端
```
但底层架构不能故意把以后扩展其他客户端的可能性堵死。  
   
⸻  
   
## 4. 语言策略  
## Windows  
默认：  
```
简体中文
```
备选：  
```
English
```
用户可手动切换。  
   
⸻  
   
## iOS  
默认：  
```
简体中文
```
备选：  
```
English
```
   
⸻  
   
## CLI  
只使用：  
```
English
```
不进行中文本地化。  
原因：  
* 保持 Codex CLI 原始开发者体验  
* 避免技术术语中英文混杂  
* 更方便与上游 Codex 文档、报错和 issue 对照  
* 降低 CLI upstream merge 成本  
CLI 命令保持：  
```
caidex
caidex exec
caidex remote
caidex models
caidex provider
caidex mcp
caidex login
```
   
⸻  
   
## 5. UI 总原则  
CAIdex Windows 和 iOS 客户端以当前 ChatGPT / Codex 官方客户端作为主要 UX 参考。  
目标不是重新发明 UI，而是：  
用户从 ChatGPT/Codex 切到 CAIdex 后几乎不需要重新学习。  
可以高度还原：  
* 布局  
* 交互流程  
* Sidebar  
* Composer  
* Model Picker  
* Thread  
* Tool Card  
* Approval  
* Codex Working 状态  
* Diff  
* Code Review  
* Mobile Sheet  
* Remote Host  
* Codex Mobile  
* Queue / Steer  
* Settings  
* 深色/浅色模式  
* 动画节奏  
* 键盘交互  
* iOS 原生手势  
但不得直接复制或分发：  
* OpenAI Logo  
* ChatGPT Logo  
* Codex Logo  
* 官方商标  
* 官方专有字体文件  
* 官方插画  
* 官方图片资源  
* 受保护品牌资产  
CAIdex 自己实现全部 UI。  
   
⸻  
   
## 6. CAIdex 的三个产品面  
整个产品分成：  
```
Chat
Codex
Remote Codex
```
   
⸻  
   
## 7. Chat  
Chat 是普通 AI 对话模式。  
架构：  
```
User
 ↓
CAIdex Chat Runtime
 ↓
Model Router
 ↓
Provider
 ↓
Model
```
主要能力：  
* 普通聊天  
* 代码问答  
* 写作  
* 翻译  
* 文件分析  
* 图片  
* 搜索  
* 多模态  
* 第三方模型  
默认不拥有：  
```
Shell
Git
项目写权限
apply_patch
本地执行环境
```
   
⸻  
   
## 8. Codex  
Codex 模式使用真正的 Codex Runtime。  
不是：  
```
CAIdex 自己模拟一个 Agent
```
而是：  
```
CAIdex UI
 ↓
Codex app-server
 ↓
Codex Runtime
```
Codex Runtime 是 Work 行为的唯一真源。  
   
⸻  
   
## 9. Codex 原生能力必须完整保留  
必须尽量保留上游所有能力，包括：  
```
Agent lifecycle
Shell
Unified exec
PTY
Long-running processes
apply_patch
Filesystem
Git
Sandbox
Approval
Network approval
MCP
Plugins / Apps
Skills
Tool auto-selection
requestUserInput
MCP elicitation
Context compaction
Interrupt
Resume
Queue
Steer
Diff
Plan
Goal
Sub-agent
Tool result handling
Usage tracking
Session state
Thread persistence
```
原则：  
CAIdex 不能为了跨模型、跨平台或 UI 方便而阉割 Codex。  
   
⸻  
   
## 10. Codex Runtime 的源码策略  
尽量保持以下部分接近上游：  
```
codex-rs/core
codex-rs/protocol
codex-rs/app-server
codex-rs/app-server-protocol
codex-rs/exec-server
codex-rs/sandboxing
codex-rs/state
codex-rs/tools
codex-rs/mcp
codex-rs/skills
```
CAIdex 自己的扩展尽量放在外部层。  
   
⸻  
   
## 11. 上游隔离原则  
禁止：  
```
直接 fork 后到处修改 codex-core
```
推荐：  
```
OpenAI Codex upstream
        │
        ▼
Tracked runtime
        │
        ├── Minimal patch layer
        │
        └── CAIdex Extension Boundary
```
CAIdex 独有功能：  
```
Multi-model
Provider
Credentials
Remote
Windows UI
iOS UI
Sync
CAIdex Settings
```
尽量全部存在 Codex Core 外部。  
   
⸻  
   
## 12. Monorepo 最终结构  
建议从第一天建立：  
```
caidex/
│
├── upstream/
│   └── codex/
│
├── runtime/
│   ├── bridge/
│   ├── protocol/
│   ├── adapters/
│   └── upstream-patches/
│
├── model/
│   ├── gateway/
│   ├── canonical/
│   ├── registry/
│   ├── capabilities/
│   ├── prompts/
│   ├── normalizer/
│   └── providers/
│       ├── openai/
│       ├── anthropic/
│       ├── google/
│       ├── deepseek/
│       ├── qwen/
│       ├── openrouter/
│       ├── ollama/
│       └── custom/
│
├── credentials/
│   ├── core/
│   ├── windows/
│   ├── ios/
│   └── linux/
│
├── remote/
│   ├── protocol/
│   ├── session/
│   ├── host/
│   ├── transport/
│   │   ├── ssh/
│   │   └── relay/
│   ├── pairing/
│   └── security/
│
├── persistence/
│   ├── schema/
│   ├── migrations/
│   └── repositories/
│
├── apps/
│   ├── desktop/
│   ├── ios/
│   └── cli/
│
├── design/
│   ├── tokens/
│   ├── interaction-spec/
│   └── assets/
│
├── localization/
│   ├── zh-CN/
│   └── en-US/
│
└── tests/
    ├── codex-parity/
    ├── provider/
    ├── remote/
    ├── security/
    └── ux-parity/
```
这就是最终目录，不以后大拆。  
   
⸻  
   
## 13. Model Architecture  
这是 CAIdex 与官方 Codex 最大的核心差异之一。  
CAIdex 不直接让 Codex Core 知道：  
```
Claude
Gemini
Qwen
DeepSeek
Ollama
```
而是增加：  
## CAIdex Compatibility Gateway  
架构：  
```
                   Codex Runtime
                         │
                  Canonical Protocol
                         │
                         ▼
          CAIdex Compatibility Gateway
                         │
              Provider Abstraction
                         │
      ┌──────────┬───────┼─────────┬─────────┐
      │          │       │         │         │
   OpenAI    Anthropic  Gemini   Qwen    DeepSeek
                                             │
                                    Ollama / Custom
```
   
⸻  
   
## 14. Gateway 不是普通 API Proxy  
Gateway 必须负责：  
```
Request normalization
Response normalization
Streaming
Tool calls
Tool results
Reasoning
Usage
Errors
Images
Structured output
Context
Model metadata
Capabilities
Prompt compatibility
```
对 Codex 来说：  
```
所有模型都是统一模型。
```
   
⸻  
   
## 15. Canonical Model Protocol  
内部建立统一模型协议。  
例如：  
```
CanonicalRequest

CanonicalResponse

CanonicalTool

CanonicalToolCall

CanonicalToolResult

CanonicalUsage

CanonicalReasoning

CanonicalStreamEvent
```
Provider Adapter 只负责：  
```
Canonical
↕
Native Provider Protocol
```
   
⸻  
   
## 16. Provider Interface  
从第一天定义最终接口。  
概念结构：  
```
trait ModelProvider {
    async fn list_models(...);

    async fn create_response(...);

    async fn stream_response(...);

    fn capabilities(...);

    fn metadata(...);

    fn credential_requirements(...);
}
```
以后任何新 Provider 都只实现 Adapter。  
Codex Runtime 不改。  
   
⸻  
   
## 17. 首批 Provider  
最终 Provider Registry 从第一天预留：  
```
OpenAI
Anthropic
Google Gemini
DeepSeek
Qwen
OpenRouter
Ollama
Custom OpenAI-compatible
Custom Responses-compatible
```
实现顺序可以分批。  
但接口从第一天固定。  
   
⸻  
   
## 18. Model Capability Registry  
每个模型必须描述能力。  
例如：  
```
Text
Vision
Reasoning
Native tools
Parallel tools
Structured output
Streaming
Web search
Image generation
Context size
Output limit
Prompt profile
Codex compatibility level
```
   
⸻  
   
## 19. Compatibility Level  
CAIdex UI 显示：  
```
Full
Compatible
Limited
Experimental
```
例如：  
```
Claude XXX
Codex compatibility: Full

✓ Shell
✓ Patch
✓ MCP
✓ Skills
✓ Plugins
✓ Vision
✓ Tool calling
```
某本地模型：  
```
Qwen Local
Codex compatibility: Limited

✓ Shell
✓ Patch
⚠ MCP
⚠ Parallel tools
✗ Vision
```
这样换模型时用户知道可能影响 Agent 表现。  
   
⸻  
   
## 20. Tool Normalization  
所有模型最终必须产生统一：  
```
ToolCall {
    id
    name
    arguments
}
```
Codex Runtime 仍然负责：  
```
Tool Router
 ↓
Approval
 ↓
Sandbox
 ↓
Execution
```
模型只决定：  
```
想调用哪个工具。
```
模型绝不能绕过 Codex Runtime 自己执行。  
   
⸻  
   
## 21. Plugin / MCP / Skills  
必须保持 Codex 原有行为：  
```
用户提出任务
↓
Codex 发现可用工具
↓
模型自动选择
↓
如果允许
    自动调用
↓
如果需要审批
    发 Approval Request
↓
客户端显示结构化 UI
↓
继续执行
```
不要变成：  
```
用户每次手动选择插件。
```
自动工具选择必须保留。  
   
⸻  
   
## 22. Prompt Compatibility Layer  
不同模型使用不同 compatibility profile。  
最终：  
```
Codex Base Instructions
+
Provider Compatibility Instructions
+
Model-specific adjustments
```
例如：  
```
Claude profile
Gemini profile
Qwen profile
Local model profile
```
但不能破坏 Codex 核心行为。  
   
⸻  
   
## 23. Provider 设置  
普通用户界面：  
```
设置
└── 模型与提供商
    ├── OpenAI
    ├── Anthropic
    ├── Google Gemini
    ├── DeepSeek
    ├── Qwen
    ├── OpenRouter
    ├── Ollama
    └── 自定义提供商
```
用户日常只需要：  
```
API Key
```
   
⸻  
   
## 24. Provider 公共配置  
内置：  
```
Base URL
Protocol
Authentication format
Known models
Capability metadata
Headers
Default timeouts
Prompt profile
```
这样用户不用填写。  
   
⸻  
   
## 25. Advanced Provider Configuration  
不能把 URL 永久锁死。  
高级设置允许：  
```
Base URL override
Protocol
Custom headers
Query parameters
Proxy
Timeout
Compatibility mode
```
适合：  
```
Azure
LiteLLM
OpenRouter
反向代理
企业 Gateway
本地 Gateway
朋友服务器
```
   
⸻  
   
## 26. API Key 原则  
任何官方或第三方 API Key：  
永远不写死在 CAIdex 客户端源代码中。  
每个用户在客户端设置自己的 Key。  
   
⸻  
   
## 27. Credential Broker  
所有 secret 必须经过：  
```
Credential Broker
```
而不是：  
```
UI → API Key string → 随便传
```
正确结构：  
```
Provider Adapter
        │
        ▼
Credential Broker
        │
   System Secret Store
```
   
⸻  
   
## 28. Windows Secret Storage  
优先使用：  
```
Windows Credential Manager
DPAPI
```
   
⸻  
   
## 29. iOS Secret Storage  
使用：  
```
Keychain
```
   
⸻  
   
## 30. CLI / Linux Secret Storage  
支持：  
```
Environment variables
Secret Service / keyring
Protected credential file
```
Credential file 必须：  
```
0600
```
并禁止进入 Git。  
   
⸻  
   
## 31. Secret Redaction  
统一过滤：  
```
Authorization
api_key
apikey
x-api-key
access_token
refresh_token
client_secret
cookie
```
禁止 secrets 出现在：  
```
Debug log
Crash log
Tool output
HTTP tracing
Analytics
Cloud sync
```
   
⸻  
   
## 32. Credentials 与 Host  
Codex Remote 环境中，API Key 与执行 Host 绑定。  
例如：  
```
VPS

OpenAI      configured
Anthropic   configured
Gemini      configured
```
iPhone 控制 VPS 时：  
```
iPhone
 ↓
Model selection
 ↓
VPS Credential Broker
 ↓
Provider
```
API Key 不需要发送给手机。  
   
⸻  
   
## 33. Remote Architecture  
Remote 从第一天设计成最终结构。  
不能先做：  
```
SSH terminal text
```
以后再换。  
从第一天就是：  
```
CAIdex Client
     │
RemoteSession
     │
Transport
 ┌───┴────┐
 SSH    Relay
  │        │
  └───┬────┘
      │
 app-server
      │
Codex Runtime
```
   
⸻  
   
## 34. RemoteSession  
上层 UI 永远只认识：  
```
RemoteSession
```
不关心底层是：  
```
SSH
Relay
Local IPC
```
这样以后增加 Relay 不改业务逻辑。  
   
⸻  
   
## 35. SSH Transport  
优先服务：  
```
Linux VPS
Linux Server
NAS
Development server
```
支持：  
```
SSH config
SSH key
ProxyJump
Tailscale hostname
Custom port
```
SSH 只负责 Transport。  
Codex 数据必须走：  
```
Structured app-server protocol
```
不能依赖 terminal 文本解析。  
   
⸻  
   
## 36. Relay Transport  
最终架构从第一天预留。  
适合：  
```
Windows PC
家庭机器
未来无需 SSH 的 Remote
```
结构：  
```
Host
 ↓ outbound connection
Relay
 ↑ outbound connection
Phone
```
不要求：  
```
公网 IP
端口映射
开放防火墙
```
   
⸻  
   
## 37. Pairing  
最终支持：  
```
QR Code
Manual short code
Trusted Device
Revoke Device
Pairing expiration
```
   
⸻  
   
## 38. Remote Security  
推荐最终设计：  
```
Device identity
Host identity
Pairing trust
Session authentication
Optional E2EE
```
长期可考虑：  
```
X25519
+
ChaCha20-Poly1305 / AES-GCM
```
让 Relay 无法读取实际内容。  
   
⸻  
   
## 39. Thread Model  
Thread 是 CAIdex 的核心对象。  
无论：  
```
Local
Remote
Windows
iOS
CLI
```
都操作同一个 Thread 语义。  
Thread 不绑定某个 UI。  
   
⸻  
   
## 40. Thread 必须持久化  
Thread 独立于连接存在。  
也就是：  
```
WebSocket disconnected
≠
Thread terminated
```
Agent 可以继续工作。  
   
⸻  
   
## 41. Remote Reconnect  
所有 event 有：  
```
sequence_id
```
手机断线：  
```
event #510
```
重新连接时：  
```
resume from #511
```
补齐：  
```
#511
#512
#513
...
```
不能依赖当前 socket 保存状态。  
   
⸻  
   
## 42. Canonical CAIdex Event Protocol  
第一天就设计完整事件空间。  
例如：  
```
ThreadStarted
ThreadUpdated

TurnStarted
TurnCompleted
TurnInterrupted
TurnFailed

UserMessage

AssistantTextDelta
ReasoningDelta

ToolStarted
ToolOutputDelta
ToolCompleted

CommandStarted
CommandOutputDelta
CommandCompleted

FileRead
FileChanged
DiffUpdated

ApprovalRequested
ApprovalResolved

UserInputRequested
UserInputResolved

McpStarted
McpElicitationRequested
McpCompleted

PluginStarted
PluginApprovalRequested
PluginCompleted

AgentStarted
AgentCompleted

PlanUpdated
GoalUpdated

UsageUpdated

ConnectionChanged
HostChanged
```
所有客户端消费同一种事件。  
   
⸻  
   
## 43. Structured Requests  
必须把：  
```
Approval
requestUserInput
MCP elicitation
Plugin request
```
作为正式结构化消息。  
绝不能降级成普通聊天文本。  
   
⸻  
   
## 44. requestUserInput  
Codex Runtime：  
```
UserInputRequested
```
Windows：  
```
Modal / Dialog
```
iOS：  
```
Sheet
```
CLI：  
```
TUI selection
```
响应回 Runtime：  
```
UserInputResolved
```
这是正式协议。  
   
⸻  
   
## 45. Approval  
支持：  
```
Allow once
Allow for session
Always allow when supported
Deny
```
Network：  
```
Allow host once
Allow host for session
Deny
```
Plugin：  
```
Allow
Deny
```
实际安全判断留在 Host。  
   
⸻  
   
## 46. Queue  
Agent 正在执行时，新 prompt 可以：  
```
Queue
```
等当前 Turn 完成后执行。  
   
⸻  
   
## 47. Steer  
可以：  
```
Steer
```
修改当前正在执行的工作方向。  
Windows 和 iOS 都必须支持。  
   
⸻  
   
## 48. Interrupt / Resume  
任何客户端都可以：  
```
Stop
Resume
```
但实际 Runtime 状态在 Host。  
   
⸻  
   
## 49. Mobile Codex  
iOS 不是缩水版 Codex。  
定位：  
Codex Remote Control Plane。  
真正执行环境在：  
```
Windows
VPS
Linux Server
```
   
⸻  
   
## 50. iOS 功能目标  
iOS 最终必须支持：  
```
Chat
Codex
Host list
Project list
Repo selection
Branch selection
Worktree selection
Thread list
New thread
Resume thread
Model picker
Streaming
Thinking
Tool events
Command output
Diff
Approval
requestUserInput
MCP elicitation
Plugin approval
Queue
Steer
Interrupt
Resume
Side Chat
Code Review
Push notification
Reconnect
Background continuation
```
   
⸻  
   
## 51. iOS 技术方案  
使用：  
```
SwiftUI
```
不要 WebView 作为主 UI。  
使用原生：  
```
NavigationStack
Sheet
Context Menu
Swipe
Haptics
Native keyboard
File picker
Photo picker
Push notifications
```
   
⸻  
   
## 52. Windows 技术方案  
推荐：  
```
Tauri 2
React
TypeScript
Rust
```
结构：  
```
React
 ↓
Tauri
 ↓
Rust Bridge
 ↓
CAIdex runtime
 ↓
Codex app-server
```
Windows 同时可以：  
```
做客户端
+
做本地 Codex Host
+
被 iPhone Remote
```
   
⸻  
   
## 53. CLI 技术方案  
CLI 尽量直接继承 Codex TUI / CLI。  
品牌和命令改为 CAIdex。  
尽量少改变：  
```
Interaction
Keybindings
Approval flow
Tool output
TUI behavior
```
语言只保留英文。  
   
⸻  
   
## 54. Windows UI Architecture  
从第一天建立正式组件：  
```
AppShell
Sidebar
SidebarHistory
SidebarCodex
SidebarProjects

ConversationView
MessageList

UserMessage
AssistantMessage

Composer
AttachmentButton
ModelPicker
ModePicker

ThinkingBlock
ToolCard
CommandCard
PluginCard
McpCard
DiffCard

ApprovalDialog
UserInputDialog
ElicitationDialog

CodexThreadHeader
HostPicker
ProjectPicker
BranchPicker
WorktreePicker

WorkingStatus
QueueIndicator
SteerMode

Settings
ProviderSettings
RemoteSettings
PluginSettings
McpSettings
```
   
⸻  
   
## 55. iOS Component Architecture  
对应：  
```
RootTab / Navigation
Sidebar
ChatScreen
CodexScreen

ConversationView
Composer

ModelSheet
HostSheet
ProjectSheet

ToolEventView
CommandOutputView
DiffView

ApprovalSheet
UserInputSheet
McpSheet

CodexStatusView
QueueView
SteerView

Settings
ProviderSettings
RemoteHosts
```
   
⸻  
   
## 56. Design System  
在写大量 UI 前建立：  
```
Design Tokens
```
包括：  
```
Spacing
Typography
Radius
Border
Semantic colors
Light mode
Dark mode
Sidebar width
Composer size
Modal sizing
Animation
Hover
Pressed state
Mobile safe area
```
Windows 和 iOS 使用同一套语义 token。  
但分别采用平台原生实现。  
   
⸻  
   
## 57. i18n  
Windows/iOS 使用统一 semantic keys。  
例如：  
```
common.cancel
common.continue

chat.new

codex.stop
codex.resume
codex.queue
codex.steer

approval.allow_once
approval.allow_session
approval.deny

remote.hosts
remote.offline

provider.api_key
provider.connected
```
中文：  
```
zh-CN
```
英文：  
```
en-US
```
CLI 不接入这套 UI i18n。  
   
⸻  
   
## 58. Database  
从第一天按最终状态建 Schema。  
主要实体：  
```
Device
Host
Project
Repository
Thread
Turn
Event

Provider
Model
CredentialProfile

Plugin
McpServer
Skill

Approval
RemoteSession
Pairing

UserPreference
```
   
⸻  
   
## 59. Thread 数据模型至少预留  
```
thread_id
host_id
project_id
repository_id
provider_id
model_id
mode
status
created_at
updated_at
last_event_seq
```
避免以后 Remote 时大迁移。  
   
⸻  
   
## 60. Model Profile  
数据库 / Registry 区分：  
```
Provider
Model
CapabilityProfile
PromptProfile
CompatibilityProfile
```
不要全部塞在 Model 一张表里。  
   
⸻  
   
## 61. Provider Credential Profile  
支持未来：  
```
Anthropic
├── Personal
└── Alternate

OpenAI
├── Main
└── Testing
```
底层从第一天支持多 profile。  
UI 第一版可以只展示一个。  
   
⸻  
   
## 62. Plugins / MCP  
不重新实现插件体系。  
优先复用 Codex：  
```
MCP
Apps
Skills
Tool discovery
Approvals
```
CAIdex 主要负责 UI 和配置入口。  
   
⸻  
   
## 63. Settings 最终结构  
Windows/iOS：  
```
设置

常规
外观
语言

模型与提供商

Codex
插件
MCP
Skills

远程主机
设备

安全

高级
关于 CAIdex
开源许可证
```
   
⸻  
   
## 64. 模型设置体验  
用户进入：  
```
模型与提供商
```
显示：  
```
OpenAI
已连接

Anthropic
添加 API Key

Google Gemini
添加 API Key

DeepSeek
添加 API Key

Qwen
添加 API Key

Ollama
本地

OpenRouter
添加 API Key

自定义提供商
+
```
普通用户只填 Key。  
   
⸻  
   
## 65. Connection Test  
每个 Provider 支持：  
```
Test connection
```
验证：  
```
Authentication
Endpoint
Model listing
Streaming
Basic request
```
必要时进一步运行 Tool compatibility probe。  
   
⸻  
   
## 66. Codex Compatibility Test Suite  
每个模型都必须跑：  
```
Basic response
Streaming
Tool call
Shell
apply_patch
Tool result
Multiple tools
Parallel tools
MCP
Plugin
Approval
requestUserInput
Error retry
Long context
Compaction
Image
Sub-agent
```
形成 Compatibility Score。  
   
⸻  
   
## 67. Codex Parity Test Suite  
任何 upstream Codex 更新后，必须跑：  
```
Shell
PTY
Patch
Git
MCP
Plugins
Skills
Approvals
Network
requestUserInput
MCP elicitation
Interrupt
Resume
Queue
Steer
Diff
Context
Sub-agent
```
防止 CAIdex fork 丢功能。  
   
⸻  
   
## 68. Remote Test Suite  
必须测试：  
```
SSH connect
Disconnect
Reconnect
Network switch
Phone background
Phone foreground
App relaunch
Agent continues offline
Event replay
Approval remotely
UserInput remotely
Queue
Steer
Stop
Resume
Diff
Large command output
Host switching
```
   
⸻  
   
## 69. Security Test Suite  
覆盖：  
```
API key leakage
Log leakage
Crash report leakage
Credential permission
SSH host validation
Pairing expiration
Replay attacks
Session authentication
Approval bypass
Sandbox bypass
Remote command injection
```
   
⸻  
   
## 70. UX Parity Test  
维护：  
```
Windows ChatGPT parity
iOS ChatGPT parity
Codex workflow parity
```
按版本持续更新。  
   
⸻  
   
## 71. Upstream Codex 更新策略  
建立两个 remote：  
```
origin
upstream
```
定期：  
```
fetch upstream
review changes
run parity tests
merge
apply CAIdex minimal patches
```
每次 CAIdex release 记录：  
```
Based on Codex commit:
xxxxxxxx
```
   
⸻  
   
## 72. Upstream Patch Policy  
任何修改 Codex 上游代码的 patch 必须满足：  
```
小
独立
有原因
有测试
尽可能可 upstream
```
禁止大量混杂修改。  
   
⸻  
   
## 73. 最终执行顺序  
这里不是“做 MVP 再重构”。  
而是：  
一次确定最终架构，然后分区域实现最终架构。  
   
⸻  
   
## 74. 阶段 A：Architecture Freeze  
首先完成全部技术设计文档。  
包括：  
```
System architecture
Module boundaries
Runtime bridge
Model protocol
Provider interface
Capability schema
Credential interface
Remote protocol
Event protocol
Database schema
Windows component tree
iOS component tree
Design tokens
i18n schema
Security model
Upstream strategy
```
这个阶段不应该急着大量开发 UI。  
验收：  
新增任何未来核心功能都不需要改变整体架构边界。  
   
⸻  
   
## 75. 阶段 B：Repository Foundation  
建立最终 monorepo。  
完成：  
```
Workspace
Build system
CI
Formatting
Linting
Test framework
Upstream Codex tracking
Patch layer
Shared schemas
Versioning
```
同时建立空但正式的：  
```
provider interfaces
remote transport interfaces
credential interfaces
event interfaces
```
   
⸻  
   
## 76. 阶段 C：Codex Runtime Integration  
首先确保完整 Codex 可以作为 CAIdex Runtime 工作。  
完成：  
```
app-server bridge
thread lifecycle
event forwarding
approval forwarding
user-input forwarding
tool events
diff events
interrupt/resume
```
验收：  
不依赖 CAIdex GUI，也可以验证 CAIdex Bridge 下的 Codex 与原版 Codex 行为一致。  
   
⸻  
   
## 77. 阶段 D：Canonical Event Layer  
实现统一：  
```
Codex protocol
↓
CAIdex canonical events
```
确保：  
```
Windows
iOS
Remote
```
以后全部依赖这个协议。  
不能让 UI 直接绑死 Codex 内部 Rust struct。  
   
⸻  
   
## 78. 阶段 E：Credential System  
一次完成正式 Credential Broker。  
实现：  
```
Windows credential backend
Linux/CLI backend
Provider credential profiles
Secret redaction
API
```
iOS Keychain backend 同时定义接口，可随后完成实现。  
   
⸻  
   
## 79. 阶段 F：Model Gateway  
实现最终 Compatibility Gateway。  
首先接：  
```
OpenAI
Anthropic
Gemini
OpenAI-compatible
Custom
```
但是 Provider API 已经支持未来所有模型。  
完成：  
```
canonical request
canonical streaming
tool normalization
usage
reasoning
errors
model metadata
capabilities
```
   
⸻  
   
## 80. 阶段 G：Codex Multi-model Integration  
让：  
```
Codex Runtime
```
通过 Gateway 使用不同模型。  
重点不是聊天成功，而是：  
```
Shell
Patch
MCP
Plugins
Skills
Approval
requestUserInput
```
仍然正常。  
这一步是 CAIdex 最核心的技术验证点。  
   
⸻  
   
## 81. 阶段 H：Persistence  
建立正式数据库。  
一次完成：  
```
Hosts
Projects
Threads
Events
Providers
Models
Settings
Remote sessions
```
不能第一版只做 messages 表。  
   
⸻  
   
## 82. 阶段 I：Windows App Foundation  
按照最终组件树建立 Windows。  
首先完整建立：  
```
App shell
Sidebar
Conversation
Composer
Settings
Design tokens
Localization
```
不是临时 UI。  
然后逐渐把事件 renderer 接进去。  
   
⸻  
   
## 83. 阶段 J：Windows Chat  
实现完整 Chat flow：  
```
New Chat
History
Model switching
Streaming
Images/files
Provider selection
Settings
```
使用同一个 Model Gateway。  
   
⸻  
   
## 84. 阶段 K：Windows Codex  
Windows 本机：  
```
CAIdex UI
↓
Local runtime
↓
Codex
```
逐项完成：  
```
Tool cards
Command
Patch
Diff
Approval
requestUserInput
MCP
Plugins
Skills
Queue
Steer
Stop
Resume
```
Windows 版此时达到完整 Codex 工作能力。  
   
⸻  
   
## 85. 阶段 L：Remote Core  
实现最终：  
```
RemoteSession
Transport
Host identity
Session authentication
Event replay
Reconnect
```
第一种 Transport：  
```
SSH
```
但架构已经支持：  
```
Relay
```
   
⸻  
   
## 86. 阶段 M：SSH Host  
正式支持：  
```
Linux VPS
Linux Server
```
功能：  
```
Add host
Test host
Discover project
Start remote app-server
List threads
Create thread
Resume thread
Stream events
Approval
User input
Diff
Queue
Steer
```
这一步直接覆盖你当前 VPS 使用场景。  
   
⸻  
   
## 87. 阶段 N：iOS Foundation  
基于最终架构直接创建：  
```
SwiftUI
Design tokens
Localization
Remote protocol client
Chat protocol client
Credential backend
```
不先写临时 SSH terminal。  
   
⸻  
   
## 88. 阶段 O：iOS Chat  
完成：  
```
Chat UI
History
Model selector
Streaming
Files/images
Chinese/English
```
   
⸻  
   
## 89. 阶段 P：iOS Codex Remote  
接入：  
```
Hosts
Projects
Threads
Remote events
Approval
requestUserInput
Tool states
Diff
Queue
Steer
Stop
Resume
```
iPhone 此时可以完整接管 VPS Codex。  
   
⸻  
   
## 90. 阶段 Q：Mobile Codex Parity  
继续实现：  
```
Branch selection
Worktree
Side Chat
Code review
Inline comment
Plan
Goal
Push notification
Background state
Reconnect
```
   
⸻  
   
## 91. 阶段 R：CLI CAIdex Integration  
CLI 保持 Codex 英文体验。  
增加：  
```
CAIdex providers
CAIdex model selection
Remote
Custom model support
```
但尽量保持 TUI upstream-compatible。  
   
⸻  
   
## 92. 阶段 S：Relay  
在已经正式存在的：  
```
Transport
```
下面增加：  
```
RelayTransport
```
不改 UI。  
实现：  
```
pairing
QR
device trust
outbound connection
revocation
```
   
⸻  
   
## 93. 阶段 T：E2EE  
如果启用 Relay，建议加入：  
```
End-to-end encryption
```
Relay 只负责 packet routing。  
   
⸻  
   
## 94. 阶段 U：UX 精细还原  
在完整功能基础上持续校准：  
```
Windows ChatGPT UX parity
iOS ChatGPT UX parity
Codex UX parity
```
包括：  
```
spacing
animations
sheets
context menu
keyboard
gesture
loading
working states
tool presentation
```
这里不是重构，而只是完善最终 UI。  
   
⸻  
   
## 95. 第一天禁止做的临时方案  
禁止：  
```
API Key 明文 settings.json
```
禁止：  
```
if model == Claude
```
散落在 Core。  
禁止：  
```
iOS 直接显示 SSH terminal
```
作为正式 Remote。  
禁止：  
```
Message(role, content)
```
作为唯一事件模型。  
禁止：  
```
Windows UI 直接依赖 Codex 私有 Rust struct
```
禁止：  
```
fork Codex 后随便改 core
```
禁止：  
```
先做一套 WebView iOS UI，以后再换 SwiftUI
```
禁止：  
```
先做单 Host schema，以后再迁移多 Host
```
这些都会造成你最不希望出现的返工。  
   
⸻  
   
## 96. 哪些东西可以暂时未完成  
允许：  
```
Relay interface 存在但尚未实现
Gemini adapter 尚未实现
某个 Tool renderer 尚未实现
Side Chat 尚未实现
某个高级设置尚未开放
```
前提：  
已经处在最终正确的模块边界内。  
   
⸻  
   
## 97. CAIdex 的最终系统图  
```
                         CAIdex
                            │
              ┌─────────────┴─────────────┐
              │                           │
            Chat                        Codex
              │                           │
       Chat Runtime                Codex Runtime
              │                           │
              │           ┌───────────────┼────────────────┐
              │           │               │                │
              │         Shell           MCP             Skills
              │         Files          Plugins           Agents
              │         Git             Apps             Tools
              │           │
              │        Sandbox
              │
              └──────────────┬────────────────────────────┘
                             │
                 CAIdex Model Gateway
                             │
       ┌──────────┬──────────┼─────────┬──────────┐
       │          │          │         │          │
     OpenAI    Anthropic   Gemini    Qwen      DeepSeek
                                                   │
                                            Ollama/Custom
```
客户端：  
```
                 CAIdex Protocol
                       │
        ┌──────────────┼──────────────┐
        │              │              │
     Windows          CLI            iOS
        │                              │
        │                         RemoteSession
        │                              │
        └──────────────┬───────────────┘
                       │
                  Transport
                ┌──────┴──────┐
                │             │
               SSH           Relay
                │             │
                └──────┬──────┘
                       │
               Windows / VPS
                       │
                 Codex Runtime
```
   
⸻  
   
## 98. CAIdex 的真正核心资产  
长期真正有价值的不是 ChatGPT 外观本身，而是：  
```
1. Codex Runtime Compatibility

2. Universal Model Compatibility

3. Multi-device Remote Codex

4. Secure Credential Architecture

5. ChatGPT/Codex UX Parity
```
五者组合起来才是 CAIdex。  
   
⸻  
   
## 99. 最终开发优先级  
最高优先级：  
```
P0
Architecture contracts
Codex parity
Canonical protocol
Model gateway
Credentials
RemoteSession architecture
```
随后：  
```
P1
Windows
SSH VPS
iOS
```
然后：  
```
P2
Relay
Pairing
Push
Side Chat
Handoff
Advanced review
```
但所有 P2 的接口从 Architecture 阶段已经预留。  
   
⸻  
   
## 100. CAIdex 项目最终定义  
CAIdex 最终应被定义为：  
**一个拥有 ChatGPT 式 Windows/iOS 使用体验、保留 Codex 原生 Agent Runtime 和完整 Work 能力、允许自由接入第三方大模型，并支持从手机和桌面继续控制本地电脑或远程服务器 Codex 工作环境的私人 AI 客户端。**  
具体平台：  
```
Windows
CLI
iOS
```
语言：  
```
Windows → 中文优先 / English
iOS     → 中文优先 / English
CLI     → English only
```
模型：  
```
OpenAI
Anthropic
Gemini
DeepSeek
Qwen
OpenRouter
Ollama
Custom
```
运行环境：  
```
Windows Local
Windows Remote
Linux VPS
Linux Server
```
连接：  
```
Local
SSH
Relay
```
Work：  
```
Codex Runtime 原生能力完整保留
```
UI：  
```
ChatGPT / Codex UX 高度还原
```
