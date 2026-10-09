# CAIdex 开发交接

更新：2026-10-09，会话结束交接。**当前开发暂停，等待用户在新会话要求继续。** 正式项目仅 `/root/projects/CAIdex-v1.0`；废弃 `/root/projects/CAIdex` 不修改、不作为架构依据。

## 当前阶段与恢复点

当前处于 **F/G 多模型与 Gateway**，不是 CLI 编码阶段。Qwen Runtime/history 控制组合源码已提交、推送，并完成精确三平台离线验收；**Qwen Lite Adapter 尚未开始实现**。上一轮被用户中断时，只读取了源码和交接检查脚本，没有编辑源码、启动测试、提交或推送。CLI 正式文档任务已完成，不重复设计或开展账户/CLI/UI编码。

新会话先读本文件、[AGENTS.md](AGENTS.md)，检查 `git status --short --branch`、`git log -3 --oneline`，再按下方步骤继续。历史 HANDOFF 已逐字保存到[历史记录](docs/CAIdex-HANDOFF-历史记录-2026-10-09.md)；其中旧失败、旧“下一步”和旧 Git 状态只代表当时，不覆盖本文件当前恢复点。

## 正式依据与相关文件

- [实施计划 V3](docs/CAIdex-实施计划-V3.md)：A–R 阶段与验收门槛；原 V2 仅用于需求背景，不覆盖新决定。
- [账户/记忆/云设计](docs/CAIdex-Account-Memory-Cloud-设计与验收-V1.md)、[CLI完整规范](docs/CAIdex-CLI-完整交互与验收规范-V1.md)、[UI规范](docs/CAIdex-UI-规范-V1.md)、[凭据设计](docs/CAIdex-Credentials-设计与验收.md)：已确定产品行为，尚未实现部分明确待验。
- [Qwen验收](docs/CAIdex-Qwen-Provider-设计与验收.md)、[Gateway设计](docs/CAIdex-Model-Gateway-设计与验收.md)、[Runtime能力对照](docs/CAIdex-Runtime-能力对照.md)：当前技术契约、证据与限制。其他 Provider 的细节在各自 `docs/CAIdex-*-Provider-设计与验收.md`。
- 下一步源码：`model/providers/qwen/src/{lib,request,tools,history,history_stream}.rs`、`model/providers/qwen/tests/provider.rs`；已验 Lite 参考为 `model/providers/deepseek/src/{lib,request,tools,history}.rs` 与 `tests/lite/mod.rs`，不能盲目复制原生推理/历史契约。
- 共享实现：`model/core`、`model/providers/custom`、`model/gateway`、`credentials/core`；实际固定 Runtime 测试位于 `runtime/bridge/tests/real_runtime.rs` 与 fixtures。锁定信息在 `upstream/codex/lock.json`，品牌原件在 `assets/brand`。

## 已完成及实际证据

