# CAIdex 开发交接

更新：2026-10-10。会话切换专用恢复点；Final Architecture First，继续既定 V3，不重新设计。正式目录 `/root/projects/CAIdex-v1.0`；废弃 `/root/projects/CAIdex` 不修改。

## 当前状态与第一步

交接整理前实际状态：branch `main`，HEAD=origin/main=`6d6b2cf3a58fe1401955ac72197f05b1cc8fe2c7`，工作区干净。该提交只收尾四份文档；最新源码为 `3a57bcbfb8f4aa942019c081404b6833daa57642`，三平台 CI 已完整核验成功。此次仅修改 HANDOFF，完成检查后提交/push；交接提交自身的 SHA 以 `git log -1` 为准，不能将上面的前置 SHA 当成最新 HEAD。

**没有未完成的代码、未提交功能、运行中的测试/CI、当前已知测试失败或待处理推送审批。** 上一任务“Lite模型切换新增2项与版本化离线报告”已经完成。下一任务是**核对 V3 F/G 剩余离线门槛和证据缺口**，尚未开始该审计或选定下一项实现；不要虚构已有审计结论或直接标记 F/G 完成。

新 Codex 顺序：

1. 先读本文件、[AGENTS](AGENTS.md)，运行 `git status --short --branch`、`git log -4 --oneline`、`git rev-parse HEAD origin/main`，核实恢复点和用户新改动。
2. 读[实施计划 V3](docs/CAIdex-实施计划-V3.md)、[Gateway验收](docs/CAIdex-Model-Gateway-设计与验收.md)、[模型切换报告V1](docs/CAIdex-模型切换-离线兼容性报告-V1.md)，对照 Registry/Router、Runtime切换测试及各Provider验收。
3. 将剩余 F/G 要求对应到“实现/测试证据/未验或待实现”，先找明确缺口，补最小的既定离线验收。重点核实每模型版本化报告、Unknown/Unsupported展示与拒绝、工具/推理及切换的证据范围；并非这些项目全部缺失，也不是任意组合都已支持。
4. 每重要步骤更新本文件。代码改动按定向→相关完整本地→最终diff→提交/push→精确源码三平台CI推进；CI必须核对实际SHA、3jobs/步骤、完整日志及测试名，不仅查看绿色状态。纯文档无需重复Rust CI。
5. 商业API/真实Key、生产Host持久关联/通用历史适配、客户端等另有边界。不能把这些缺口伪装成离线完成；满足V3前置门槛后按 H→I→J/K→L→M/N/O→P→Q→R，不据fixture跳H。

## 最新已验证证据

