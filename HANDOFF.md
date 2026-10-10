# CAIdex 开发交接

更新：2026-10-10，OpenRouter Classic平面function工具精确三平台验收完成；下一步namespace/custom及完整政策绑定历史。 正式项目仅 `/root/projects/CAIdex-v1.0`；废弃 `/root/projects/CAIdex` 不修改、不作为架构依据。

## 当前阶段与恢复点

当前处于 **F/G 多模型与 Gateway**。OpenRouter基础Classic Adapter已完成契约核对→实现/14项定向→完整本地→推送/精确三平台CI四步，每步已更新本文件。源码`9563df09607a71da409466ba3d0c69b9a09c3ca6`/[CI38004040230](https://github.com/bboytang/CAIdex/actions/runs/38004040230)已验，workspace524/519/523、既有Runtime57/56/56，旧通过名精确+14。

本步 **OpenRouter显式Runtime上下文/neutral text与逐route effort** 四步骤均已完成并逐步更新交接：契约核对→最小实现/9项新增定向→完整本地→推送/精确源码CI。源码`92d0612a50bd23dd13c0d69e2536636e7c6320b1`/[CI38005373396](https://github.com/bboytang/CAIdex/actions/runs/38005373396)已验，Provider23、workspace533/528/532、既有Runtime57/56/56，全通过名精确旧CI+9。

本步 **OpenRouter逐route正文控制** 四步骤已完成并逐步更新交接：契约核对→显式verbosity指导/tier映射及8项新测试→完整本地→推送/精确源码CI。源码`2799a4cbe75371528955ca8d24efe5660cd54429`/[CI38006780427](https://github.com/bboytang/CAIdex/actions/runs/38006780427)已验，Provider31、workspace541/536/540、既有Runtime57/56/56，所有通过名精确旧CI+8；不是原生工具或历史回放验收。

本轮 **OpenRouter Classic平面function工具** 已完成契约核对→最小实现/15项新增→完整本地→精确源码三平台CI，各步均更新交接。逐route显式with_native_tools须先配backend；原生声明/auto-none-required-named选择/原顺序成对文本结果，JSON与SSE调用身份/参数/终态门控、旧工具call_id/item ID复用拒绝；SSE预算内缓冲模型事件到终态，Heartbeat继续传递，取消/deadline/Drop与满槽释放已验，不执行工具。opaque reasoning/未知输出扩展及大整数ID保留，但其输入回放仍关闭；Unknown不升级，Unsupported显式拒绝。旧36项测试正文逐字保留，Provider51/0/0、本地workspace561/0/59、既有固定Runtime57/0/0及Clippy/fmt/diff通过；全通过名精确旧workspace546+15、旧Runtime57完全保持，依赖/共享生产源码/Runtime/其他Provider/workflow不变。源码`65001cd0952685d635d61e4d6d682e174928eb01`/[CI38010128508](https://github.com/bboytang/CAIdex/actions/runs/38010128508)三平台已验，完整证据见末节。

准确恢复点：继续原生工具namespace/custom适配及完整政策绑定历史。平面function配对是明文校验，不能冒充签名历史或原生call未知扩展的无损输入回放；summary/context/include、reasoning输入、namespace/custom/allowed_tools/deferred/server工具及Lite仍拒绝。后续载体须绑定执行端/profile/endpoint、显式backend政策、model、完整compiled前缀/instructions及verbosity/tier/effort/工具政策，保留原生reasoning/signature和未知扩展；base slug可覆盖多个region/variant，metadata显示名不是精确endpoint证明，不能用slug/tier冒充稳定后端。不照搬Qwen summary-only或DeepSeek明文规则。再Lite→实际Classic/Lite Runtime审批/执行/取消/磁盘恢复；商业调用未授权，不跳H/I/CLI/UI。工具route的文本显示延迟到终态；实际Runtime/下游长流体验未验，shortcut注释保留该限制。

新会话先读本文件、[AGENTS.md](AGENTS.md)，检查 `git status --short --branch`、`git log -3 --oneline`，再按下方步骤继续。历史 HANDOFF 已逐字保存到[历史记录](docs/CAIdex-HANDOFF-历史记录-2026-10-09.md)；其中旧失败、旧“下一步”和旧 Git 状态只代表当时，不覆盖本文件当前恢复点。

## 正式依据与相关文件

- [实施计划 V3](docs/CAIdex-实施计划-V3.md)：A–R 阶段与验收门槛；原 V2 仅用于需求背景，不覆盖新决定。
- [账户/记忆/云设计](docs/CAIdex-Account-Memory-Cloud-设计与验收-V1.md)、[CLI完整规范](docs/CAIdex-CLI-完整交互与验收规范-V1.md)、[UI规范](docs/CAIdex-UI-规范-V1.md)、[凭据设计](docs/CAIdex-Credentials-设计与验收.md)：已确定产品行为，尚未实现部分明确待验。
- [Qwen验收](docs/CAIdex-Qwen-Provider-设计与验收.md)、[Gateway设计](docs/CAIdex-Model-Gateway-设计与验收.md)、[Runtime能力对照](docs/CAIdex-Runtime-能力对照.md)：当前技术契约、证据与限制。其他 Provider 的细节在各自 `docs/CAIdex-*-Provider-设计与验收.md`。
- 已验Qwen接线：`runtime/bridge/tests/real_runtime.rs`、`runtime/bridge/tests/fixtures/responses_server.py`、独立Qwen Runtime用例与catalog fixture、`runtime/bridge/Cargo.toml`；已验参考为`runtime/bridge/tests/deepseek/mod.rs`。Qwen适配源码及`model/providers/qwen/tests/lite/mod.rs`已验，Qwen仅summary契约，不能复制DeepSeek reasoning_text/content原生输出。
- 本步OpenRouter：`model/providers/openrouter/src/lib.rs`、`src/request.rs`、`src/tools.rs`、`tests/provider.rs`及`tests/tools/mod.rs`；下步仍在该crate内原生工具/后端绑定历史，不修改Runtime执行器。
- 共享实现：`model/core`、`model/providers/custom`、`model/gateway`、`credentials/core`；实际固定 Runtime 测试位于 `runtime/bridge/tests/real_runtime.rs` 与 fixtures。锁定信息在 `upstream/codex/lock.json`，品牌原件在 `assets/brand`。

## 已完成及实际证据

- A–E 当前范围：固定上游/协议/工程与三平台CI、真实app-server facade、请求/审批/Queue/Steer/Goal/MCP/PTY离线回归，以及执行端Broker/系统存储/受保护文件/环境引用/脱敏。生产多端Host权限不属于这些已验范围。
- F/G 共享模型核心、Registry门槛、Custom传输及本地Responses Gateway已有离线验证；OpenAI原生适配、Anthropic/Gemini/Ollama适配及固定Classic/Lite Runtime离线接线已有各自三平台证据。商业模型兼容性不由合成fixture证明。
- DeepSeek Adapter 57项已验；实际Classic/Lite Runtime新增7项涉及真实审批、隔离执行、取消及磁盘恢复，源码 `1b501ba` / [CI37886226991](https://github.com/bboytang/CAIdex/actions/runs/37886226991)三平台通过，Runtime50/49/49。生产Host与商业Live/Full未验。
- Qwen基础14项→上下文/effort21项→summary历史32项→function/namespace/v2历史43项→custom映射/v3历史54项均已分别三平台验收。custom源码 `ae0b2d0` / [CI37939652724](https://github.com/bboytang/CAIdex/actions/runs/37939652724)；不重做已验适配。
- 上一已验控制组合源码 **`0858493baf30917f0f536c1f70f474507d39e4ae`**：双runtime_context/native_history显式消费summary auto/context all_turns/include；source先预算、effort只映射一次；统一验证v1/v2/v3 carrier的native effort内部契约。新增8项，总62项。默认/部分策略仍拒绝，完整summary/JSON载体不宣传为精简、加密或来源认证。
- 中断前Qwen60通过/2失败已修正，不是当前阻塞：流夹具误把投影summary事件当native reasoning_text事件；NativeHistory缺内部reasoning契约校验。有效RED7/1后最小修复，最终定向62/0/0；workspace500/0/52，Clippy全workspace/all-targets `-D warnings`、fmt/diff通过。旧492通过名+8新名精确保持。

控制组合精确源码 [CI37974303497](https://github.com/bboytang/CAIdex/actions/runs/37974303497) 已 completed/success，本次交接再次只读核对GitHub三job终态，并重验保存的完整日志与源码绑定：

| 平台 / job | workspace 通过/失败/忽略 | 既有固定Runtime 通过/失败/忽略 |
| --- | --- | --- |
| Linux / 113968552358 | 500/0/52 | 50/0/0 |
| Windows / 113968552578 | 495/0/50 | 49/0/0 |
| macOS / 113968552644 | 499/0/50 | 49/0/0 |

每平台Qwen62、DeepSeek57逐名一次；完整日志2108/1793/1804行，全通过名551/544/548精确为旧CI+8，无遗漏/重复。每job17steps成功或条件跳过。**这些Runtime回归不是实际Qwen Runtime接线，Rust macOS CI不是iOS应用构建。**

CLI最终设计已归档；最新文档提交 `c766d93f35acb1a4fafa60118834ae42b23e18df` 已push。固定上游11份源码及隔离版本/5项help共6项核对通过；CLI-01～34、账户55项、UI16项仍待实现/未执行。当前 `apps/cli` 只有doctor、credentials status/set/remove、version/help。

## 本会话Qwen Lite验收

- 显式`with_lite_options`按route保留Classic/Lite元数据，native传输始终Classic；默认入口不放宽。消费首项developer additional_tools并绑定稳定ID，Lite developer message有效ID保留在native输入/完整前缀；复用custom→function指导和既有工具选择/结果配对，不改Runtime执行器。
- Lite parallel=false是本地最多交付一个调用，JSON/SSE终态多调用502且不提前交付；缺省/true仍绑定Lite政策。v4绑定完整scope/endpoint/model/前缀/原工具/format/稳定ID；不混用旧v1/v2/v3或Classic，非加密/来源认证。summary/context/include仍须runtime_context+history显式消费，effort一次映射。
- 新增8项/Qwen70；本地workspace508/0/52、既有固定Runtime50/0/0、Clippy全workspace/all-targets-D warnings、fmt/diff通过。旧62项测试正文逐字保留，全workspace旧500+8精确保持；仅四份Qwen生产源码/lib/request/tools/history及测试变化，共享Core/Gateway/Custom/Broker/Runtime/依赖/workflow未改。
- 精确CI37978406850 completed/success，Linux113982424514/Windows113982424527/macOS113982424369各17steps成功或条件跳过。完整日志2115/1801/1812行；workspace508/503/507（failed0，ignored52/50/50）、既有Runtime50/49/49（failed/ignored0），Qwen70和DeepSeek57每名一次；全通过名559/552/556精确旧CI+8，无遗漏/重复。watch和三份下载exit0，ci-check通过。这不是实际Qwen Runtime接线或商业Live/Full。

## 已验Qwen实际Runtime证据

精确源码`bf94c9d3041b75806470ae06ca4b658d2966228b`/[CI37982550340](https://github.com/bboytang/CAIdex/actions/runs/37982550340)整体completed/success。Linux113996415936/Windows113996416140/macOS113996416351各17steps成功或条件跳过；完整raw2132/1818/1829行，workspace510/505/509（failed0，ignored59/57/57）、实际固定Runtime57/56/56（failed/ignored0），Qwen72、DeepSeek57及新增Qwen Runtime7每名每平台一次。全通过名568/561/565精确为旧CI37978406850集合+2 Provider+7 Runtime，无遗漏/重复。watch及三份完整日志下载exit0，ci-check通过；完整日志`/tmp/caidex-ci-37982550340-{linux,windows,macos}-raw.log`及status.json，checker/result在`/tmp/caidex-qwen-runtime/`。本节证明固定Runtime离线链路，不授商业Live/Full、生产Host或iOS应用验收。

首轮1/6及次轮6/1均已定位修复：显式runtime_context仅接受typed数组developer/user有效ID，默认/状态/历史绑定不放宽；Lite native文本拼接与Runtime磁盘数组分别精确核对。一项旧Lite负例改user为system，新Provider2项覆盖合法ID与坏ID/状态/前缀拒绝。无新外部依赖、Runtime生产实现或执行器改动。

## 未完成与下一步顺序

1. **三份F/G文档已收尾**：README、Qwen/Gateway验收文档核对79本地链接/22锚点、源码与0858493一致、完整旧CI日志逐名检查和GitHub源码/成功终态通过；纯文档提交`165d905`已推送，不启动Rust CI。
2. **Qwen Lite Adapter已收尾**：固定Runtime真实wire及Qwen官方契约已核对，显式Lite/v4策略、70项测试与3ec8ca7/CI37978406850精确三平台已验；保留证据，不重复已验适配。
3. **实际固定Qwen Classic/Lite Runtime已精确三平台收尾**：真实审批、隔离执行、工具结果、取消、重启与磁盘恢复不重跑未知工具；离线fixture先验，商业Live/Full另需明确授权。不要借既有50/49/49回归代验新接线。
4. **OpenRouter基础/上下文/effort/正文/backend路由与平面function已精确三平台收尾，再namespace/custom/完整政策绑定历史/Lite/实际Runtime**，继续F/G未完范围；满足V3门槛后再 **H → I → J/K Windows → L SSH → M/N/O iOS → P CLI → Q Relay → R**。不因账户/CLI文档存在跳过Provider或提前标完成。
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

- Qwen旧基准本地72项Provider/7项定向Runtime、workspace510/0/59、完整Runtime57/0/0及Clippy/fmt/diff已通过；日志`/tmp/caidex-qwen-runtime/`，local-check.py/local-result.json精确核对旧通过名+2/+7和修改边界。精确源码bf94c9d/CI37982550340已completed/success，watch与三份完整raw下载exit0、ci-check通过，所有本步句柄已结束。
- 实际Qwen Runtime离线三平台已验；OpenRouter基础Adapter14项、workspace524/0/59、既有Runtime57/0/0及Clippy/fmt/diff本地通过，源码9563df0/CI38004040230已精确三平台completed/success。首次发布拒绝已因本会话OpenRouter明确授权解除；所有测试/watch/下载句柄已结束，无当前已知失败。OpenRouter平面function已精确三平台验收；namespace/custom/后端绑定历史/Lite/实际Runtime未实现，商业Live/Full/生产Host/iOS应用未验；Qwen自动截断风险仍待真实模型验证。
- 既有Minor/覆盖空缺保留在Provider文档：Gemini thought-call/无tools/none、整组载体互换、projection/满槽取消、整数/空ID、usage/thought-only等；Anthropic重启第三轮/完整Lite custom结果等。不是本轮已修复项，不派重复审查或无关重构。
- 同步SecretStore调用开始后不能强停，只保证取消后不POST；comment-only native chunk/实际Runtime下游idle单独未验。jsonschema0.58.6离线retriever虽有字节/regex限制但无硬CPU抢占，留H隔离；不自动重试/修补坏回答。

## Git、环境与操作授权

- 分支 **main**，本轮源码`65001cd0952685d635d61e4d6d682e174928eb01`已提交/push且精确CI38010128508已验；收尾前HEAD=origin/main为该SHA。仅HANDOFF/README/OpenRouter/Gateway四文档更新验收，源码保持精确CI版本；纯文档提交号依git log -1，不重复Rust CI。无依赖/共享生产源码/Runtime/其他Provider/workflow改动。
- 新会话用户已明确授权本次核验通过的文档与Qwen Lite源码提交推送main及源码三平台CI；首次推送曾因自动审批不认可旧交接授权被拒，取得本会话明确授权后推送成功。
- 用户2026-10-08持续授权本地检查通过后直接commit/push、源码三平台CI，不再重复询问；纯文档不运行完整Rust CI。离线合成fixture/临时marker已授权；未授权读取用户Key、商业API/邮件/生产部署/模型下载或购买服务。
- `.git`普通沙箱只读，Git写入/push需授权执行环境。push使用 `git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main`，不改全局凭据或输出秘密。
- loopback测试用授权环境，`TMPDIR=/var/tmp`；保留`/tmp/.git`、target/测试成果/用户数据，不用reset --hard、clean -fdx或清缓存。Rust1.99.0、Codex0.160.1、Node22.23.3按锁定；本机缺平台依赖交给CI。磁盘剩余约5.8GB（交接时），实际修改前重查df。

## 交接检查与证据位置

上一会话仅复核与归档；该次交接检查通过：17份Markdown/79本地链接/22锚点、git diff --check、旧HANDOFF归档逐字及SHA256一致；除HANDOFF之外198个既有tracked文件未变（包括三份未提交F/G文档）。提交仅HANDOFF与历史归档，推送后核对HEAD/origin及剩余三文档状态，最终结果在 `/tmp/caidex-session-handoff-oct09/`。临时日志只是补充，跨机器续接应依本文件、项目docs、固定源码与公开CI链接，不能要求原会话或/tmp仍存在。

本机补充：`/tmp/caidex-qwen-history-controls/{local-result,ci-result}.json`及workspace-final/clippy-final/resume-green.log；`/tmp/caidex-ci-37974303497-{linux,windows,macos}-raw.log`及status.json；CLI只读核对在`/tmp/caidex-cli-formal-audit-final/`。全部重要结论、SHA、CI编号和下一步已保存在项目文件中。

本会话补充日志：`/tmp/caidex-qwen-lite/{workspace-final,clippy-final,runtime-regression,ci-watch}.log`、local-result.json/ci-result.json；完整CI日志为`/tmp/caidex-ci-37978406850-{linux,windows,macos}-raw.log`与status.json。源码/CI/数量/边界已写入仓库文档，续接不依赖/tmp。

本步补充：`/tmp/caidex-qwen-runtime/{provider-final,focused-final,workspace-final,clippy-final,runtime-final}.log`及local-check.py/local-result.json；独立Qwen Runtime用例与fixtures在runtime/bridge/tests，ID策略测试在model/providers/qwen/tests/runtime_ids。

本步收尾核对：17份Markdown/84本地链接/22锚点、git diff --check通过；收尾仅上述五份文档，源码逐字保持bf94c9d/CI37982550340已验内容。纯文档不重复Rust CI，提交推送后再核对HEAD/origin与工作区。

OpenRouter步骤1依据：[Responses无状态](https://openrouter.ai/docs/api_reference/responses/overview)、[基础请求](https://openrouter.ai/docs/api_reference/responses/basic-usage)、[目录](https://openrouter.ai/docs/api/api-reference/models/list-all-models-and-their-properties)、[路由](https://openrouter.ai/docs/guides/routing/provider-selection)、[Responses请求参考](https://openrouter.ai/docs/api/api-reference/responses/create-responses)。目录完整raw保留，公开metadata只返回配置交集/Unknown；不调用商业API、不读用户Key。

OpenRouter步骤2：新增model/providers/openrouter/{src/config,src/catalog,src/request,src/lib}.rs及tests/provider.rs、Cargo workspace成员及lock内部package；无新外部包或共享生产源码/Runtime/workflow改动。恢复文件见[OpenRouter验收](docs/CAIdex-OpenRouter-Provider-设计与验收.md)；本地检查已结束，日志`/tmp/caidex-openrouter-basic/`。

OpenRouter步骤3本地证据：`/tmp/caidex-openrouter-basic/{focused-final,workspace-final,clippy-final,runtime-final}.log`与local-check.py/local-result.json；workspace旧510+14、Runtime旧57逐名完全保持，外部package/version集合不变。

OpenRouter步骤4已完成：首次发布组合命令被自动审批拒绝，理由为此前明确发布授权只覆盖Qwen Lite；命令未执行。保存本地9563df0后，用户明确答复“授权OpenRouter推送main并运行CI”，按本次授权推送成功并核验精确CI，拒绝已解除，不是当前阻塞。

OpenRouter本地收尾：18份Markdown/87本地链接/22锚点及git diff --check通过，完整本地记录已保存；本地源码提交后仅HANDOFF状态更新，源码保持9563df0。三平台CI已验；不认领实际OpenRouter Runtime/商业Full。

本会话明确授权补充：用户已授权OpenRouter本地提交9563df0（含源码/Cargo/验收文档）推送bboytang/CAIdex main并运行源码三平台CI；不重复询问本次发布。

精确源码`9563df09607a71da409466ba3d0c69b9a09c3ca6`/[CI38004040230](https://github.com/bboytang/CAIdex/actions/runs/38004040230)整体completed/success。Linux114068611307/Windows114068611369/macOS114068611031各17steps成功或条件跳过；完整raw2166/1852/1863行，workspace524/519/523（failed0，ignored59/57/57）、既有固定Runtime57/56/56（failed/ignored0），OpenRouter14、Qwen72、DeepSeek57每名每平台一次。全通过名582/575/579精确为旧CI37982550340集合+14，无遗漏/重复；既有Runtime通过名完全保持。watch及三份完整日志下载exit0，ci-check通过；日志`/tmp/caidex-ci-38004040230-{linux,windows,macos}-raw.log`及status.json，checker/result在`/tmp/caidex-openrouter-basic/`。这些Runtime回归不代验实际OpenRouter接线或商业Live/Full，也不代验iOS应用。

OpenRouter收尾核对：18份Markdown/87本地链接/22锚点、git diff --check及修改边界通过；仅四文档变化，源码保持精确CI版本。提交推送后核对HEAD/origin及工作区，续接不依赖/tmp。

## OpenRouter上下文/neutral text/effort四步完成

1. 契约核对：恢复时main=origin=8fa8b2c且干净；固定Runtime client build_reasoning/Responses请求与实际Classic合成wire已检查，官方Responses reasoning与reasoning-tokens已核对。Runtime身份/缓存归因仅本地消费，不推断model slug稳定后端。
2. 最小实现：仅OpenRouter src/lib.rs/src/request.rs及tests/provider.rs；显式runtime_context消费三个已允许header、client_metadata/prompt_cache_key/neutral text，typed developer/user合法ID保留且不允许status。逐route effort显式映射一次，缺配置/明确Unsupported门控，none例外不升级能力。summary/context/include、非neutral verbosity/原生推理输入/工具历史仍拒绝。9项新增/23项定向通过；API未实现时编译失败属预期，普通沙箱socket PermissionDenied后授权TMPDIR=/var/tmp全部通过，无已知代码失败。
3. 完整本地：workspace533/0/59、既有固定Runtime57/0/0、全workspace/all-targets Clippy -D warnings、fmt/diff通过；local-check.py核对旧524通过名精确+9、旧Runtime57完全保持。旧14项测试逐字保留、Cargo.lock逐字不变、共享/其他Provider/Runtime/workflow未改。18份Markdown/90本地链接/22锚点通过。
4. 提交推送与三平台：按本会话用户明确OpenRouter main/CI授权推送；完整证据如下，所有本地/watch/下载句柄已结束。仅纯文档收尾，源码保持已验版本；后续恢复无需/tmp。

精确源码`92d0612a50bd23dd13c0d69e2536636e7c6320b1`/[CI38005373396](https://github.com/bboytang/CAIdex/actions/runs/38005373396)整体completed/success。Linux114072825747/Windows114072825607/macOS114072825701各17steps成功或条件跳过；完整raw2175/1861/1872行，workspace533/528/532（failed0，ignored59/57/57）、既有固定Runtime57/56/56（failed/ignored0），OpenRouter23、Qwen72、DeepSeek57每名每平台一次。全通过名591/584/588精确为旧CI38004040230集合+9，无遗漏/重复；既有Runtime通过名完全保持。watch及三份完整日志下载exit0，ci-check通过；日志`/tmp/caidex-ci-38005373396-{linux,windows,macos}-raw.log`及status.json，checker/result在`/tmp/caidex-openrouter-context/`。这些Runtime回归不代验实际OpenRouter接线或商业Live/Full，也不代验iOS应用。

源码提交后的收尾只改四文档；18份Markdown/89本地链接/22锚点、diff及源码逐字保持精确CI版本检查通过，纯文档提交号依git log -1。推送后核对HEAD/origin与工作区；续接时仍按开头检查真实状态。继续下一步如顶部恢复点，不重做基础适配，不认领商业Full或实际OpenRouter Runtime。

## OpenRouter逐route正文控制四步完成

1. 契约核对：恢复时main=origin=18c7a05且干净；固定Runtime verbosity/text及tier构造与Qwen/Anthropic/Gemini显式政策模式已检查，OpenRouter Responses参考/Service Tiers已核对。直接Responses页面404、官方搜索缓存可读；实时Service Tiers明确支持Responses，并说明请求政策不保证实际端点/SLA/价格。
2. 最小实现：仅OpenRouter lib/request新增逐route with_verbosity_instruction和with_service_tier_mapping。原instructions保留，追加一次显式指导；不是原生verbosity等级或结构输出。tier只映射一次，auto/default组、flex同值、priority/fast别名组、ultrafast同值，未知/重复/缺映射/跨档本地降级/Null拒绝，JSON/SSE实际tier不覆盖。源与compiled预算均Key/POST前校验，identity/结构输出/工具/summary/context/include/原生推理历史仍默认关闭。新增8项/定向31通过，旧23测试正文逐字保留。首轮30/1仅SSE夹具漏stream=true，补齐后31/0/0，无当前已知失败。
3. 完整本地：workspace541/0/59、既有固定Runtime57/0/0、全workspace/all-targets Clippy -D warnings、fmt/diff通过；local-check.py核对旧533通过名精确+8、旧Runtime57完全保持。Cargo.lock逐字不变，无共享/其他Provider/Runtime/workflow改动。18份Markdown/91本地链接/22锚点通过；日志/tmp/caidex-openrouter-body/，后续恢复不依赖/tmp。
4. 提交推送与三平台：按用户明确OpenRouter main/CI持续授权完成精确源码验收，完整证据如下；所有本地/watch/下载句柄已结束。剩余纯文档收尾保持源码不变，继续顶部下一步，不重做旧适配。

精确源码`2799a4cbe75371528955ca8d24efe5660cd54429`/[CI38006780427](https://github.com/bboytang/CAIdex/actions/runs/38006780427)整体completed/success。Linux114077274589/Windows114077274731/macOS114077274576各17steps成功或条件跳过；完整raw2183/1869/1880行，workspace541/536/540（failed0，ignored59/57/57）、既有固定Runtime57/56/56（failed/ignored0），OpenRouter31、Qwen72、DeepSeek57每名每平台一次。全通过名599/592/596精确为旧CI38005373396集合+8，无遗漏/重复；既有Runtime通过名完全保持。watch及三份完整日志下载exit0，ci-check通过；日志`/tmp/caidex-ci-38006780427-{linux,windows,macos}-raw.log`及status.json，checker/result在`/tmp/caidex-openrouter-body/`。这些Runtime回归不代验实际OpenRouter接线或商业Live/Full，也不代验iOS应用。

收尾仅四文档，18份Markdown/91本地链接/22锚点、diff与源码逐字保持精确CI版本检查通过；纯文档提交号依git log -1，推送后核对HEAD/origin与工作区，续接仍先检查真实状态。实际OpenRouter Runtime与商业Full仍未验，不借既有Runtime回归代验。


## OpenRouter显式后端路由前置步骤已验

精确源码`2c9954c80fd37950a188b9799ffc9c428b730fd4`/[CI38008446175](https://github.com/bboytang/CAIdex/actions/runs/38008446175)整体completed/success。linux job114082598633，workspace546/0/59、既有固定Runtime57/0/0，raw2188行/通过名604；windows job114082598878，workspace541/0/57、既有固定Runtime56/0/0，raw1874行/通过名597；macos job114082598892，workspace545/0/57、既有固定Runtime56/0/0，raw1885行/通过名601。各17steps成功或条件跳过，OpenRouter36/Qwen72/DeepSeek57每名每平台一次；全通过名精确旧CI38006780427+5，无遗漏/重复，旧Runtime名完全保持。watch和完整日志下载exit0，ci-check通过；/tmp/caidex-openrouter-backend/及/tmp/caidex-ci-38008446175-{linux,windows,macos}-raw.log只作补充，仓库证据足够跨机器恢复。这些既有Runtime回归不是实际OpenRouter接线或商业Live/Full。

收尾仅四文档，18份Markdown/91本地链接/22锚点、diff及源码保持精确CI版本检查通过；纯文档提交推送后核对HEAD/origin与工作区，提交号依git log -1。恢复时先读顶部，不重做已验路由政策；工具/绑定历史/Lite/实际OpenRouter Runtime仍待实施。


## OpenRouter Classic平面function工具已验

初期RequestContext非Clone及新测试编译问题已修正。首可执行47/1、补充50/1均为新取消断言499与共享503契约不符，统一后51/0/0。差异检查复现新call_id复用旧工具item ID，负例先失败，再最小门控修复；未知item整数ID另加保留夹具，最终51/0/0。Clippy初次只有新测试初始化写法问题已修正；最终workspace/Clippy/fmt/diff与边界检查通过，无当前已知失败，全部本地/watch/下载句柄结束。

精确源码`65001cd0952685d635d61e4d6d682e174928eb01`/[CI38010128508](https://github.com/bboytang/CAIdex/actions/runs/38010128508)整体completed/success。linux job114087959146，workspace561/0/59、既有固定Runtime57/0/0，raw2203行/通过名619；windows job114087959047，workspace556/0/57、既有固定Runtime56/0/0，raw1889行/通过名612；macos job114087959157，workspace560/0/57、既有固定Runtime56/0/0，raw1900行/通过名616。各17steps成功或条件跳过，OpenRouter51/Qwen72/DeepSeek57每名每平台一次；全通过名精确旧CI38008446175+15，无遗漏/重复，旧Runtime名完全保持。watch和完整日志下载exit0，ci-check通过；/tmp/caidex-openrouter-tools/及/tmp/caidex-ci-38010128508-{linux,windows,macos}-raw.log只作补充，仓库证据足够跨机器恢复。这些既有Runtime回归不是实际OpenRouter接线或商业Live/Full。macOS首次完整日志下载返回EOF，重试下载exit0并逐名完整核对通过；不是CI失败。

收尾仅四文档，18份Markdown/91本地链接/22锚点、diff及源码保持精确CI版本检查通过；纯文档提交推送后核对HEAD/origin与工作区，提交号依git log -1。恢复按顶部下一步，不重做已验平面function，不认领完整历史/Lite/实际OpenRouter Runtime。
