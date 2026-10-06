# CAIdex 项目交接

更新：2026-10-06。开始工作先读本文件，再检查 Git；按下一步继续，不重新规划已确认的方向。

## 当前任务

- 阶段 A 基础准备与 C 基础 CI 已完成；B 的元数据/stdio 验证通过。正在推进 D：完整 Runtime facade 与上游功能对照；已完成完整清单、facade、19 项协议回归及 5 项真实 Runtime/本地 Responses 集成；能力对照已落盘，三平台 CI 已通过；本轮已继续完成真实 patch accept/cancel，MCP form accept/decline/cancel 与 PTY/长运行已通过 Linux；正在完善能力对照并准备三平台 CI。客户端 UI 尚未开始。
- 用户已明确允许完整项目以及原始方案、HANDOFF、UI 规范、架构/实施计划公开到 `bboytang/CAIdex`，后续按此授权续接。

## 已完成

- 固化 Windows/iOS UI 规范：尽量 1:1 参考官方，缺失部分自行补齐；CAIdex 功能融入原有位置，API Key 在设置中配置。
- 建立本文件及项目级 `AGENTS.md` 续接规则。
- 本地 Git 初始化成功，分支 `main` 跟踪 `origin/main`；当前已验证工程基准 `7209e21`。确认 Rust 1.99.0、Node 22.23.3、npm 10.9.9、Python 3.12.3。
- 上游 `rust-v0.160.1` 已核对：tag object `c3e23d4c4385619ecec78408766e46b7fa7dd9ad`，源码 commit `d27764b82f7118f674371e6d6e76271d9d606edb`。
- V3 计划已落盘；建立 Rust workspace、锁依赖、stdio 双向适配、开发阶段 `caidex doctor` 与 9 项协议测试。
- doctor 在隔离临时目录验证真实 app-server 初始化、线程创建/列表/读取与事件，不启动模型轮次。
- 公共仓库 `https://github.com/bboytang/CAIdex` 已上传完整工程；连接器确认 push/admin 权限，origin 已设置。
- 三平台基础 CI 实际通过；action SHA 固定，Windows 使用 npm 中的原生二进制。仅更新交接文档不会重复触发 push CI。
- 将用户原始 V2 方案与四份品牌原件归档到项目内；V2 仅作历史参考，已确认调整优先。
- 本环境 `codex-cli 0.160.1`；`openaiDeveloperDocs` MCP 已配置且 enabled，此前会话实际完成官方文档搜索/读取，本轮通过官方网页核对协议。本机 CLI 配置不随项目迁移。
- D 第一步：已归档固定版本完整常规/实验 schema；分别包含 104/167 个客户端请求、10/11 个服务端请求、83 个通知。实验 schema 指纹已写入锁定文件；默认生成会遗漏实验方法，不能用它代表全部上游功能。
- D 第二/三步：19 项协议回归通过；真实 Runtime/本地 Responses 集成 5 项 Linux 通过（历史/resume/fork、Queue CRUD/审批 cancel、Linux accept 实际命令/工具结果、Plan 用户输入、Steer/interrupt/过期审批）。两份 schema 重新生成指纹通过。跨平台修复已验证：固定 bundle LF 避免 Windows checkout 改字节；本地测试服务跳过反向 DNS，macOS 启动恢复。最终 CI `37520407878` 三平台 success（Linux 5 项，Windows/macOS 4 项真实集成）。

- D 第四步：Linux 两项真实 apply_patch 回归通过；批准后仅写临时目标，取消后不写文件且轮次 interrupted（上游此时不保证 fileChange item/completed）。仅 Responses fixture，未调用商业模型。

- D 第五步：真实 MCP stdio fixture 经 Runtime 完成 handshake、工具/资源发现、资源读取、手动工具调用及 form elicitation→显式 accept；structuredContent/_meta 完整保留。Linux accept/decline/cancel 全部通过，跨平台待 CI。

- D 第六步：Linux 真实 PTY 输入/resize/UTF-8 字节/exit 通过；长运行进程重复 handle 拒绝、显式 kill、过期 stdin 拒绝通过。base64 仅新增为锁定 dev-dependency；生产接口未新增依赖。

## 未完成

- 尚无 iOS 应用工程；macOS 基础 Rust CI 不等于 iOS 构建。
- 完整 Runtime 能力验收仍有缺口（Queue start/reorder、Goal/compaction、其他工具场景等）；本轮 patch/MCP/PTY 尚待跨平台 CI，见能力对照；持久化恢复、生产 Host 尚未实现。
- Gateway/提供商/凭据、Windows/iOS UI、完整 CLI attach、Chat 同步、SSH/Relay 未实现；未配置或验证模型 API Key。