最新源码 `3a57bcbfb8f4aa942019c081404b6833daa57642` / [CI38072098372](https://github.com/bboytang/CAIdex/actions/runs/38072098372)：整体completed/success，每job17steps成功或条件跳过。

| 平台 / job | workspace 通过/失败/忽略 | 固定Runtime 通过/失败/忽略 | 完整raw行数 / 通过名数 |
| --- | --- | --- | --- |
| Linux / 114271430456 | 619/0/73 | 71/0/0 | 2296 / 691 |
| Windows / 114271430496 | 614/0/71 | 70/0/0 | 1982 / 684 |
| macOS / 114271430487 | 618/0/71 | 70/0/0 | 1993 / 688 |

完整通过名精确为旧CI38071015251+2，无遗漏/重复；OpenRouter100、Qwen72、DeepSeek57及新增Lite2每名每平台一次，旧Runtime保持。workspace的ignored包含真实Runtime测试，已由固定Runtime步骤显式执行；不能把ignored记为通过。watch及三份完整日志下载exit0。

本地：workspace619/0/73、固定Runtime71/0/0（旧69+2）、Clippy workspace/all-targets-D warnings、fmt/diff通过；19份Markdown/110本地链接/22锚点和报告JSON检查通过。源码阶段221个其他tracked文件、旧Runtime/Classic/cross_provider测试正文逐字保持；四文档收尾224个其他tracked文件保持该精确CI源码。依赖、生产源码、fixture及workflow在Lite切换阶段未改。

### 上一任务具体实现

- Harness显式模式 `gateway-model-switching-lite`，复用OpenAI Adapter/ModelRouter、既有wire-lite fixture。公共route `gpt-6.1-sol` / `gpt-6-sol`选择固定Runtime Lite协议，native模型是合成 `native-switch-0/1`；不是商业模型调用。
- `switching::lite::lite_model_switch_disk_resume_and_fork_keep_dialect_and_parent_history`：完成轮次后切换，后续轮次及disk restart保持模型/历史；fork返回父关联且创建不推理，父子模型/历史隔离；验证Lite header、additional_tools前缀和稳定ID。
- `switching::lite::lite_unknown_model_rejects_before_credentials_and_explicit_recovery_keeps_lite`：未知模型Key/POST零访问，显式恢复保留Lite协议。
- 首定向0/2仅新断言误写developer.additional_tools；实际wire是独立 `type=additional_tools` / `id=at_...` / `tools` 数组，修正后2/0/0。没有因此修改生产策略，无当前已知失败。
- [离线报告V1](docs/CAIdex-模型切换-离线兼容性报告-V1.md)：Classic两route `gpt-5.5/gpt-5.1-codex`，Lite两route如上；明确native fixture、方言和 `testedModelVersion`。只认领 ProtocolFixture / Experimental，不自动挂到Registry，不升级Unknown/空报告/Full，不赋予真实模型限额或能力。

### 最近前置证据，避免重复实现

| 已完成范围 | 精确源码 / CI | workspace Linux/Windows/macOS | 固定Runtime Linux/Windows/macOS |
| --- | --- | --- | --- |
| 跨Provider显式可见文本交接/源reasoning拒绝2项 | b5e7fca6b620eb45e0689eca5a8c2eee36c93562 / [38071015251](https://github.com/bboytang/CAIdex/actions/runs/38071015251) | 619/614/618 | 69/68/68 |
| 同Provider Classic轮次切换/模型fork/未知模型3项 | 430305d5a0ae323b9d739bc6db05ae2dfc30c3bf / [38069700504](https://github.com/bboytang/CAIdex/actions/runs/38069700504) | 619/614/618 | 67/66/66 |
| ModelRouter Core7/Gateway2，注册/目录/六方法/报告绑定 | e2f3039c6c4f82898f2a30246e6bd1981ffec671 / [38063184946](https://github.com/bboytang/CAIdex/actions/runs/38063184946) | 619/614/618 | 64/63/63 |
| OpenRouter实际固定Classic/Lite Runtime7项 | 0e2cb32834af889a5d9b343cb867c7cb5e0d40af / [38057298047](https://github.com/bboytang/CAIdex/actions/runs/38057298047) | 610/605/609 | 64/63/63 |

以上全为完整日志逐名核验的三平台成功证据，各17steps成功或条件跳过，failed0；旧回归保持。ModelRouter实际生产实现提交c0fffad673717366610144d2d0d8e940ffff681b，e2f3039为对应授权交接/CI HEAD，源码相同。

- ModelRouter：显式公共ID→Adapter绑定，Registry结构校验，目录每Adapter一次、过滤归属并排序；元数据/能力/报告版本漂移拒绝，无默认fallback/retry、凭据读取或跨Provider签名转换。Gateway复用既有单Provider注入口，不另造HTTP栈。Unknown/Unsupported门控与上下文/取消/Drop/错误retry-after原样传递已有测试。
- Classic只验证该明确同Provider/同方言组合。Lite同理；Classic↔Lite互切、其他组合、活动轮次切换、完整工具状态跨模型切换、商业模型升级并未由上述测试证明。
- 跨Provider正例：真实OpenAI源thread/read显式选可见assistant文本和来源ID，进入独立OpenRouter新线程；真实CommandApproval前无marker，Accept后只在隔离临时目录执行，disk restart不重复。源模型/历史/Key/POST保持，目标不含源opaque/签名/未选输入；来源ID是可见标注，**不是生产Host持久关联**。
- 跨Provider负例：从源实际rollout取reasoning直接提交目标Gateway，HTTP400且目标Key/POST零访问。这是Gateway/Adapter拒绝边界，不能说已实现Runtime原始history迁移。
- 稳定thread/start没有原始history入口；实验thread/resume.history标注 `FOR CODEX CLOUD - DO NOT USE`，未使用，不能用它代替生产关联/历史适配。
- OpenRouter100项Provider：显式backend政策、原生function/namespace/custom、完整JSON/SSE/签名/未知扩展载体、summary/context/include/effort逐route政策、Lite additional_tools和单调用门控、取消/预算/隔离。固定Runtime7项真实审批/临时执行、cancel/interrupt/迟到审批、partial/multi拒绝、重启磁盘完整恢复且不重复执行。默认/部分策略仍Key/POST前拒绝。详情见Provider文档，不复制Qwen别名或扁平化原生结果。
- Qwen72、DeepSeek57及其实际Classic/Lite Runtime各7项已三平台离线验证；Anthropic/Gemini/Ollama/OpenAI/Custom已有各自实现/离线证据，以各Provider文档的实际限制为准，不据这段概述宣称全功能或商业Full。

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

## 未完成、限制与阻塞

- F/G尚未整体完成：商业真实模型版本/兼容性/限额/性能与授权API验证待执行；版本化fixture报告不代表商业Full。Registry校验报告结构及来源范围，不认证报告真实性；公开目录/配置不自动赋予支持能力。
- 生产Host线程持久关联、轮次边界约束、通用历史适配尚未实现。H持久Host/SQLite journal/竞争审批/幂等恢复尚未实现；不要把测试Harness当生产Host。
- I账户/PostgreSQL/Memory/同步/邮件及J–O客户端、L SSH、P完整CLI、Q Relay、R真实平台/多端/性能验收待实现。`apps/cli`当前只有doctor、credentials status/set/remove、version/help；CLI-01～34、账户55项、UI16项仍待实现/未执行，文档归档不等于功能完成。
- 各Provider文档中的既有覆盖空缺仍需核实：Gemini thought-call/无tools/none、整组载体互换、projection/满槽取消、整数/空ID、usage/thought-only；Anthropic重启第三轮/完整Lite custom结果等。没有在此次Lite任务修复这些项目，也未确认每项都仍缺失；接手应对实际当前代码/测试核实，避免重复或无关重构。
- native载体非加密/来源认证，backend配置不是实际服务端endpoint证明；文本终态缓冲、完整前缀二次增长及预算限制保留。context须执行端显式model/backend支持声明，不按slug猜测。Qwen自动截断风险待真实模型验收。
- 同步SecretStore开始后不能强停，仅保证取消后不POST；Runtime下游idle单独未验。jsonschema0.58.6离线retriever有字节/regex限制但无硬CPU抢占，H隔离待验。不自动重试坏结果/未知操作。
- 当前无活动阻塞。此前main推送自动审批拒绝已因用户明确“授权OpenRouter推送main并运行CI”及“你重新推送，我给你授权”解除；后续ModelRouter/切换及文档推送成功，不能把旧拒绝记作当前阻塞。

## 代码与设计入口

- 核心：`model/core/src/{registry,router,provider,responses,stream,sse}.rs`，`model/core/tests/router.rs`；Gateway `model/gateway/src/`及HTTP测试。
- Runtime：`runtime/bridge/src/`；Harness和实际Runtime主文件 `runtime/bridge/tests/real_runtime.rs`；切换 `runtime/bridge/tests/switching/{mod,lite,cross_provider}.rs`；Provider Runtime模块 `runtime/bridge/tests/{openrouter,qwen,deepseek}/`；fixtures位于 `runtime/bridge/tests/fixtures/`。
- Adapter：`model/providers/{openai,custom,anthropic,google,ollama,deepseek,qwen,openrouter}/`；Broker `credentials/core/`；开发CLI `apps/cli/`。
- 最终基准：[V3计划](docs/CAIdex-实施计划-V3.md)、[UI规范](docs/CAIdex-UI-规范-V1.md)、[账户/记忆/云设计](docs/CAIdex-Account-Memory-Cloud-设计与验收-V1.md)、[CLI规范](docs/CAIdex-CLI-完整交互与验收规范-V1.md)、[Runtime能力对照](docs/CAIdex-Runtime-能力对照.md)、[凭据验收](docs/CAIdex-Credentials-设计与验收.md)。[原始V2](docs/CAIdex-原始方案-V2.md)仅保留需求背景，冲突按V3及已确认新设计执行。
- Provider专属设计/验收文件位于 `docs/CAIdex-*-Provider-设计与验收.md`；固定上游锁 `upstream/codex/lock.json`；CI `.github/workflows/ci.yml`。

## 环境、复验与操作授权

- repo origin `https://github.com/bboytang/CAIdex.git`，branch main。Git与公开CI可重新读取，临时日志不是交接必要条件。
- 锁定Rust1.99.0（rust-toolchain.toml）、Codex0.160.1/固定commit；Node先按环境核实。本机2026-10-10磁盘剩余3.5GB，执行前重查 `df -h .`，不主动清缓存/用户数据。
- 本地loopback/固定Runtime测试使用允许启动本地进程/socket的环境及 `TMPDIR=/var/tmp`；常规沙箱限制时按平台审批机制执行。保留 `/tmp/.git`、target、已有测试成果，不做reset --hard/clean -fdx、历史重写或全局凭据修改。
- 已有用户持续授权本地验证后commit/push main和源码三平台CI；纯文档不跑完整Rust CI。自动审批实际拒绝时解释具体原因，不能绕过。未授权读取用户Key、调用商业API/邮件、生产部署、模型下载或购买服务；授权离线合成fixture/隔离临时marker不扩大到这些操作。
- 推送沿已验证命令：`git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main`。不输出任何秘密，推送后核对HEAD/origin及工作区。

需要源码复验时，从项目根目录执行（不需因换会话重跑已验无修改源码）：

```bash
cargo fmt --all -- --check
TMPDIR=/var/tmp cargo clippy --workspace --all-targets --locked -- -D warnings
TMPDIR=/var/tmp cargo test --workspace --locked
# 脚本只解析本地已安装固定二进制；缺失时先核实CI安装步骤，不替换为任意版本。
CAIDEX_CODEX_BIN="$(node scripts/codex-binary.mjs)" TMPDIR=/var/tmp cargo test -p caidex-runtime --test real_runtime --locked -- --ignored
# 定向Lite切换：上一命令在 --ignored 后追加 switching::lite::
git diff --check
```

schema/doctor依CI使用固定二进制：`scripts/verify-codex-schema.mjs`、`cargo run -p caidex-cli --locked -- doctor`。原生SecretService单独步骤参见CI和 `scripts/test-linux-secret-service.sh`，不能把普通workspace ignored当作原生存储已实测。

本机补充证据：`/tmp/caidex-lite-switching/{focused-second,workspace,runtime,clippy,ci-watch}.log`及ci-check.py/ci-result.json；完整CI `/tmp/caidex-ci-38072098372-{linux,windows,macos}-raw.log`及status.json。此前阶段补充目录 `/tmp/caidex-{cross-provider,model-switching,model-router,openrouter-runtime}/`。丢失/tmp可用上表GitHub CI和仓库源码恢复；不要求上个Codex记忆或句柄存在。

## 本次交接核验

交接仅整理本文件，删除过时“下一步/未实现/旧Git”流水状态，保留当前实现、准确恢复点、关键证据/限制/架构/授权。旧HANDOFF完整内容可从整理前commit `6d6b2cf`读取；更早记录见[2026-10-09历史归档](docs/CAIdex-HANDOFF-历史记录-2026-10-09.md)，历史状态不覆盖本文件。未开发新功能、未重跑无变更源码测试；最新源码CI证据如上。交接提交前已通过19份Markdown/112本地链接/22锚点、git diff --check及最终差异检查；227个其他tracked文件逐字保持6d6b2cf，唯一修改为HANDOFF。提交/push后核实branch/HEAD/origin/干净工作区，交接提交号以git log -1为准。
