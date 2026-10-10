# CAIdex 开发交接

更新：2026-10-10。会话切换专用恢复点；Final Architecture First，继续既定 V3，不重新设计。正式目录 `/root/projects/CAIdex-v1.0`；废弃 `/root/projects/CAIdex` 不修改。

## 当前完整F/G目标恢复点（本轮）

本轮恢复HEAD=origin/main=`4931b58663435448139d725ed2414db496a001e4`，开始干净。已加强两个既有Anthropic固定Runtime用例：Classic重启第四POST逐值回放第三native回复全组（含opaque）；Lite落盘custom调用input/身份与完整结果逐值转native text核对，并断言Key读取2次。无生产/fixture/依赖/workflow变更。定向各1/0/0、完整workspace621/0/73、固定Runtime71/0/0、Clippy workspace/all-targets-D warnings、fmt/diff通过；最终diff已检查。当前待提交/push及精确三平台CI，不能将本地结果记成CI证据。

下一步：提交本阶段并核验精确CI，继续Gemini缺prompt usage下界、thought-only phase、整组互换/满槽cancel，随后逐route报告及固定Runtime compaction证据。F/G目标保持active，不能以两断言替代全部。用户已明确：真实模型验收最后全项目完成后由用户自行执行；当前先完成可离线实现/验证任务。不再询问Key/API，不读取用户Key或调用商业API；真实模型保持未验，不升级LiveRuntime/Full。生产Host仍属后续阶段。

## 当前状态与第一步