## 下一步顺序

1. 读取本文件和 Git 状态，先提交并验证当前 patch/MCP/PTY/长运行用例的三平台 CI；通过后按 `docs/CAIdex-Runtime-能力对照.md` 继续 D：Queue start/reorder、Goal/compaction 等协议与上游对照；能力未验证就保留待验收状态。
2. 使用可重复协议场景与上游对照验证；实际模型/工具执行与模拟测试分别记录。当前底层 transport 的断开/超时不等于持久 Host 恢复完成。
3. 推进 E/F 的凭据层、模型核心与 Responses Gateway；实际付费 API/凭据复用或创建前明确授权，离线验证继续使用测试服务。
4. 依阶段推进多模型、持久 Host、Chat/同步、Windows、SSH/iOS、CLI 整合、Relay；iOS 应用测试/无签名构建在 GitHub macOS runner。

## 重要架构决定

- 客户端：Windows Tauri 2 + React/TypeScript/Rust；iOS SwiftUI，初期 Swift 5 模式；共享 Rust 核心经 UniFFI 接入 iOS；CLI 继承上游英文体验。首批目标 Windows 11 x64、Linux x86_64 Host/CLI、iOS 17+。
- 上游：https://github.com/openai/codex 。使用真实 app-server/Runtime，固定 commit、最小补丁；运行状态和审批由 Runtime 提供，不自造第二套 Agent。接口通过验证后版本化，取消原 V2 的永久冻结要求。
- 普通 Chat 独立于 Codex Host，无 Shell/Git/项目写权限；按提供商实际能力展示搜索、图片、附件等入口，不宣称所有模型能力相同。
- Gateway 面向 Codex 的 Responses HTTP/SSE 协议；保留提供商不透明签名/推理数据，统一工具消息，Gateway 不执行工具。提供商分阶段适配与验收。
- 模型切换在明确的轮次边界生效；同提供商仅允许已验证兼容组合，跨提供商通过关联分支/新线程和适配后的历史承接。
- API Key 由对应执行端的凭据管理层保存；Remote 使用执行 Host 的 Key，手机不读取已保存的远程 Key；聊天历史同步不携带 Key。
- Host 独立运行，GUI/SSH 断开不决定任务生命周期；事件按 Host/stream 编号、持久化后广播，缺口通过快照恢复；提交幂等、有效审批首次处理生效，不承诺外部工具恰好执行一次。
- 固定 CLI 模型元数据存在两条路径：经典 Responses（如 bundled gpt-5.5，独立 freeform apply_patch）与 Responses Lite/code_mode_only（如 gpt-6.1-sol）。旧测试 gpt-5.1-codex 属未知元数据 fallback；不能由它外推所有模型工具能力。F/G 必须按真实 model/tool_mode/use_responses_lite 验证 Gateway；本轮 patch 只验证经典路径。
- Runtime 请求的 availableDecisions 决定 UI 审批选项；空闲 Queue add 可能启动轮次；turn 完成/中断也撤销线程待回应请求，不依赖逐项 resolved。固定源码和真实用例已核实。
- Remote 先 SSH，后 Relay；生产 Relay 从首版使用成熟方案的端到端加密。保留原生 Queue/Steer。切换 Host 不迁移运行中的线程。
- Chat 历史自动同步到自托管 VPS，SQLite 本地缓存/outbox/cursor；完成或中断轮次同步，冲突分支与删除 tombstone；HTTPS 服务端属于信任边界，当前不承诺历史同步端到端加密。
- iOS 先测试及无签名构建，尚未进入签名/TestFlight；无签名产物不等于可安装 IPA。其他客户端当前环境优先，环境不足的检查转 GitHub；真机/UAC 等验证单独记录。
- UI/品牌规则见 UI 规范；普通布局与缺失状态已授权自主判断。API Key 在“设置 → 模型与提供商”，本机与 Host 配置归属必须清楚。

## 问题 / 阻塞

- `.git` 对普通执行仍为只读挂载，已通过授权提权完成初始化；读状态可用，写 Git 元数据需使用相应权限，不删除目录。
- 文档公开授权已补齐。本机 GitHub CLI 2.45.0 已登录 `bboytang`，`gh api user` 验证成功，OAuth scope 包含 `repo`、`workflow`；后续可通过 gh 管理工作流。此前 Git credential helper 使用的凭据缺少 workflow scope，与当前 gh 凭据应区别记录；未修改 Git 凭据配置。插件写工作流再 fetch/合并的备用路径已验证。不要输出凭据。
- Windows、Swift/Xcode 不在本机；对应平台检查需 GitHub runner。环境无 OPENAI_API_KEY；当前只做离线协议验证，不使用付费模型或用户凭据。
- 无真实客户端逐状态截图和真机验证覆盖；现有 UI 资料足够建立基准，缺失状态按 UI 规范补齐并保留验收项。

