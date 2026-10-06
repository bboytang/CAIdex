# CAIdex 项目交接

更新：2026-10-06。开始工作先读本文件，再检查 Git；按下一步继续，不重新规划已确认的方向。

## 当前任务

- 阶段 A 基础准备与 C 基础 CI 已完成；B 的元数据/stdio 验证通过。下一任务是 D：完整 Runtime facade 与上游功能对照。客户端 UI 尚未开始。
- 用户已明确允许完整项目以及原始方案、HANDOFF、UI 规范、架构/实施计划公开到 `bboytang/CAIdex`，后续按此授权续接。

## 已完成

- 固化 Windows/iOS UI 规范：尽量 1:1 参考官方，缺失部分自行补齐；CAIdex 功能融入原有位置，API Key 在设置中配置。
- 建立本文件及项目级 `AGENTS.md` 续接规则。
- 本地 Git 初始化成功，分支 `main` 跟踪 `origin/main`；首批实现 `edb946f`，已上传集成提交 `eebc0cc`。确认 Rust 1.99.0、Node 22.23.3、npm 10.9.9、Python 3.12.3。
- 上游 `rust-v0.160.1` 已核对：tag object `c3e23d4c4385619ecec78408766e46b7fa7dd9ad`，源码 commit `d27764b82f7118f674371e6d6e76271d9d606edb`。
- V3 计划已落盘；建立 Rust workspace、锁依赖、stdio 双向适配、开发阶段 `caidex doctor` 与 9 项协议测试。
- doctor 在隔离临时目录验证真实 app-server 初始化、线程创建/列表与事件，不启动模型轮次。
- 公共仓库 `https://github.com/bboytang/CAIdex` 已上传完整工程；连接器确认 push/admin 权限，origin 已设置。
- 三平台基础 CI 实际通过；action SHA 固定，Windows 使用 npm 中的原生二进制。仅更新交接文档不会重复触发 push CI。
- 将用户原始 V2 方案与四份品牌原件归档到项目内；V2 仅作历史参考，已确认调整优先。
- 本环境 `codex-cli 0.160.1`；`openaiDeveloperDocs` MCP 已配置且 enabled，本轮会话实际完成官方文档搜索/读取。本机 CLI 配置不随项目迁移。

## 未完成

- 尚无 iOS 应用工程；macOS 基础 Rust CI 不等于 iOS 构建。
- Runtime 仅实现底层 stdio 信封，完整 thread/turn facade、真实执行/审批、持久化恢复、生产 Host 尚未实现。
- Gateway/提供商/凭据、Windows/iOS UI、完整 CLI attach、Chat 同步、SSH/Relay 未实现；未配置或验证模型 API Key。

## 下一步顺序

1. 读取本文件和 Git 状态，按 D 补齐 Runtime facade、thread/turn/审批/用户输入与能力协商；先检查固定版本 schema，保留完整上游功能清单，避免直接将原始 Rust 类型暴露给 UI。
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
- `Cargo.toml`、`Cargo.lock`、`rust-toolchain.toml`：Rust workspace 与锁定工具链。
- `runtime/bridge/`、`apps/cli/`：stdio 适配、协议测试与离线 doctor。
- `upstream/codex/`：锁定文件、生成 schema 与上游 LICENSE/NOTICE。
- `.github/workflows/ci.yml`、`scripts/codex-binary.mjs`：三平台基础 CI 与原生二进制定位。
- `assets/brand/`：最新 dark 1024 应用图标、Windows ICO、透明 symbol 与完整 Logo 原件。
- branch：`main` 跟踪 `origin/main`；代码验证基准为 `eebc0cc`。本轮工程与交接收尾均提交，无保留的未完成工作区修改；续接时仍以 Git status 核对。构建缓存已忽略。

## 测试 / 验证

- Git 读取、暂存、提交和完整源码 push 成功；此前文档授权/工作流 scope 问题已按上述方式解决。
- 本机 `gh auth status` 成功，活动账户已启用；`gh api user` 返回 `bboytang`，权限响应包含 `workflow`。此次仅核验权限并更新交接，无代码或工作流改动。
- `codex --version`、`codex mcp list`：成功返回版本及 enabled 配置；官方 Docs 搜索/读取工具可用。CLI 有无法创建 PATH aliases 的只读文件系统警告。
- 文档检查通过：8 类必需信息齐全；新建文档格式正常，已检查新增文件差异。原方案及四份品牌资产的 SHA-256 与附件原件完全一致；原有 UI 规范未修改。
- `cargo test --workspace --locked`：9 项协议测试通过；覆盖乱序、未知字段、审批转交/回应、错误数据、退出、超时、取消、溢出与坏消息。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo fmt --all -- --check`：通过。
- `cargo run -p caidex-cli --locked -- doctor`：真实 Codex 0.160.1/Linux 输出 status=ok，模型轮次未启动。生成 schema 指纹与锁定文件一致。
- [CI run 37516548646](https://github.com/bboytang/CAIdex/actions/runs/37516548646)，基准 `eebc0cc`：ubuntu-24.04、windows-2022、macos-15 全部 success；各平台格式、Clippy、9 项协议测试、真实离线 doctor 均通过。
- 初始仅登记工作流的提交 `d437a58` 无完整工程，其 run 37516452005 为 failure；已由上述完整集成提交的成功运行取代，不能将登记提交用于验收。
- iOS 构建、真实模型/工具执行、真机测试：尚未执行。