本会话恢复基线 `a9502451bbe392a5fd00bd39e073caef17c20960` 与用户交接一致，开始时 main=origin/main、工作区干净。最新源码 `d9b0d1a49d2aa78822d4110d07a5dc91a22e0800` 已提交/push；[CI38079737190](https://github.com/bboytang/CAIdex/actions/runs/38079737190) 已完整三平台核验成功。本次重要步骤已完成，纯文档收尾提交号以最终 `git log -1` 为准；不能把恢复基线或源码SHA当作收尾HEAD。

已完成[离线门槛核对V1](docs/CAIdex-FG-离线验收核对-V1.md)：对照V3 F/G、Registry/Router、八Adapter和Runtime，区分已实现/已验/明确拒绝/后续阶段/真实API门槛；重新下载旧精确CI三个日志，529个Adapter测试名三平台保持。Lite切换/报告及既有Router不重复实现。

核对后补齐Gemini单调用thought豁免、无tools、未opt-in none三个专项，新增2个HTTP/Provider测试覆盖JSON/SSE×Classic/Lite。生产源码、固定Runtime、fixture、依赖与workflow未改；225个其他tracked文件保持恢复基线。定向3/0/0（含旧测试）、Google92/0/0、workspace621/0/73、固定Runtime71/0/0、Clippy全workspace/all-targets-D warnings、fmt/diff与schema指纹通过；新CI全部通过名精确旧CI+2、旧Runtime保持。唯一独立只读审查无Critical/Important，文档多余承诺已修。

上轮收尾时没有未完成代码、运行中测试/CI、已知失败或推送阻塞；本轮状态以上方当前恢复点为准。 本次纯文档收尾仅涉及HANDOFF、Gemini验收及离线核对V1；最终HEAD/origin与干净工作区以提交后的检查为准。F/G仍未整体完成；Anthropic断言本轮本地通过，后续Gemini等恢复点以上方为准。商业API/LiveRuntime/Full与生产Host仍未验，不从fixture升级Registry能力。

新 Codex 顺序：

1. 先读本文件、[AGENTS](AGENTS.md)，运行 `git status --short --branch`、`git log -4 --oneline`、`git rev-parse HEAD origin/main`，核实恢复点和用户新改动。
2. 读[实施计划 V3](docs/CAIdex-实施计划-V3.md)、[Gateway验收](docs/CAIdex-Model-Gateway-设计与验收.md)、[模型切换报告V1](docs/CAIdex-模型切换-离线兼容性报告-V1.md)，对照 Registry/Router、Runtime切换测试及各Provider验收。
3. 本次核对与Gemini三个单调用分支专项已完成；Anthropic本轮本地已补精确恢复/结果断言，再按表处理Gemini usage下界、thought-only文本phase、整组载体互换/满槽cancel等已确认缺口。每模型商业报告/真实API、生产Host仍未验，不自动升级Registry能力。
4. 每重要步骤更新本文件。代码改动按定向→相关完整本地→最终diff→提交/push→精确源码三平台CI推进；CI必须核对实际SHA、3jobs/步骤、完整日志及测试名，不仅查看绿色状态。纯文档无需重复Rust CI。
5. 商业API/真实Key、生产Host持久关联/通用历史适配、客户端等另有边界。不能把这些缺口伪装成离线完成；满足V3前置门槛后按 H→I→J/K→L→M/N/O→P→Q→R，不据fixture跳H。

## 最新已验证证据

最新源码 `d9b0d1a49d2aa78822d4110d07a5dc91a22e0800` / [CI38079737190](https://github.com/bboytang/CAIdex/actions/runs/38079737190)：整体completed/success，三个job各17steps成功或条件跳过，完整日志下载exit0、watch exit0，精确SHA/步骤/通过名核对通过。

| 平台 / job | workspace 通过/失败/忽略 | 固定Runtime 通过/失败/忽略 | 完整raw行数 / 通过名数 |
| --- | --- | --- | --- |
| Linux / 114293993979 | 621/0/73 | 71/0/0 | 2298 / 693 |
| Windows / 114293994016 | 616/0/71 | 70/0/0 | 1984 / 686 |
| macOS / 114293993905 | 620/0/71 | 70/0/0 | 1995 / 690 |

每平台Google92、Anthropic123、Custom7/OpenAI11/Ollama69/DeepSeek57/Qwen72/OpenRouter100逐名一次；新增2名各一次、旧workspace/Runtime集合不漏不重复，Linux原生凭据及Secret编译失败doctest保持。离线fixture不证明商业模型或生产Host。19份Markdown/124本地链接/22锚点与最终diff检查通过。

本会话环境问题已解决：定向首次因沙箱禁止loopback监听失败，允许socket环境复跑通过；固定Runtime首次因项目二进制缺失71项NotFound。按CI安装项目隔离0.160.1、版本/schema校验后完整71/0/0；全局0.162.1未用于验收。不是生产代码缺陷RED，无当前已知测试失败。

### 前置Lite切换证据（保持）

前置源码 `3a57bcbfb8f4aa942019c081404b6833daa57642` / [CI38072098372](https://github.com/bboytang/CAIdex/actions/runs/38072098372)：整体completed/success，每job17steps成功或条件跳过。

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
- 本次已核实交接点名的覆盖缺口：Gemini thought-call/无tools/none已补并三平台验收；重复/部分SSE usage此前已覆盖。剩余Gemini整组载体互换、满槽cancel/Drop、缺prompt的usage下界及thought-only文本phase；canonical整数/显式空ID仍为既定兼容限制。Anthropic重启第三轮内容及完整Lite结果/落盘精确断言尚未补。具体范围见离线核对V1，不重复旧任务，不把结构校验当历史真实性认证。
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
- 锁定Rust1.99.0（rust-toolchain.toml）、Codex0.160.1/固定commit；Node22.23.3已核实，项目.tools/codex固定0.160.1已安装（不改全局0.162.1）。本机2026-10-10磁盘剩余约1.9GB，执行前重查 `df -h .`，不主动清缓存/用户数据。
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

## 上次交接核验（a950245历史）

交接仅整理本文件，删除过时“下一步/未实现/旧Git”流水状态，保留当前实现、准确恢复点、关键证据/限制/架构/授权。旧HANDOFF完整内容可从整理前commit `6d6b2cf`读取；更早记录见[2026-10-09历史归档](docs/CAIdex-HANDOFF-历史记录-2026-10-09.md)，历史状态不覆盖本文件。未开发新功能、未重跑无变更源码测试；最新源码CI证据如上。交接提交前已通过19份Markdown/112本地链接/22锚点、git diff --check及最终差异检查；227个其他tracked文件逐字保持6d6b2cf，唯一修改为HANDOFF。提交/push后核实branch/HEAD/origin/干净工作区，交接提交号以git log -1为准。


## 本次F/G核对与Gemini专项恢复点

核对、两个测试/三个分支的本地与精确三平台CI已完成，源码d9b0d1a已push；本次收尾仅三份文档，226个其他tracked文件逐字保持精确CI源码；下一步Anthropic断言尚未实施，不重跑无变更源码Rust CI。相关日志是本会话新生成的/tmp/caidex-fg-audit-20261010/{focused-allowed,google,workspace,runtime,clippy,schema,ci-watch,ci-linux,ci-windows,ci-macos}.log，checker为check_evidence.py/check_status.py；持续恢复以本仓库与精确CI为准，不要求/tmp保留。上次交接a950245记录作为历史，不覆盖本次状态。

下一项代码入口：runtime/bridge/tests/real_runtime.rs 的 real_classic_native_anthropic_discovers_and_executes_mcp_tools（重启第四POST补前一完整原生回复组）、real_lite_native_anthropic_code_mode_executes_tool_and_replays_result（canonical/disk custom结果与native tool_result完整逐值对照）。先读fixture实际格式与既有断言，定向复现，再决定是否只需补测试；本会话未修改这两个用例。