## 相关文件与工作区修改

- `AGENTS.md`：开始工作及交接更新规则。
- `HANDOFF.md`：当前续接入口。
- `docs/CAIdex-UI-规范-V1.md`：已确认的布局、功能入口、状态、品牌与 UI 验收要求。
- `docs/CAIdex-原始方案-V2.md`：用户原文归档；其中永久冻结等内容已被上述调整替代。
- `docs/CAIdex-实施计划-V3.md`：已落盘执行顺序与验收条件。
- `docs/CAIdex-Runtime-能力对照.md`：V2 原生能力逐项验收、完整方法/请求/通知清单及 CLI 入口对照。
- `Cargo.toml`、`Cargo.lock`、`rust-toolchain.toml`：Rust workspace 与锁定工具链。
- `runtime/bridge/src/{protocol,runtime,stdio}.rs`：锁定方法清单、Runtime facade 与传输；`tests/{runtime,real_runtime,stdio}.rs` 及本地 fixtures：回归/真实 Runtime 场景。
- `apps/cli/`：通过 facade 运行的隔离离线 doctor。
- `upstream/codex/`：锁定文件、生成 schema 与上游 LICENSE/NOTICE。
- `.github/workflows/ci.yml`、`scripts/{codex-binary,verify-codex-schema}.mjs`：三平台 CI、原生二进制定位和 schema 指纹核验。
- `assets/brand/`：最新 dark 1024 应用图标、Windows ICO、透明 symbol 与完整 Logo 原件。
- branch：`main` 跟踪 `origin/main`；D 实现 `c15d447`、跨平台修复 `7209e21` 均已上传，代码验证基准 `7209e21`。本轮未提交：5 项新增真实 patch/MCP/PTY/进程回归、本地 Responses/MCP fixtures、base64 dev-dependency/Cargo.lock 与交接/能力对照；新增用例跨平台 CI 待执行。续接以 Git status 核对。构建缓存已忽略。

## 测试 / 验证

- Git 读取、暂存、提交和完整源码 push 成功；此前文档授权/工作流 scope 问题已按上述方式解决。
- 本机 `gh auth status` 成功，活动账户已启用；`gh api user` 返回 `bboytang`，权限响应包含 `workflow`。gh 凭据将用于此次包含工作流的发布；不输出凭据。
- `codex --version`、`codex mcp list`：成功返回版本及 enabled 配置；官方 Docs 搜索/读取工具可用。CLI 有无法创建 PATH aliases 的只读文件系统警告。
- 文档检查通过：8 类必需信息齐全；新建文档格式正常，已检查新增文件差异。原方案及四份品牌资产的 SHA-256 与附件原件完全一致；原有 UI 规范未修改。
- `cargo test --workspace --locked`：19 项协议回归通过（9 transport + 10 facade）；真实集成用例默认 ignored，CI 单独显式运行。
- `cargo test -p caidex-runtime --test real_runtime --locked -- --ignored`：Linux 10 项真实 Runtime/本地 Responses/MCP/进程用例通过；无商业模型/Key，工具执行和文件修改仅使用隔离临时测试目录。新增 Windows/macOS 结果待 CI。
- `node scripts/verify-codex-schema.mjs`：常规/实验固定 schema 重新生成及归档 SHA-256 均匹配。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo fmt --all -- --check`：通过。
- `cargo run -p caidex-cli --locked -- doctor`：真实 Codex 0.160.1/Linux 输出 status=ok，模型轮次未启动，新增 thread/read 检查。生成 schema 指纹与锁定文件一致。
- [CI run 37520407878](https://github.com/bboytang/CAIdex/actions/runs/37520407878)，基准 `7209e21`：ubuntu-24.04、windows-2022、macos-15 全部 success；各平台格式、Clippy、19 项协议测试、常规/实验 schema 指纹及离线 doctor 均通过；真实 Runtime/本地 Responses 集成 Linux 5 项、Windows/macOS 各 4 项通过。
- 初次新增 CI `37519900562` 的 Windows 换行指纹失败与 macOS 测试服务启动超时，已由 `7209e21` 修复并在上述新运行验证，不作为当前验收结果。
- iOS 构建、商业真实模型、其他 Provider、Windows 实际批准执行与真机测试：尚未执行。真实 Linux 工具链仅验证临时标记场景，不能外推所有工具。
