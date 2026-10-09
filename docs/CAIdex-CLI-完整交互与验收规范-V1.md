# CAIdex CLI 完整交互与验收规范 V1

正式决定：2026-10-09。状态：**最终设计基准；完整 CLI 尚未实现，CLI-01～CLI-34 全部待实现、未执行**。本文不启动 CLI 编码，不改变 A–R 顺序或当前 F/G 恢复点。原 V2 仅作需求背景，冲突以 [V3](CAIdex-实施计划-V3.md)、本文及[账户/记忆/云设计](CAIdex-Account-Memory-Cloud-设计与验收-V1.md)为准。

本次续接复核沿用已归档的 V1，不重建架构。后续命令实现须同时核对本规范、执行 Host 的实际能力和版本；目录、help 或设计文档存在均不是功能通过证据。2026-10-09 再次核对六份固定上游源码及版本/help，仅纠正第12节 stdin 编码和空输入边界，新增断言仍属待验收。

## 目录

- [1. 目标、非目标与当前实现](#1-目标非目标与当前实现)
- [2. 系统职责与运行模式](#2-系统职责与运行模式)
- [3. 固定上游依据与命名冲突](#3-固定上游依据与命名冲突)
- [4. 正式命令契约](#4-正式命令契约)
- [5. TUI 与终端交互](#5-tui-与终端交互)
- [6. 模型、Provider 与凭据](#6-模型provider-与凭据)
- [7. 配置、目录与优先级](#7-配置目录与优先级)
- [8. 账户登录、令牌与会话](#8-账户登录令牌与会话)
- [9. 本地记忆与账户级同步](#9-本地记忆与账户级同步)
- [10. Host、Remote、线程与任务](#10-hostremote线程与任务)
- [11. 审批与非交互安全](#11-审批与非交互安全)
- [12. 输入输出、JSONL、错误与退出码](#12-输入输出jsonl错误与退出码)
- [13. 扩展能力、安装与迁移](#13-扩展能力安装与迁移)
- [14. H/I/P/R 实施职责](#14-hipr-实施职责)
- [15. CLI 专项验收矩阵](#15-cli-专项验收矩阵)
- [16. 实施时待验证的技术细节](#16-实施时待验证的技术细节)

## 1. 目标、非目标与当前实现

CAIdex CLI 是 Windows 11 x64 / Linux x86_64 的完整终端 Codex 客户端，覆盖本地终端、SSH/VPS、脚本/CI 与经授权的远程 Host 管理。所有命令、菜单、帮助、错误与交互文案使用英文；Windows/iOS GUI 继续默认简体中文、英文备选，布局见 [UI 规范](CAIdex-UI-规范-V1.md)。不增加 Android、macOS 原生 GUI 或新 Web 客户端；官方账户登录网页只是认证端点。

唯一 Agent、工具执行与审批真源是锁定的 **Codex 0.160.1 / d27764b82f7118f674371e6d6e76271d9d606edb**。CLI 复用共享 Host、Runtime facade、ModelProvider、Gateway 和 Credential Broker，不另造 Agent、工具执行器、审批引擎或模型 HTTP 传输。跨模型记忆不等于活动 Agent 跨 Provider 无损迁移。

当前 `apps/cli` 仅实现：`doctor`、`credentials status|set|remove`、`--version`、`--help`。doctor 是隔离 app-server 元数据检查，不启动模型轮次；credentials 仅管理执行端本地凭据，set 必须显式 `--stdin`，尚无掩码交互输入。现有测试与证据见[凭据设计](CAIdex-Credentials-设计与验收.md)和[Runtime 对照](CAIdex-Runtime-能力对照.md)，原证据完整保留。本文其余命令/界面/认证/同步/生产 Host 全部属于未来实施；源码阅读与 help 核对不算功能验收。

## 2. 系统职责与运行模式

| 模式 | 入口 | 正式职责 / 成功含义 |
| --- | --- | --- |
| 交互式 TUI | `caidex [PROMPT]` | 项目、模型、工具输出、审批、Queue/Steer、线程/Host 与恢复；退出界面不自动取消 Host 后台任务 |
| 非交互执行 | `caidex exec [PROMPT]` | 等待真实最终结果，支持脚本/CI、stdin、JSONL、取消/超时；不会转成可无限等待人工输入的后台任务 |
| 持久化 Host 任务 | `caidex task submit\|list\|status\|attach\|cancel` | Host 持久接收、执行、恢复、多端查看/审批；submit 成功只表示持久受理，不表示工具或任务完成 |
| 管理 | 模型/Provider/凭据/账户/记忆/Remote/扩展/doctor | 复用相同核心及权限契约；管理账户不管理另一台机器的 Shell 权限 |

Host 管理真实 Thread/Turn 与后台任务，CLI 是操作和显示端。CAIdex 官方云服务只管理账户、获准的 Chat/记忆与同步，不成为执行 Host。客户端本地 SQLite 和 Host journal 逻辑分离，官方云端仍 PostgreSQL + pgvector。CLI/SSH 连接消失只能证明连接消失，不能推断运行已经停止。

## 3. 固定上游依据与命名冲突

本轮在当前环境只运行固定二进制 `--version`、各命令 `--help`；读取固定 commit 源码，不启动 TUI、登录或模型调用。最新[官方 CLI 参考](https://learn.chatgpt.com/docs/cli/reference)仅作导航，版本事实以以下不可变源码及项目 [lock](../upstream/codex/lock.json)为准：

- [CLI 命令树](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/cli/src/main.rs)、[exec 参数](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/exec/src/cli.rs)。
- [exec 配置解析与服务端请求处理](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/exec/src/lib.rs)：`build_exec_config`、`handle_server_request`、stdin、interrupt、终态与退出。
- [exec JSONL 类型](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/exec/src/exec_events.rs)、[JSONL 投影](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/exec/src/event_processor_with_jsonl_output.rs)。
- [Slash commands](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/tui/src/slash_command.rs)、[默认 keymap](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/tui/src/keymap.rs)、[TUI 退出边界](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/tui/src/chatwidget.rs)。

固定版本已有 `agents/exec/review/login/logout/mcp/plugin/app-server/remote-control/completion/update/doctor/sandbox/debug/apply/resume/queue/archive/delete/migrate-rollouts/unarchive/fork/cloud/exec-server/features`。没有顶层 `task/models/provider/credentials/config/remote/account/memory/skills/plugins/threads`；不要因存在相关协议或 Slash command 就声称已有对应顶层命令。

2026-10-09 复核：上述 CLI main、exec lib、SlashCommand 三份缓存与 GitHub 固定 commit 原文逐字一致，二进制为 `codex-cli 0.160.1`，顶层及 exec/login/resume/fork help 与契约对应。上游还有平台门控的 `app`（Windows/macOS）及部分隐藏维护入口；不透传启动原生 Codex Desktop，也不因此增加 CAIdex macOS GUI。上游 `debug models` 仅为目录/诊断能力，不能代替正式 `models` 的 Host Registry、凭据状态及兼容证据。

| 冲突 / 等效能力 | 采用的正式规范 |
| --- | --- |
| 上游 `login/logout` 是 OpenAI/Codex 认证 | CAIdex 同名入口只管理 **CAIdex Account**；绝不直接透传上游认证，也不把账号令牌注入上游 `account/login/*`。`login status` 保留为 `account status` 只读别名；上游 `--with-api-key/--with-access-token/--device-auth` 不作为 CAIdex Account 参数。模型 Key 用 Broker |
| 上游 `--profile/-p` 是配置层，不是 Credential Profile | 保留配置层语义；新增模型选择参数 `--credential-profile`。已有 `credentials --profile` 仍是 CredentialRef.profile，不改现有命令 |
| 上游 `plugin`，提议 `plugins` | `plugins` 为正式管理入口，`plugin` 保留兼容别名；底层实际支持的子命令/启用操作在 P 按固定版本暴露 |
| 上游 `agents/resume/fork/queue` 与线程列表/管理 | 保留已验证的原生命令入口；`threads` 增加带 Host 归属的清单，不将上游 agent 列表等同于 CAIdex 持久任务列表 |
| 上游 `--remote` 接 app-server 地址，提议 `remote ...` | 保留地址选项的语义；CAIdex 常规目标用 `--host <host-id>` 及 `remote` 管理。两者同时指定拒绝，原始地址也须安全传输与独立 Host 授权；不能绕过 Host facade |
| 上游 `/memories`、`/logout`、`/daemon`、`/stop` | `/memories` 保留上游 Runtime 记忆含义，与 CAIdex Memory 明确分区；新增 `/memory`。`/logout` 明示退出 CAIdex Account，不清除模型 Key。`/daemon` 不冒充整个 CAIdex Host 管理；新增 `/host`。`/stop` 仍是停止后台终端，不偷换成 task cancel |
| 上游 doctor/update/cloud | CAIdex doctor 保留当前离线检查；update 只能更新经 CAIdex 验证的兼容套件，不自动追最新 Codex。上游 cloud 是 Codex Cloud，不冒充 CAIdex 云记忆或 `task`；无已授权可用后端则提示不支持 |

原生重要入口还包括 `exec resume/fork/review`、`review`、`completion`、`apply`、`sandbox`、`features`、线程 archive/delete/unarchive、worktree 和 queue。在 P 逐一验证并接入，帮助仅显示实际可用功能；纯上游维护/实验入口需标明所属系统与门控，不能靠无条件进程透传绕过 Broker/Host。命令重名处理属于本次正式契约，不改 Runtime 的认证、记忆或审批内部实现。

## 4. 正式命令契约

下面除明确列出的现有开发入口，**全部待实现**。未实现时不得展示成功样例冒充实测。`ID` 是不透明 Host/Thread/Task/Session/Memory 标识，不能以显示名称跨 Host 猜测；多个候选必须显式选择。

### 4.1 命令树与逐项对照

```text
caidex [PROMPT]
caidex exec [PROMPT]                 # includes resume / fork / review
caidex resume [THREAD_ID] [PROMPT]
caidex fork [THREAD_ID] [PROMPT]
caidex threads
caidex task submit|list|status|attach|cancel
caidex models
caidex provider
caidex credentials status|set|remove
caidex config
caidex remote list|add|test|use|attach
caidex login [--browser | --device-code]
caidex logout
caidex account status|sessions
caidex memory status|list|search|edit|delete|export
caidex memory sync status|on|off
caidex mcp
caidex skills
caidex plugins                      # plugin is a compatibility alias
caidex doctor
caidex --version
caidex --help
```

**没有 `register`、`signup` 或 CLI 邮箱验证注册流程。** 浏览器/Device 验证页仅登录已有账户，提示新用户通过 Windows/iOS 注册，不能在 CLI 登录流程嵌入网页注册入口。

| 入口 | 固定上游对照 | 参数、作用域、默认行为及失败 |
| --- | --- | --- |
| `caidex` | 默认 TUI 已有 | 当前 cwd 与选定 Host；可带初始 prompt；无 TTY 拒绝并提示 exec，不自动开始别的运行模式 |
| `exec` | 已有，别名 `e` 保留 | 继承 prompt/stdin、`--json`、`--color`、`--output-schema`、`-o/--output-last-message`、`--ephemeral`、`--skip-git-repo-check`；CAIdex 增加 `--host` 和显式 `--timeout <seconds>`；审批/结果见第11/12节 |
| `resume/fork` | 已有 | 显式 Thread ID，或 TTY picker/`--last`；保留 `--all` 与 resume `--include-non-interactive`；筛选只在授权 Host 内，headless 无 ID/last 报参数错误，不弹 picker；fork 返回新的 Thread ID与父引用 |
| `threads` | 顶层新增；agents 不完全等效 | `--host`、可选 `--project <id>`、`--limit <n>`、`--cursor <opaque>`、`--json`；默认当前 Host/cwd，分页大小按 Host 有界默认；显示真实状态/cwd/worktree与ID |
| `task submit` | 新增；Codex Cloud 不等效 | prompt/stdin 与显式 Host/cwd/model、`--operation-id <id>`；客户端首次生成幂等 ID并保存，重试复用同ID；受理后返回Task/Thread ID、状态与event sequence；无持久确认返回结果未知 |
| `task list/status` | 新增 | list 分页/Host筛选；status须Task ID或互斥 `--operation-id <id>`（用于提交回应丢失时查原操作），只读真实状态/Turn/待请求/最后seq；无权限或不存在不泄漏实体资料 |
| `task attach` | 新增；remote TUI是传输基础 | Task ID、`--after-seq <n>`；恢复快照/缺口，再进入TUI；无TTY只能显式 `--json` 观察事件。关闭观察端不取消任务 |
| `task cancel` | 新增 | Task ID与幂等操作ID；确认取消后返回终态，否则返回cancel-requested/unknown，不能假称完成；只取消该任务，工具外部副作用不保证可回滚 |
| `models` | 顶层新增；`/model` 已有 | 默认读执行 Host Registry；`--provider <id>`、`--json`；远程失联报离线，保留的目录标 stale，不用本机目录替代 |
| `provider` | 新增 | `list/show/configure/discover/test/default`；Provider ID、显式 Host与 Credential Profile引用。show/configure 只处理非秘密元数据；discover/test 是显式网络动作，可能产生调用成本，帮助说明其验证范围；失败不改当前模型 |
| `credentials` | CAIdex 已有，上游无同名 | 完整保留 `status/set/remove --owner --provider --profile --store`、`--kind`、`--directory`、`--variable`、set `--stdin`。后续安全掩码输入另在P接入，同一Broker，无get/export；当前不管理remoteHost |
| `config` | 顶层新增；上游 `-c` 等效部分 | `show/get/set/import`；非秘密key，set必须 `--scope user\|project\|host`，默认不写隐式scope；Host写入需管理员允许，项目需信任；import显式选源与范围。show显示resolved值/来源并脱敏 |
| `remote list/add/test/use/attach` | 新增；`--remote` 原生已有 | list只读配置/状态；add Host ID和 `--ssh <target>`（复用OpenSSH alias）；test独立连接授权；use保存明确默认Host，`--local`恢复本地；attach Thread ID与Host，不迁移线程、不代表终端屏幕转发 |
| `login/logout` | 同名但账户体系不同 | 登录默认按环境选择浏览器/Device，可显式覆盖；logout仅当前CAIdex会话，先处理未同步数据，服务器未确认撤销明确pending；不删除Key/项目/Host任务 |
| `account status/sessions` | 新增 | status区分本地保存/服务器核验/离线；sessions需在线，显示当前/平台/时间与sessionID。规划 `sessions revoke <id>`、`revoke-others`，按敏感权限重认证，首次有效撤销后拒绝后续云请求 |
| `memory status/list/search` | 新增；上游记忆不是同一服务 | 默认本地；`--scope personal\|project`、project须 `--project <id>`；search显式查询、分页/limit，不扩权限。status包括自动记忆、服务端sync/待确认、未同步数量及Provider权限；无账户可查游客本地数据 |
| `memory edit/delete/export` | 新增 | edit Memory ID、base revision及文件/stdin正文；delete显式ID/范围确认，export `--output <path>`/scope。只访问当前归属，导出不含秘密、不覆盖文件而不确认；同步关闭不使用云导出绕过下载禁令 |
| `memory sync status/on/off` | 新增 | status最小控制面；on需在线认证与首次范围确认，可用 `--scope personal` 或显式项目选择，不能通用yes代替范围；off必须选择 `--retain-cloud` 或 `--delete-cloud`（TTY可问，headless无选择拒绝），B需重新认证/本地保留确认 |
| `mcp` | 已有 | 保留list/get/add/remove/login/logout；明确当前Host、安全配置与MCP自己的OAuth归属，不与CAIdex Account登录混用 |
| `skills` | 顶层新增；`/skills` 已有 | list/show及TUI选择引用；安装/启用仅暴露固定Host已验证能力，不虚构独立管理协议 |
| `plugins` | 上游 `plugin` 已有 | 沿用实际管理能力与权限，保留singular别名；远程作用在Host，不隐式本机安装 |
| `doctor/--version/--help` | 上游有同名；CAIdex自有已实现 | 当前doctor只离线元数据；完整doctor后续增加脱敏分项，云不可用与本地故障分别报告。版本显示CAIdex及锁定Runtime；help英文且只标实际实现状态 |

参数错误在读取秘密/提交工具前拒绝；远程写入须独立 Host ACL，不因为 `--owner` 或 `--host` 字符串获得授权。未来管理命令的 `--json` 为版本化对象；task attach是JSONL事件。凡交互确认必需且无TTY的命令，要求明确范围与操作选项，否则失败，不能猜用户选择。

`task submit` 使用 Host 允许的持久任务审批配置，受理结果显示实际 policy/reviewer/sandbox，不能复制 exec 的无头 Never 默认值后假称能等待跨端人工审批。自动化提交也不能擅自提升该配置；Host 不支持持久受理或所需审批能力时明确拒绝。`exec` 仍等待最终结果，不因已有手机客户端在线就改变审批模式。

### 4.2 通用非秘密选项

`--host <id>` 默认本地，除非已保存明确默认远程目标；与原生 `--remote <ADDR>` 互斥。`-m/--model <qualified-id>`、`--credential-profile <id>`、`-C/--cd <dir>`、`-p/--profile <config-profile>`、`-c/--config <key=value>` 只对支持的命令生效。未知/重复互斥参数拒绝，不把Key/token作为参数接收。

继承 `--sandbox`、TUI `--ask-for-approval` 等选项时，只能请求Host许可上限之内的策略；`--approve-for-me` 须明确允许且展示实际自动审查器配置。危险绕过/ignore-rules/ignore-user-config/no-daemon等原生选项不能隐式启用或削弱管理员边界。`--no-daemon` 若保留，只能进入明确标注的独立本地维护模式，不能作为共享Host失败后的备用Agent路径；是否暴露该维护入口留P验证。

## 5. TUI 与终端交互

保持上游 conversation/composer、工具进度、真实审批、Diff/Review、项目/worktree和按键习惯。状态区显示执行Host、cwd/worktree、真实Thread/Turn、当前任务运行模型、下一轮选定模型、连接状态；账户与memory状态是独立入口。无连接的旧状态必须标 stale。

| 入口 / 默认键 | 正式行为 |
| --- | --- |
| `/model` | Registry选择器，展示Provider/Profile/能力/证据；当前Turn不改，下一轮才生效 |
| `/status` | 真实任务/模型/权限/usage；另区分CAIdex账户、Host授权与同步权威状态 |
| `/host`、`/memory`、`/help` | CAIdex新增Host选择、记忆管理及帮助；调用共享核心，不新建执行流程 |
| `/new`、`/resume`、`/fork`、`/diff`、`/review`、`/permissions`、`/mcp`、`/skills`、`/plugins`、`/keymap` | 继承固定实际语义与运行中可用性门控；菜单不能提供当前线程不支持的动作 |
| `/memories`、`/stop`、`/ps` | 明示上游Runtime记忆、后台终端停止/列表；`/memory`及task cancel另列，不混用 |
| Enter / Tab | 默认提交 / Queue；有活动Turn时Enter按真实Steer能力发送，Tab排队，弹层和粘贴状态优先；不支持Steer时明确失败或要求Queue，不偷偷重开线程 |
| Shift+Enter / Ctrl+J | 换行，沿用keymap/终端实际编码；粘贴与输入法组合不误提交 |
| Esc | 先关闭当前浮层；主会话按上游中断当前Turn；不自动取消整个持久任务 |
| Ctrl+C / Ctrl+D | 按上游输入上下文处理清空/取消/中断及双按退出提示；空输入与编辑状态有区别，不硬改成单一Stop。界面退出只detach共享Host；若先明确发送interrupt，须显示实际结果 |
| Ctrl+T / Ctrl+G / Ctrl+O / `?` | 默认transcript / 外部编辑器 / 复制 /快捷键提示，保持上下文及系统可用性；运行时keymap可覆盖并显示实际绑定 |
| `/quit`、`/exit` | 退出界面/取消订阅，保留后台Host；上游shutdown-first不得被映射为关闭整个共享Host |

Queue/Steer都来自真实Runtime，不以本地队列假冒服务端受理。审批按availableDecisions生成快捷键；Help不承诺所有请求都有a/d/session。终端resize/窄宽布局使用上游组件，有界滚动与输出，不丢请求内容；支持ANSI自动检测、无颜色与 `--no-alt-screen`，不强制终端剪贴板。Unicode/中文路径作为OS路径处理，不以shell拼接或错误截断破坏；实际WindowsTerminal/LinuxSSH、bracketed paste与keymap降级需P/R验证。

## 6. 模型、Provider 与凭据

### 6.1 Registry 与切换

模型来自**当前执行Host** Registry/Provider配置，展示Provider、模型ID/原生ID/显示名、Host、Credential Profile配置状态、能力三态与dialect、兼容报告来源/模型版本/限制、最近连接测试及时间。Unknown/Configured/ProviderCatalog/ProtocolFixture不等于LiveRuntime；Full/Compatible仍须实际模型+固定Runtime报告，沿用现有 `model/core` 门槛。

公开标识采用 `provider-id/registry-model-id`，按第一个 `/` 分隔Provider，其后完整作为该Provider的Registry ID（可含 `/`，不把它当文件路径）；Provider ID沿用小写安全ID，显示名称与native_model不作为唯一键。Host聚合时保持Registry原始ID，用完整限定值解决同名冲突；Gateway的公开route映射到原ID/endpoint/profile。短名称仅在当前Host唯一匹配时接受，否则拒绝并列候选。此是未来Host聚合契约，不修改现有每Provider Registry键值。

`/model` 和启动 `--model` 只设置下一轮选择。正在执行的Turn模型来自Host结果，不改变；默认值改变不影响其他活动线程。同Provider仅实际验证兼容的组合可续同线程，目标缺工具/推理/所需dialect等能力明确拒绝或要求确认已知限制，未知能力不当Supported。跨Provider建关联分支/新线程，历史适配与opaque载体限制沿V3，不绕绑定、不承诺无损迁移。缺Key时提示**目标执行端**配置，不借客户端或另一Host的Key。

### 6.2 Provider / Profile / Endpoint

支持Provider选择、多个Credential Profile、安全新增/更新/删除Key、状态、Endpoint、目录发现、Test Connection及默认模型。便捷入口复用Broker；GUI/CLI同执行端按权限共享非秘密配置与CredentialRef，不共享到其他Host，账户切换也不扩大Key权限。

终端新增Key输入必须不回显、不进shell历史/日志/命令参数；保留既有 `--stdin` 安全路径，无秘密查看/导出。系统store不可用明确报错；Linux VPS可显式选择Git外0700目录/0600文件的**权限保护明文**，不能宣传加密。新交互输入是P功能，当前set仍拒绝TTY。

Key属于实际执行端：Windows本机Host、LinuxHost、iOS本地Chat各自保存；Remote CLI只接收配置状态/引用，不复制远程已存Key。未来远程保存新Key须独立授权且安全传输到明确目标，不能无条件把当前 `credentials` 命令当远程管理。Endpoint变化应暂停原Profile认证复用，核对新地址并显式授权；HTTPS/TLS、禁重定向/无隐式认证fallback等沿现有transport，不将原Provider Key发送到未授权地址。账户Token、Gateway随机token、SSH/Relay身份绝不互换。

## 7. 配置、目录与优先级

### 7.1 正式存储位置与分工（待实现）

| 内容 | Windows | Linux |
| --- | --- | --- |
| 用户非秘密配置 | `%APPDATA%/CAIdex/config.toml` | `${XDG_CONFIG_HOME:-~/.config}/caidex/config.toml` |
| 用户配置Profile | 配置目录 `profiles/<id>.config.toml` | 同结构；`--profile`仅叠加本次配置层 |
| 本地状态/缓存 | `%LOCALAPPDATA%/CAIdex/` | `${XDG_DATA_HOME:-~/.local/share}/caidex/` |
| 本地记忆/同步 | 状态目录 `accounts/<user_id>/memory.sqlite` 或 `anonymous/<local-id>/memory.sqlite` | 同结构，OS用户目录保护；独立索引/附件/outbox归属同分区 |
| Host配置与Runtime | 状态目录 `hosts/<host-id>/`，非秘密配置、journal、隔离runtime目录各自分开 | 同结构；服务用户与交互OS用户之间只经授权Host协议访问 |
| 项目配置 | 项目根 `.caidex/config.toml` | 同结构，可信项目才生效；不得含秘密 |
| 账户与模型秘密 | 独立系统安全存储namespace | Secret Service；VPS明确授权Git外受保护文件，两类秘密分别namespace |

Host journal和memory.sqlite不合表、不相互自动上传。生成的Runtime配置/rollout仅Host管理，使用隔离子进程数据目录，不修改全局CODEX_HOME或自动读用户 `~/.codex/auth.json`。上述新目录与DTO在H/I/P版本化实现；当前凭据命令仍需显式store/directory，不假称已有默认路径。

### 7.2 解析与写入

非安全配置从高到低：①本次显式命令参数（含 `-c`）→②当前会话显式选择→③已验证可信项目配置→④当前执行Host/用户默认配置（用户选择配置Profile叠加此层）→⑤内置默认。来源和值可在config/status诊断，始终脱敏。

环境变量只作明确的本进程默认/路径引用：规划 `CAIDEX_HOST/CAIDEX_MODEL/CAIDEX_CREDENTIAL_PROFILE` 属第④层临时覆盖、不能覆盖①～③；`CAIDEX_CONFIG_HOME/CAIDEX_DATA_HOME`显式重定位对应应用目录，检查OS权限/归属，不能导入秘密。已有 `CAIDEX_CODEX_BIN` 仅指定锁定二进制路径，版本不符拒绝。环境凭据仍为现有Broker的显式reference→variable映射，不自动扫描Key变量，不写回配置。

命令参数/会话选择默认不持久；config set/provider default/remote use是显式写入动作。Remote情况下项目配置来自执行Host项目，客户端默认只能提出参数，Host决定可用配置。配置写入/覆盖必须拒绝秘密字段（Key、Account/Gateway token、SSH私钥），不能借通用-c/config set把它们放进普通配置。可信判定按执行端项目身份/实际路径和既有Runtime trust机制验证；项目不能声明自身可信或脚本化覆盖安全store。

**权限、sandbox、Host管理员上限、Host ACL及账户Memory Sync不适用普通覆盖优先级。** 客户端请求不得超过Host政策；同步由认证服务版本/epoch权威决定，项目文件/环境变量/`-c`不能打开或绕过。迁移必须保存来源/schema/旧配置，拒绝无法安全理解的安全字段，不用宽松忽略实现升级。

## 8. 账户登录、令牌与会话

### 8.1 只登录已有账户

Windows/iOS应用内注册并验证邮箱，CLI不注册；邮箱可变而user_id不变。CLI未登录仍可按既有权限使用已配置本地Codex/模型/项目/本地记忆，云端数据必须认证。

`caidex login` 在有本机图形浏览器且非SSH时优先浏览器；SSH、无图形VPS优先Device Code。提供互斥 `--browser` / `--device-code` 显式覆盖；环境检测不可靠时显示所选方式。浏览器打开/loopback失败给出Device Code回退入口，交互确认后重新开始；无TTY不能自动发起第二个授权流程。显式选择失败清楚返回原因和另一命令，不收集终端密码、不假报登录。

### 8.2 系统浏览器 + Authorization Code + PKCE

CAIdex CLI属于公开OAuth客户端，不能内置所谓长期保密Client Secret。采用成熟OAuth/OIDC库；规范依据 [RFC 8252](https://www.rfc-editor.org/rfc/rfc8252.html) 与 [RFC 7636](https://www.rfc-editor.org/rfc/rfc7636.html)，认证服务在I实现，P接入CLI。

1. 生成高熵PKCE verifier、S256 challenge、state与一次性上下文；若使用OIDC同时绑定nonce。verifier留进程内，不进诊断。
2. 临时监听只绑定loopback IP随机可用端口，固定回调路径；不得监听0.0.0.0/公网。回调可使用本机 `http://127.0.0.1`/IPv6 loopback，官方登录/令牌端点必须HTTPS；本机HTTP例外不扩展到网络传输。
3. 打开系统默认浏览器到官方CAIdex登录页；用户只在官方页面用已有邮箱密码/Passkey认证，不传密码给CLI，不使用普通WebView。仅注册的client/redirect组合获准。
4. 回调核对state、一次性上下文/路径，授权码绑定client、redirect、challenge与有效期；S256交换一次性码，不降级plain。伪回调、code重放、并发登录串上下文失败。
5. 验证issuer、目标audience/client、令牌归属和必要签名/时间/nonce属性；opaque token通过可信后端核验，不凭token文本自认user_id。创建独立CLI auth_sessions。
6. 安全保存令牌、关闭监听并清除一次性材料，查询账户设置。取消、超时、端口抢占或交换失败也关闭监听；不会用未校验回调当登录成功。

授权范围只包含本次需要的账户/云数据能力，必须由官方后端和用户授权共同限定；认证同意不自动开启 Memory Sync、扩大项目访问、允许历史资料上传或授予 Host 权限。令牌保存失败不能显示登录成功；新建会话应尝试撤销并明确报告尚未确认的结果，不能留下可供后续启动误用的半写入登录状态。issuer/client/回调和资源 audience 的校验分别按选定成熟 OAuth/OIDC 库完成，不把 ID Token 当 API Access Token。

### 8.3 VPS / SSH Device Authorization Grant

`caidex login --device-code` 使用同一官方账户后端的 [RFC 8628](https://www.rfc-editor.org/rfc/rfc8628.html)。用户授权的是本次CLI会话，不是旧设备批准新设备的硬件绑定；任意受支持浏览器可完成，默认不限制普通登录设备数量。

1. 请求Device Authorization，接收秘密高熵device_code、可展示短时user_code、官方HTTPS verification_uri、expires_in和interval。device_code不显示、不记日志；用户码不作为长期凭据。
2. 终端显示官方URL、user_code、期限和“Approve only a CLI request you started.”；官方网页明确目标CAIdex CLI、请求上下文与授权范围，降低诱骗他人批准风险。页面无账户注册入口。
3. 用户在Windows/iPhone或其他浏览器登录已有账户，输入用户码并明确确认或拒绝。不需要事先登录的设备或旧设备批准。
4. CLI按服务端interval轮询；未提供interval按标准默认5秒，authorization_pending继续，slow_down将本次及后续间隔至少增加5秒，连接超时进一步退避。不得并发密集轮询。
5. access_denied/expired_token/取消/总超时停止；过期不能无用户动作无限新建请求。建议有效期约10分钟，以服务端实际政策为准，未部署/未验证。
6. 批准后请求一次性消费并绑定client/登录上下文，校验令牌并独立建CLI会话，安全保存/读取账户设置。服务端限流暴猜/滥用，错误不给token；成功仅在核验后显示。

### 8.4 令牌、撤销与状态

CAIdex Account access/refresh token与模型Key、Gateway token、Host SSH/Relay秘密分别namespace/ACL，复用可用安全存储能力但不混用认证语义。Windows系统store；Linux优先系统store，无Secret Service可明确选Git外保护文件，不能静默降级。账户token不进项目配置、Chat、Memory、日志、普通同步库或终端。

建议Access Token约10分钟、Refresh Token轮换且约30天闲置到期；具体数值、token类型与合法并发刷新恢复在I安全/可用性测量后固定，**不是已上线参数**。服务端只存刷新安全摘要、维护token family和独立auth_sessions；重放检测/撤销后受保护请求必须拒绝，即使Access Token尚未自然过期，不能只靠JWT签名/到期实现撤销。

CLI会话出现在Windows/iOS“设置 → 账户与安全 → 已登录设备”，显示当前设备/平台/登录及活动时间，可独立撤销/退出其他设备；名称/随机安装ID仅展示。注销/敏感撤销按后端重认证规则；不提供CLI注册或终端密码登录绕过。

核验账户及权威设置后可显示以下**未来文案示例**：

```text
Login successful.
Account: user@example.com
Memory sync: Enabled
```

或 `Memory sync: Disabled`。身份核验成功但设置获取失败时显示 `Account authenticated. Memory sync: Unknown (server unavailable).` 并暂停同步，不能猜默认值覆盖服务端。已有本地token离线时显示 `Saved session; not verified online.` 及最后确认版本/时间；明确区分离线本地能力、当前服务端会话与sync状态。

logout先展示并明确处理未同步内容，进入退出提交后立即停账户数据传输、锁定原账户缓存。网络不可用的撤销记录为pending，不假称服务器已经撤销；重试材料若含凭据，只能留在独立安全存储中，普通outbox仅记公开操作状态，不能为了补交撤销继续下载/上传该账户正文。账户退出/被撤销不自动删除Codex项目、模型Key、不自动取消全部Host任务、不撤销独立Host配对。离线下载资料不能保证远程立即擦除。

## 9. 本地记忆与账户级同步

### 9.1 本地可用与归属

Linux CLI正式采用本地SQLite记忆与同步缓存，Windows CLI与GUI在同OS用户/执行端、权限允许时共享一致账户分区/核心。按OS用户、CAIdex账户（或匿名local-id）、项目ACL隔离DB/索引/附件/outbox；不同账户不展示/检索/使用上一账户记忆。登出保留数据仍锁定原归属，不能作为游客自动开放；游客数据并入账户须显式确认。多进程共享SQLite连接/迁移/锁策略在I验证。

区分个人/项目长期记忆、Chat原文、Host执行journal、临时上下文。Memory来自账户/授权项目，跨Chat模型复用，整合/Embedding模型独立；复用ModelProvider，无模型可保存、关键词/结构检索，Embedding换空间版本重索引。自动记忆控制提取/整合/使用，与sync独立；用户可list/search/edit/delete/export，来源/revision/纠错/权限及prompt injection防护沿账户设计，记忆不能绕Runtime审批。

### 9.2 同一账户权威状态与登录继承

服务器统一保存 `enabled/settings_version/cloud_epoch/authorized scope`，**新账户默认false**；Windows、iOS、CLI是同一状态。Windows已开启，CLI随后登录应继承Enabled；iOS已关闭，CLI应继承Disabled。CLI不创建自己的独立默认开关，重新安装/重新登录不重置账户设置。控制面查询可在关闭时进行，不下载记忆正文。

**继承Enabled不等于授权上传这台CLI的全部历史记忆。** 未归属当前user_id、其他账户、匿名积累或未批准范围的旧资料保持隔离，只在明确归属/首次上传范围确认后入队。之后在当前账户授权范围内正常生成的记忆可增量同步；不上传整个项目、Host journal、未经选择的来源或任何Key。初次开启和已有Enabled设备首次上传许可是两个不同状态。

共享核心分别记录服务器权威开关、当前本地数据归属/上传许可和待确认控制操作；上传许可不是另一个设备级同步开关。CLI登录只读取权威状态，不隐式调用 `sync on`，也不清除本机尚未确认的关闭意图；有此意图时本机继续停传，先提交关闭或明确处理版本冲突，再处理数据。on/off被CAS拒绝时不静默反转用户意图或恢复旧队列。

### 9.3 CLI同步状态机与命令

| 动作/状态 | 本机 / 服务端要求 |
| --- | --- |
| `sync status` | 在线读取权威enabled/version/epoch/范围；离线展示最后确认和pending/unknown，不以本机bool替代 |
| `sync on` | 必须登录且在线，显示全账户影响，首次/新增上传选择范围与归属；CAS提交已有settings_version，确认后才数据同步；并发冲突保留数据并刷新，不强制覆盖 |
| `sync off --retain-cloud` | 文案 `Stop sync and retain cloud memories.`；本地立即停上传/下载，冻结队列；服务器确认后全账户停后续记忆上传/下载和旧job提交，旧云资料保留 |
| `sync off --delete-cloud` | 文案 `Stop sync and delete cloud memories.`；确认本地保留范围/未同步数据/其他设备影响，重新认证。先关闭+epoch屏障，后幂等删除正文/revision/embedding/敏感jobs，保留最少墓碑 |
| 离线关闭 | 显示 `Waiting for server confirmation to disable memory sync.`；本机即停，不假称其他设备已停；重连先控制请求、再状态/epoch，绝不先冲刷旧outbox |
| 离线B删除 | 只记录关闭/删除意图，本机停传；在线重新认证和范围确认后提交破坏性删除，未经有效重认证不能执行；分开报告disabled与deletion completed |
| 关闭/删除后重开 | 重验最新epoch、墓碑/来源屏障、各端revision及明确选择；本地保留副本脱离旧云关联，不凭旧ID/新ID重提取复活已删除数据 |
| 旧设备/旧客户端/旧任务 | 服务端拒绝过期状态/epoch、已撤销会话及非授权请求；返回冲突/升级提示，不靠客户端遵守实现开关 |

关闭后自动本地记忆、基础检索、跨模型使用和管理仍可继续。Chat历史同步与Provider记忆外发权限分别控制；关闭CAIdex云同步不保证第三方模型API不收到授权上下文，本地限定记忆未经独立许可不得发送外部。记忆整合作业用实际执行端Key，Host离线等待，不能复制Key到云后台。

云删除不代表离线副本/本机旧备份立即擦除。沿既定tombstone、epoch、cursor重建、source屏障、幂等operation、base revision与版本迁移机制，不另造同步协议。只做VPS本机有限备份/检查/恢复和手动导出，**不新增异地备份**；云数据库与本机备份可在整机/磁盘损坏时全损，客户端SQLite不是全量备份。

## 10. Host、Remote、线程与任务

默认本地Host，只有显式选择或已保存明确默认才使用远程；Host失联不静默改本机执行相同任务。SSH沿L既定协议/OpenSSH与配对；Relay在Q沿Noise/Snow E2EE，不新增执行机制。原生app-server地址能力只是传输基础，不等于完成CAIdex Host ACL/journal/恢复。

每个Thread绑定Host、项目、cwd/worktree；切Host不迁移活动Thread。CLI/Windows/iOS只有取得该Host执行权限才可操作同一线程/审批；账户设备列表和Host配对列表分别管理。任务是Host持久化管理对象，包含提交operation、Thread/Turn引用、真实状态和事件seq，不是新Agent或云端任务执行器。

| 状态/操作 | 真源与恢复规则 |
| --- | --- |
| New/List/Resume/Fork/Status | 真实Host/Runtime结果；fork新ID/父关联，resume恢复原Host，不把cached记录当已运行 |
| Queue / Steer | Runtime真实队列/当前Turn前置条件；Host持久化受理，失败/过期不能显示成功 |
| Interrupt Turn | 只针对明确Thread/Turn，请求后确认真实interrupted；不自动取消整个任务 |
| Attach / Detach | 订阅/取消订阅，快照+event sequence补齐；多端同一真源，连接断开不取消任务 |
| Task Cancel | 明确Task ID，Host取消/使相关待请求失效并确认；未知结果保留未知，不因客户端断开推断取消 |
| Submitted / Running / Blocked / Completed / Failed / Cancelled / Unknown | CAIdex状态投影真实Runtime+Host日志，标明等待审批/用户输入/离线/已确认终态；不编造完成或重启后自动继续非可恢复步骤 |

提交请求ID/幂等操作、载荷hash、Host/Thread/Turn归属先落盘，事件落盘后广播。网络超时先查同操作结果，不能重造ID重新执行工具；工具副作用结果未知时不自动重跑，不承诺外部exactly-once。SSH断开、Host重启、事件缺口需要可信快照/seq重建并展示未知项；保留Diff/工具结果与项目归属。Host任务等待审批/用户输入需明确有界资源、阻塞/过期规则，由H按Runtime支持设计，不自动批准、不无限忙轮询。

## 11. 审批与非交互安全

### 11.1 交互TUI

Runtime允许即按Host权限执行，需要审批则暂停并展示Host、cwd、真实操作、Host/Thread/Turn/Request ID及availableDecisions。仅显示真实支持的批准/拒绝/取消/会话/策略范围，不能硬编码Allow/Deny/Session/Policy全部出现；批准范围由Runtime/Host校验，长等候不自动批准。审批拒绝、取消Turn与禁止策略分别显示。

固定TUI有shutdown-first退出路径；CAIdex共享Host接入必须把界面退出处理成detach，不触发全Host shutdown。保留用户明确中断当前Turn的动作；不能声称直接调用未经适配的上游TUI进程已经满足此边界。

### 11.2 exec无头行为

固定源码先设置 `AskForApproval::Never`；`build_exec_config` 对最终解析为AutoReview的配置，在非强制保留无头策略时可能重建并采用其配置策略。故报告**实际解析后的approval policy/reviewer/sandbox**，不承诺所有配置绝对Never，也不把自动审查器当自动越权许可。

普通无人工exec：sandbox内允许动作可执行；权限不足不得自动获批。源码 `handle_server_request` 对command/file人工审批与request_user_input返回“不支持exec”的RPC错误；MCP elicitation明确返回Cancel（不是Decline或假填表），附加线程同样处理。CAIdex沿此语义，不弹无人能回应的界面、不代答用户、不将exec偷偷转成task等手机审批；无危险绕过默认。需要可跨端审批的持久任务用task submit。

可用的自动审查器/策略只在Host明确配置并通过固定Runtime验收后生效。上游任何安全弱化参数不覆盖Host管理员上限；未知安全配置拒绝。模型失败、人工拒绝、policy denied、interaction unavailable、cancelled是不同错误类别，不能都假称用户拒绝。

### 11.3 Host任务跨设备审批

Host先持久化真实请求和授权上下文，绑定Host/Thread/Turn/Request ID与Runtime可用决策，多端看到同一有效请求。独立Host授权的Windows/iOS/CLI可回复，首次有效结果生效；后到响应为handled/expired/invalid。授权撤销、Turn结束、任务取消与Host恢复要使过期请求不可再批准。

账户登录、设备名称或账户memory权限不能代替Host ACL。断连/超时不批准，重启不重跑未知工具；有效决策到Runtime的提交结果未知不能盲目重发。H定义持久竞争/过期/结果查询协议，P接入，R实测竞态；现有局部单连接回应约束不冒充跨重启已实现。

## 12. 输入输出、JSONL、错误与退出码

### 12.1 终端与流

- TUI要求TTY，不能把pipe输入当交互批准。exec可无TTY；无prompt或显式 `-` 从stdin读到EOF，空或仅空白输入失败。根exec已有位置参数prompt时，非空pipe按固定上游追加 `<stdin>`上下文，空或仅空白pipe忽略并保留原prompt；resume/fork/review分别沿其真实参数解析，不假定全部附加stdin。prompt与Key输入通道不同，不能猜输入是秘密。
- prompt解码沿固定 `decode_prompt_bytes`：UTF-8（可带BOM）、带BOM的UTF-16LE/BE；拒绝UTF-32 BOM、非法UTF-8、UTF-16奇数字节或无效代理对，不猜测无BOM的UTF-16编码。这不改变现有credentials set的UTF-8/长度/仅移除一个末尾换行规则，不能对秘密套用prompt裁剪。
- 人类exec：最终助手内容stdout，进度/诊断stderr；`-o`输出最终消息文件，写失败须报告，文件可能含私人项目内容。schema约束与模型能力遵真实结果，不保证无能力模型自动满足。`--ephemeral`沿原生不保存会话正文，不取消Host用于幂等/安全恢复的最少操作状态，帮助明确范围；不以它擦掉审批审计或改为第二Runtime。
- `--json`：stdout仅JSONL，无ANSI/欢迎词/登录日志混入；stderr诊断脱敏。`--color auto|always|never`继承固定exec，NO_COLOR及非TTY行为在P实测，always不污染JSONL。
- 默认无CAIdex额外任务总超时（沿上游）；Host/Provider已有有界传输/阻塞策略仍有效。可显式 `--timeout <positive seconds>` 设置客户端等待及向Host中断的期限；到时不声称副作用回滚。中断确认失败标unknown，可用status查同Task/Turn，不重提交。
- Ctrl+C exec沿固定TurnInterrupt并等待真实结果，不能只杀本机显示端。SIGTERM在可处理平台请求中断/保存恢复点，SIGHUP/SSH断线与不可捕获SIGKILL/进程崩溃只证明客户端离线；持久Host任务继续或按实际安全阻塞。Windows控制事件/Unix信号差异必须实测。

### 12.2 两类JSONL互不混淆

**默认 `exec --json`保留固定上游结构**：`thread.started`、`turn.started/completed/failed`、`item.started/updated/completed`、`error`与固定payload。按 `exec_events.rs` 版本快照校验，不将app-server原始通知误称exec JSONL。它没有CAIdex schema_version，文档版本绑定Codex0.160.1；不静默包装/增加前导元事件破坏现有脚本。

CAIdex扩展事件使用显式 `exec --json --json-format caidex-v1`，task attach `--json`默认相同CAIdex envelope。未来字段固定：`schema_version: 1`、`type`、`host_id`、可选 `task_id/thread_id/turn_id/request_id`、可用时Host单调 `event_sequence`、`timestamp`、`data`；缺ID为null/省略而不造值，非Host本地错误不伪造seq。`runtime.event` 的data保留真实上游类型和扩展，`task.accepted/status`、`approval.pending/resolved`、`operation.unknown`、`error`分开。升级只在明确新schema版本切换，消费者保留未知字段/事件。

管理 `--json`单对象包含schema_version、operation/status、公开标识或安全error；既有credentials JSON保持原reference/configured/readOnly等，不强加新envelope。Task submit返回受理对象，不输出伪 `turn.completed`；task attach断开不输出伪task cancelled；终态由Host结果产生。

### 12.3 错误与退出码（未来契约，保留既有例外）

| 码 | 范围 / 含义 |
| --- | --- |
| 0 | 操作确认成功；exec仅真实成功终态，submit仅持久受理，attach仅正常detach/观察结束；status成功读到failed任务仍可0，任务状态在payload |
| 1 | 已有CLI运行/参数错误保持现状；正式exec沿固定上游一般失败/被中断终态1，不承诺Ctrl+C总是130 |
| 2 | 未来clap参数/缺非交互确认选项错误，与运行失败区分；现有手写credentials/main目前仍1 |
| 3 / 4 | CAIdex新增管理命令需要账户认证/会话失效 / 权限或策略禁止；exec为兼容仍1，细类写安全error |
| 5 / 6 | 新增管理命令依赖/Host/安全store不可用 / 版本或revision/settings冲突；exec兼容仍1并输出具体分类 |
| 75 | CAIdex扩展提交/中断结果未知，须查原operation/任务，禁止盲重试；不同于可安全重新执行的一般失败 |
| 124 | 显式CAIdex等待超时，报告已确认中断或结果未知，不是Host任务已经成功/取消证明 |
| 128+signal | 由OS/shell强制终止或显式信号退出产生的平台状态，不包装成Runtime终态；非Windows通用约定 |

安全错误分类至少有invalid_argument、authentication_required/session_revoked、host_unauthorized/policy_denied、credential_missing/store_unavailable、model_unsupported、interaction_unavailable、user_denied/cancelled、host_offline、rate_limited、timeout、protocol_incompatible、conflict、operation_unknown。保留Host/Thread/Task公开ID、是否可查询/安全重试，不返回Provider原始body、token/Key/敏感路径或未授权实体。diagnostics可显示配置来源，不导出秘密；模型/工具正文属于用户输出，不作为可公开诊断上传。

## 13. 扩展能力、安装与迁移

MCP servers/elicitation、Skills、Plugins、Queue/Steer、project/worktree、sandbox、tool output、Diff/Review均复用实际Host/Runtime。先核对固定协议、功能开关、平台及执行端配置，再显示入口；MCP第三方OAuth、安全store与CAIdex Account身份隔离。上游Runtime记忆与CAIdex Memory不自动合并/同步，不能借原生记忆功能绕过账户归属、自动记忆关闭或Provider外发权限；P验证原生记忆生成/注入的门控与双重注入风险，保留时明确来源并执行相同隐私边界。仅有UI名称或schema入口不得评Full；不支持能力明确拒绝而不是fake成功。

正式分发Windows x64 CLI / Linux x86_64 CLI，显示CAIdex/锁定Runtime版本，校验下载来源与包完整性；版本/平台/Host协议不兼容拒绝执行。`completion`生成实际命令树补全，不把计划命令提前冒充可执行。update采用CAIdex验证过的Runtime/Host/CLI组合，发布渠道、包签名及回滚方式在P/R确定，不自动升级上游破坏pin。

可选导入原生Codex配置必须显式选择路径与非秘密范围（模型配置、项目设置等），先preview兼容性，无法支持字段报告并保留源文件。不自动复制/上传auth.json、API Key、token、SSH/Relay秘密或整个历史；CredentialRef重新在执行端安全配置。旧线程仍由所属Host/Runtime验证resume，不自动跨Provider/Host迁移。

SQLite schema与Host/账户API均版本化。迁移前检查可用空间、兼容窗口、备份/锁，保留旧库和未同步outbox/控制请求；失败不丢资料/墓碑，不让旧客户端写新库。回滚不支持新schema时停止相关写入并提示恢复策略，不用降级覆盖；普通本地迁移副本不新增自动异地云备份。

## 14. H/I/P/R 实施职责

| 阶段 | CLI相关正式交付 | 验证边界 |
| --- | --- | --- |
| F/G | 按HANDOFF当前恢复点继续Qwen/OpenRouter/Gateway；DeepSeek Lite及固定Runtime已完成各自离线三平台验收 | 不被文档任务重做/跳过；已有Provider/Runtime离线CI不代验CLI、商业Live/Full或生产Host |
| H | 本地Host生命周期/attach-detach、SQLite journal、线程/任务状态、seq恢复、提交幂等/unknown、多端有效审批/竞争与安全阻塞 | 共用真实Runtime，不变云Agent；CLI/SSH退出不停止Host，重启未知不重跑 |
| I | 账户公开客户端PKCE/Device协议、CLI auth_sessions/token轮换撤销、settings API、Linux本地memory/cache DTO与迁移、账户同步/epoch/删除/首次上传许可 | 共享Rust/API与服务端能力先稳定；CLI完整TUI登录入口仍P，非提前做新GUI |
| J/K | Windows GUI/CLI/同机Host配置及CredentialRef权限一致，设备列表可显示CLI，真实审批可共享 | 同机不等于不同账户都有Key权限 |
| L | CLI使用既定SSH/Host协议/授权/断线恢复 | 不另造远程执行器或终端镜像架构 |
| M/N/O | SwiftUI同账户settings/会话撤销/Memory状态，授权iOS处理同Host请求 | 账户会话不代Host授权；真正iOS测试按既定GitHub分工 |
| P | TUI/exec真实整合、命令/按键、Registry切换、Provider/Profile/Key、安全交互、配置/Remote、threads/tasks、浏览器/Device登录、账户会话、本地记忆/统一sync、审批/QueueSteer、MCP/Skills/Plugins、JSONL/退出/诊断/升级 | CLI-01～34逐项有适当层级证据；集成方式验证固定TUI/exec客户端边界，不建第二Agent |
| Q | CLI Relay沿Noise/Snow Host配对/授权/撤销 | Account Token不代Relay或Host身份 |
| R | WindowsTerminal、LinuxSSH、无TTY、真实Host、多设备/认证服务、升级恢复和权限验收 | 未运行的真实层级不能借单测/合成协议成功通过 |

完整阶段顺序仍见V3。H/I稳定底层契约，P完整终端整合，R收集真实平台证据；文档任务后回到HANDOFF的F/G步骤，不立即编写上述功能。

## 15. CLI 专项验收矩阵

**以下34项统一状态：待实现 / 未执行。** 本次文档/源码/help检查不算其中任意项通过。后续每项记录精确commit、CLI/Runtime/Host/API/schema版本、OS/终端、配置、步骤、脱敏日志/抓包断言、结果及限制。

层级：U＝单元测试；O＝固定Runtime+合成服务的离线集成；H＝实际持久Host；A＝真实认证服务测试环境；D＝Windows/iOS/CLI实际多设备；T＝实际终端/无TTY。O不能代替Live模型、H、A、D或T。夹具使用隔离OS数据目录、合成Key和A/B测试账户、H1本地/H2 SSH、私有PA/PB、网络故障/延迟/重放注入与两个模型协议；真实服务另用授权测试账户，禁止用户既有秘密/生产数据/未授权付费模型。

| 编号 | 阶段 / 层级 | 可执行操作 | 通过断言与证据 |
| --- | --- | --- | --- |
| CLI-01 | P/R；U/T | Windows/Linux取help、补全及命令树，尝试register/signup | 文案全英文、实际命令准确、无注册入口；保留当前credentials/help版本证据，不以旧3项测试代验 |
| CLI-02 | H/P；O/H/T | 启动TUI，提交工具任务，追踪Host/Runtime PID与Thread/Turn | 唯一固定Runtime负责执行，TUI只是客户端；无第二Agent/执行器/HTTP框架 |
| CLI-03 | F/G/P；U/O/H | 当前Host装两个同名模型及Unknown/catalog/fixture报告，查看models与Key状态 | 完整限定ID/Host/Profile/三态/证据时间可见，catalog不冒充Full，秘密不显示 |
| CLI-04 | P；O/H/T | 活动Turn中选择已验同Provider模型，继续下一轮，另线程并行 | 当前Turn实际模型不变，下一轮确认后切换；无能力组合拒绝，其他线程不受默认改变 |
| CLI-05 | P；O/H | A模型线程切不同Provider B，含opaque历史/工具结果 | 新线程/关联分支、父归属明确，验证过历史才承接，活动Turn不无损迁移 |
| CLI-06 | E/P；U/T | 掩码输入与stdin添加/替换/删除fixture Key，故意参数泄漏/坏输入 | Broker同路径、无明文argv/回显/history/log/export；坏输入保留旧Key，store故障不降级 |
| CLI-07 | P；U/O/H | 两Profile同Provider，自定义Endpoint后discover/test，改到未授权地址 | Profile隔离，网络只用目标执行端授权Key；变更地址不发送旧Key，测试不伪称完整Runtime兼容 |
| CLI-08 | J/K/P；H/D | 同Windows用户GUI/CLI读取允许配置，另账户/Host尝试同引用 | 同机授权共享正确，其他Host/账户不能读Key/私有配置，配置profile不混Credential Profile |
| CLI-09 | H/L/P；H/T | local→SSH H2、remote list/add/test/use/attach，再切回 | 当前目标/授权/项目清楚，连接使用既定协议，切换不迁活动Thread |
| CLI-10 | L/P；H | H2选中后断网再提交，监测本地Runtime/工具marker | 明确Host离线/结果未知，本地无静默执行/重复工具 |
| CLI-11 | H/P；O/H/T | New/List/Resume/Fork/Detach，重连并补seq/快照 | 真ID/父关系/状态/cwd/worktree，缺口可恢复，detach不取消Host任务 |
| CLI-12 | P；O/H/T | 活动轮次Tab Queue/Enter Steer/Esc Interrupt，查看队列与终态 | 与实际Runtime结果一致，Stop/退出/后台终端task cancel不混淆 |
| CLI-13 | H/P；O/H/T | 可用批准/拒绝/取消请求，伪ID、过期ID、超范围session/policy回复 | 只显示真实availableDecisions，绑定有效请求/HostACL；拒绝无被拒副作用，未支持选项拒绝 |
| CLI-14 | P；O/T | 无TTY普通exec请求越sandbox工具、request_user_input、MCP elicitation；另测AutoReview配置 | 普通Never与实际解析例外准确；无无人回应prompt/伪输入/越权，elicitation Cancel与其他不支持错误区分 |
| CLI-15 | P/R；U/O/T | exec成功/失败/interrupt/timeout/未知提交，stdout/stderr与两种JSONL解析，output文件错误 | 终态/退出码契约、raw上游schema与caidex-v1不混；无ANSI/秘密污染，工具item完成不当turn成功 |
| CLI-16 | H/O/P；H/D | task submit后杀CLI/断SSH，由获权GUI/iOS attach/审批/查看完成 | 先持久受理、返回ID，submit0不当最终成功；共享真实任务，不额外启动Agent |
| CLI-17 | I/P/R；U/A/T | Windows browser PKCE成功；注入错state/redirect/verifier/issuer/audience/code重放/并发回调 | loopback-only且监听及时关闭，S256/归属/一次性校验有效，密码只网页，失败不留认证成功状态 |
| CLI-18 | I/P/R；A/T | 无GUI Linux VPS login默认Device，以手机浏览器已有账户确认 | HTTPS代码流程、轮询和独立CLI session成功，无旧设备限制/注册入口，核验后显示sync权威状态 |
| CLI-19 | I/P；U/A/T | 注入pending/slow_down/超时/拒绝/取消/重放，暴猜user_code与钓鱼请求提示 | 规定间隔/退避/终止、一次消费与客户端绑定、限流生效，token/device_code不在终端/log |
| CLI-20 | I/P；H/A/T | 无账户调用本地配置Codex/记忆；无账户尝试云访问/注册 | 既有权限本地可用，云拒绝，无register/signup及隐式开户 |
| CLI-21 | I/P；U/A/T | Windows/Linux安全store及显式文件，轮换并发/旧refresh重放/会话撤销再请求 | 分namespace、文件权限/明文告知、digest/family撤销，Access尚未过期也拒绝，凭据不进config/log |
| CLI-22 | I/J/M/P；A/D | CLI browser/device两会话，在GUI已登录设备列表单独退出CLI | CLI类型/时间/当前session显示正确，指定会话后续云拒绝，另一会话/Host任务不误取消 |
| CLI-23 | I/J/P；A/D | Windows先开启账户sync并确认范围，随后CLI新装登录 | CLI继承Enabled/版本/epoch，不重置为本机默认；历史上传仍需独立许可 |
| CLI-24 | I/N/P；A/D | iOS关闭账户sync，在线/离线CLI恢复状态，重新登录 | CLI继承Disabled，离线显示stale/pending不假称服务器已变；Chat同步独立 |
| CLI-25 | I/P；U/A/D | 关闭确认后旧CLI携旧版本上传/下载，旧memory job提交 | 服务端原子拒绝全账户后续记忆载荷/结果，不能只依靠客户端flag |
| CLI-26 | I/P；A/D | Enabled账户首次CLI登录，本机匿名/其他账户/未授权旧记忆与新授权记忆并存 | 旧数据不默传、不默改归属；明确范围后只所选同步，新授权数据可增量同步 |
| CLI-27 | I/P；U/O/H/T | 不登录/sync关闭，自动本地记忆、编辑删导出，切A/B模型，去掉Embedding | SQLite本地可用、跨模型共享授权内容、基础检索回退；Provider外发独立授权 |
| CLI-28 | I/P；U/A/D | off A后查保留状态，off B重认证/选择本地保留；旧设备/旧来源/新ID重新上线 | A不宣称删除，B正文/向量删除与epoch/tombstone生效，不误删Chat/journal、不复活云数据/假擦除离线副本 |
| CLI-29 | I/P；U/H/A/T | 同OS用户游客→A→B→重启，检查SQLite/附件/索引/outbox/缓存与Key引用 | B看不到A资料，游客不自动上传，退出未同步选择明确，无静默丢失或移交 |
| CLI-30 | H/I/P；H/A/D | 仅CAIdex Account Token尝试Host Shell/文件/Key/审批及Relay | 全部独立执行授权检查，Account/Gateway/SSH/Relay凭据不可互换 |
| CLI-31 | P；O/H/T | 当前Host MCP调用/征询、Skill引用、plugin管理，切Host/撤销权限 | 功能真实来自执行Host，权限/experimental门控保持，不靠目录/菜单标Full |
| CLI-32 | H/P/R；O/H/D | 断线/重启/超时、两个授权端审批竞态、工具结果未知、seq缺口 | 首有效、迟到失效、无timeout自动批准、恢复不盲重跑，明确blocked/unknown与实际终态 |
| CLI-33 | H/I/P/R；U/H/A/T | 旧配置/SQLite迁移，坏schema/满盘/回滚失败，旧Host/client协议，选择导入Codex | 源文件/未同步队列/墓碑保护，旧版本安全拒绝，秘密不自动复制/上传，pin不自动升级 |
| CLI-34 | P/R；T/H/A/D | Windows11 Terminal、Linux SSH/VPS、无TTY CI实际全链路；resize/ANSI/nocolor/paste/中文路径/signals | 各平台实运行报告，keymap上下文正确，headless无prompt死等、恢复准确；无真实覆盖的层级保留未验 |

复核补充断言仍归以上原编号，不算新增通过：CLI-16/32在task受理和恢复时核对真实policy/reviewer/sandbox，exec不会被改成跨端人工等待；CLI-17/21注入安全store写失败，确认无半写入成功状态，新增会话撤销失败明确pending，撤销重试秘密不进普通outbox；CLI-24/26/32在离线关闭后重新登录Enabled账户及CAS冲突，确认本机仍停传、旧队列不自动恢复、用户意图不被默默覆盖。

CLI-15/34还须用Windows/Linux真实管道验证UTF-8/BOM、UTF-16LE/BE BOM、非法编码，以及根exec“无prompt/显式 `-` 的空stdin拒绝”和“已有prompt的空stdin忽略/非空stdin追加”三种路径；另核对resume/fork/review各自入口。不以源码阅读代替运行通过，不改变CLI-01～34编号和当前未执行状态。

## 16. 实施时待验证的技术细节

不重新决定已经确定的公开客户端PKCE/Device Code、只登录、账户默认sync关闭/自动继承、执行端Key或唯一Runtime。以下为同一行为的实施选项：

- H/P验证固定TUI已有remote/共享daemon可复用程度、Host facade事件/请求适配及退出detach改动；固定exec当前使用in-process app-server并关闭客户端，不能未经验证就认为原进程可直接管理持久Host。选择最小适配/上游补丁，保持实际Runtime执行，禁止另造Agent。
- I选择成熟OAuth/OIDC库、官方issuer/client IDs/redirect注册、loopback平台保护、Device码策略/限流与token参数；刷新并发/丢失回应恢复、重新认证和撤销必须真实测试，不内置client secret。
- I/P验证安全存储的会话原子保存、部分写入清理及撤销待确认恢复；仅复用现有backend，不将普通outbox当秘密存储。已有Enabled账户重登录时保留pending关闭意图的CAS处理，也须真实认证/多设备故障测试。
- H/I/P验证Host聚合qualified model ID/Gateway route与Profile绑定、共享配置加载/trust、Windows路径、多进程SQLite锁/账户分区与迁移、可选本地加密边界。
- H验证任务状态/阻塞期限、跨重启审批幂等与Runtime不可恢复请求；本地退出/信号既不假称任务停止，也不能吞掉用户明确的interrupt。
- P核对完整原生命令/参数/Slash兼容表、选项门控、默认TTY键及剪贴板/信号/颜色；保留exec raw JSONL，CAIdex envelope/schema与退出例外用真实脚本测试固定。
- I/P/R实测CLI会话在GUI设备表、跨端settings/epoch、首次上传归属、A/B删除、长期离线与账户切换；现有本机备份、EmailSender可替换（Brevo优先/Resend备用）、资源/邮件网络/公开运营要求仍依账户设计，不新增服务或容量承诺。
- P/R确定Windows/Linux包安装与更新渠道、签名/完整性、补全、协议兼容窗口/回滚保护和实际终端覆盖；测试环境认证可访问性与多设备条件不具备时明确阻塞。

这些事项是待实现阶段验证，不是本次运行结果。后续首先按 [HANDOFF](../HANDOFF.md) 继续F/G；不能因设计文档存在提前把H/I/P/R标完成。