- A–E 当前范围：固定上游/协议/工程与三平台CI、真实app-server facade、请求/审批/Queue/Steer/Goal/MCP/PTY离线回归，以及执行端Broker/系统存储/受保护文件/环境引用/脱敏。生产多端Host权限不属于这些已验范围。
- F/G 共享模型核心、Registry门槛、Custom传输及本地Responses Gateway已有离线验证；OpenAI原生适配、Anthropic/Gemini/Ollama适配及固定Classic/Lite Runtime离线接线已有各自三平台证据。商业模型兼容性不由合成fixture证明。
- DeepSeek Adapter 57项已验；实际Classic/Lite Runtime新增7项涉及真实审批、隔离执行、取消及磁盘恢复，源码 `1b501ba` / [CI37886226991](https://github.com/bboytang/CAIdex/actions/runs/37886226991)三平台通过，Runtime50/49/49。生产Host与商业Live/Full未验。
- Qwen基础14项→上下文/effort21项→summary历史32项→function/namespace/v2历史43项→custom映射/v3历史54项均已分别三平台验收。custom源码 `ae0b2d0` / [CI37939652724](https://github.com/bboytang/CAIdex/actions/runs/37939652724)；不重做已验适配。
- 最新Qwen源码 **`0858493baf30917f0f536c1f70f474507d39e4ae`**：双runtime_context/native_history显式消费summary auto/context all_turns/include；source先预算、effort只映射一次；统一验证v1/v2/v3 carrier的native effort内部契约。新增8项，总62项。默认/部分策略仍拒绝，完整summary/JSON载体不宣传为精简、加密或来源认证。
- 中断前Qwen60通过/2失败已修正，不是当前阻塞：流夹具误把投影summary事件当native reasoning_text事件；NativeHistory缺内部reasoning契约校验。有效RED7/1后最小修复，最终定向62/0/0；workspace500/0/52，Clippy全workspace/all-targets `-D warnings`、fmt/diff通过。旧492通过名+8新名精确保持。

最新精确源码 [CI37974303497](https://github.com/bboytang/CAIdex/actions/runs/37974303497) 已 completed/success，本次交接再次只读核对GitHub三job终态，并重验保存的完整日志与源码绑定：

| 平台 / job | workspace 通过/失败/忽略 | 既有固定Runtime 通过/失败/忽略 |
| --- | --- | --- |
| Linux / 113968552358 | 500/0/52 | 50/0/0 |
| Windows / 113968552578 | 495/0/50 | 49/0/0 |
| macOS / 113968552644 | 499/0/50 | 49/0/0 |

每平台Qwen62、DeepSeek57逐名一次；完整日志2108/1793/1804行，全通过名551/544/548精确为旧CI+8，无遗漏/重复。每job17steps成功或条件跳过。**这些Runtime回归不是实际Qwen Runtime接线，Rust macOS CI不是iOS应用构建。**

CLI最终设计已归档；最新文档提交 `c766d93f35acb1a4fafa60118834ae42b23e18df` 已push。固定上游11份源码及隔离版本/5项help共6项核对通过；CLI-01～34、账户55项、UI16项仍待实现/未执行。当前 `apps/cli` 只有doctor、credentials status/set/remove、version/help。

## 未完成与下一步顺序

1. **先收尾三份已有F/G文档修改**：README、Qwen/Gateway验收文档只是记录0858493/CI37974303497已取得结果；核对diff、链接、源码未改后按既有授权独立提交/push，不为纯文档启动完整Rust CI。原HANDOFF的全部收尾内容已收入历史记录，当前恢复事实在本文件。
2. **Qwen Lite Adapter**：先核对固定Runtime真实Lite wire（additional_tools/developer、稳定ID、parallel=false、summary/context/include）与Qwen原生契约，再沿现有ToolMap/NativeHistory/Custom传输实现显式Lite适配、本地单调用交付及历史策略绑定。当前lib/routes与request仍Classic-only，ToolMap/history只支持v1/v2/v3；新政策/版本须明确验证，不能松绑旧历史或把本地拒绝多调用当原生生成约束。尚无Lite源码草稿或新测试。
3. Lite适配验证后，接**实际固定Qwen Classic/Lite Runtime**：真实审批、隔离执行、工具结果、取消、重启与磁盘恢复不重跑未知工具；离线fixture先验，商业Live/Full另需明确授权。不要借既有50/49/49回归代验新接线。
4. Qwen之后OpenRouter，继续F/G未完范围；满足V3门槛后再 **H → I → J/K Windows → L SSH → M/N/O iOS → P CLI → Q Relay → R**。不因账户/CLI文档存在跳过Provider或提前标完成。
5. H：持久Host/SQLite journal/事件恢复/审批竞争/幂等；I：官方账户/PostgreSQL、独立Chat/Memory/同步/邮件/本机恢复共享核心；P：完整英文TUI/exec/task/登录/记忆/Remote整合；R：真实平台、认证、多端、迁移与性能。Windows/iOS客户端、生产Host/Remote/Relay、账户服务、Memory Engine、同步及完整CLI均尚未实现。

每个重要步骤：先检查→最小实现→定向及相关回归→diff→更新HANDOFF→按授权提交/push与精确源码三平台CI；旧CI只证明对应源码，不冒充新功能通过。

## 已确定的架构边界（不得擅自改动）

- 唯一Agent/工具执行器/审批真源：Codex **0.160.1 / d27764b82f7118f674371e6d6e76271d9d606edb**。复用Runtime facade、ModelProvider、Gateway、Credential Broker及Custom传输，不另造执行器、审批引擎或模型HTTP栈；未知wire保留，Classic/Lite分别验。
- Windows11x64 Tauri2+React/TypeScript/Rust；Linuxx86_64 Host/CLI；iOS17+ SwiftUI/Swift5+UniFFI。GUI默认简中、英文备选，CLI全英文。用户品牌原件为准，布局尽量参考同平台官方，Key入口在设置；不新增Android/Web/macOS原生GUI。
- Registry目录/配置/协议fixture不授LiveRuntime/Full；native载体绑定执行端/profile/endpoint/model/完整前缀/工具政策。不能自动升级、跨Provider复用签名或放宽历史绑定；JSON载体不是加密/真实性证明。
- Host管理线程/任务/真实审批；退出GUI/CLI/SSH只断连接，不等于取消。journal先落盘再广播，快照/seq补缺口，审批首次有效，不盲重跑未知结果、不承诺外部exactly-once。切Host不迁活动线程；模型轮次边界切换，跨Provider关联分支/新线程。
- 普通Chat独立，没有Shell/Git/项目写权限；API Key归实际执行端，Remote不读Host已存Key，不进Git/日志/Chat/Memory/普通同步。Account Token、Gateway token、CredentialRef.owner、SSH/Relay/Host ACL各自独立；登录账户不授执行/Key读取/代审批权。
- 官方统一Account，不可变user_id；Windows/iOS应用内注册/密码/Passkey/恢复。CLI只登录已有账户：浏览器Authorization Code+PKCE、VPS/SSH Device Grant；公开客户端无长期secret。auth_sessions独立撤销/refresh轮换，不强制设备绑定、旧设备批准或默认设备数量限制。
- 正式云端账户/Chat/Memory使用PostgreSQL+pgvector/RLS与版本化迁移；Windows/iOS/Linux CLI本地缓存/记忆、Host journal继续SQLite且逻辑分离。不退回用户自建身份或云SQLite临时路径。未登录/云故障不破坏已有本地权限能力。
- Memory模型无关，个人/项目/Chat原文/执行事件/临时上下文分开，来源/revision可纠错导出，Git/源码/实际测试优先；记忆和外部文本不能提升Runtime权限。Chat/整合/Embedding角色独立，空间版本化，无模型可基础检索；整合在授权执行端复用ModelProvider，不复制Key到云后台。
- 自动记忆、Memory Sync、Chat同步、Provider外发权限独立。新账户Memory Sync默认关；三平台登录继承同一服务端enabled/settings_version/cloud_epoch，Enabled不授权上传匿名/其他账户/未许可旧资料。首次范围确认；离线关本机立即停传/待服务器确认，确认后全账户拒绝新memory上传/下载/旧job提交；A保留云/B重认证删云，以epoch/tombstone/来源屏障防复活；账户切换隔离本地资料。
- 云内测2核2GB50GB单实例，无GPU/强制Redis/Kubernetes；可替换EmailSender（Brevo优先/Resend备用），实际额度/中国网络送达另验。只本机有限备份/恢复与用户导出，无异地自动备份；整VPS/磁盘损坏可能数据库+备份全损，本地缓存不是全量云备份，不承诺容量或全故障恢复。
- Remote先SSH，Q沿Noise/Snow E2EE配对/撤销；不与账户登录设备混用。exec报告实际解析的无头审批策略（通常Never、特定AutoReview例外），不代答人工请求；持久task允许独立Host授权的跨端审批，submit成功不等于任务成功。
- iOS测试/最终构建在GitHub macOS真正simulator/无签名archive；真机/签名另验，无签名archive不是可安装IPA。当前三平台Rust CI不代GUI或iOS验收。

## 问题与阻塞

- 当前没有已知未修复测试失败或运行中的本轮编译/测试/CI；本次/proc只读检查未发现cargo/rustc/clippy/gh/caidex任务，仅会话基础进程。CI37974303497三job均终态。上轮中断不留下已确认存活的任务，不盲目重启旧handle。
- Qwen Lite、实际Qwen Runtime、OpenRouter等是未实现工作，不是阻塞。Qwen原生输入自动截断风险待真实模型验证；本地预算不能替代token/context能力。商业模型/签名真实性/Full、生产权限、多端Host、Windows桌面/UAC、iOS真机/签名未验。
- 既有Minor/覆盖空缺保留在Provider文档：Gemini thought-call/无tools/none、整组载体互换、projection/满槽取消、整数/空ID、usage/thought-only等；Anthropic重启第三轮/完整Lite custom结果等。不是本轮已修复项，不派重复审查或无关重构。
- 同步SecretStore调用开始后不能强停，只保证取消后不POST；comment-only native chunk/实际Runtime下游idle单独未验。jsonschema0.58.6离线retriever虽有字节/regex限制但无硬CPU抢占，留H隔离；不自动重试/修补坏回答。

## Git、环境与操作授权

- 分支 **main**，交接检查基线HEAD=origin/main=`c766d93f35acb1a4fafa60118834ae42b23e18df`。源码与已验0858493一致，无未提交Rust/TS/Swift、依赖或workflow修改，无暂存或未跟踪源码。
- 交接前未提交4文档：HANDOFF、README、Qwen/Gateway验收。**本次仅将HANDOFF更新与其完整历史归档独立提交**，标题`docs: hand off CAIdex session at Qwen Lite boundary`；最终提交SHA以`git log -1`为准，避免自引用提交号。交接提交后仍保留README、Qwen/Gateway三份原修改，逐字不改、不纳入此次提交；新会话先核对实际Git状态再收尾它们。
- 用户2026-10-08持续授权本地检查通过后直接commit/push、源码三平台CI，不再重复询问；纯文档不运行完整Rust CI。离线合成fixture/临时marker已授权；未授权读取用户Key、商业API/邮件/生产部署/模型下载或购买服务。
- `.git`普通沙箱只读，Git写入/push需授权执行环境。push使用 `git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main`，不改全局凭据或输出秘密。
- loopback测试用授权环境，`TMPDIR=/var/tmp`；保留`/tmp/.git`、target/测试成果/用户数据，不用reset --hard、clean -fdx或清缓存。Rust1.99.0、Codex0.160.1、Node22.23.3按锁定；本机缺平台依赖交给CI。磁盘剩余约7.6GB（交接时），实际修改前重查df。

## 交接检查与证据位置

本次只复核项目文档/相关源码、Git、已有完整日志与GitHubCI状态，整理恢复文件；不启动新开发、不重跑Rust编译或CI。交接检查通过：17份Markdown/79本地链接/22锚点、git diff --check、旧HANDOFF归档逐字及SHA256一致；除HANDOFF之外198个既有tracked文件未变（包括三份未提交F/G文档）。提交仅HANDOFF与历史归档，推送后核对HEAD/origin及剩余三文档状态，最终结果在 `/tmp/caidex-session-handoff-oct09/`。临时日志只是补充，跨机器续接应依本文件、项目docs、固定源码与公开CI链接，不能要求原会话或/tmp仍存在。

本机补充：`/tmp/caidex-qwen-history-controls/{local-result,ci-result}.json`及workspace-final/clippy-final/resume-green.log；`/tmp/caidex-ci-37974303497-{linux,windows,macos}-raw.log`及status.json；CLI只读核对在`/tmp/caidex-cli-formal-audit-final/`。全部重要结论、SHA、CI编号和下一步已保存在项目文件中。
