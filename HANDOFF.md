# CAIdex 开发交接

更新：2026-10-10。Final Architecture First，继续既定V3；正式目录`/root/projects/CAIdex-v1.0`，废弃目录不修改。恢复依据为仓库、文档和精确CI，不依赖旧会话记忆或/tmp日志。

## 当前F/G恢复点：Chat Completions CI修复已本地复验，准备提交/push

2026-10-10用户正式确认V3保留两类Custom，范围阻塞已解除。当前main=origin/main=18249beabed7702581cc033f88b1922da975b1f3；[CI38089213569](https://github.com/bboytang/CAIdex/actions/runs/38089213569)整体completed/failure，macOS成功，Linux/Windows固定Runtime失败。完整失败日志已读：Linux新增工具恢复测试trace.json EOF（80通过/1失败）；Windows新增文本恢复测试trace.requests旧值1而非2（79通过/1失败）。共同根因为新Chat fixture在终态flush后才截断重写证据，读者可能抢先读取；已将写入移至终态交付前，不加轮询掩盖。job：Windows114321996516、Linux114321996732、macOS114321996776。旧源码47b6b519/CI38086078152仅作旧范围基线。

要求级复核另发现无tools却tool_choice=required会访问凭据/上游：新增既有负例先真实RED（502而非400），最小三行门控后GREEN，凭据零读取。当前未提交为该request/test、Chat fixture与HANDOFF/验收矩阵；完整本地复验workspace640/0/83（本次含原生凭据子进程项）、固定Runtime81/0/0，Clippy全workspace/all-targets-D warnings、fmt/diff通过。下一步提交/push修复并核验新的精确三平台CI，不能拿失败运行或旧绿色代验。

已实现：独立`model/providers/chat-completions`标准/chat/completions Adapter，复用六方法/Canonical/共享传输/Broker/ModelRouter/Gateway；固定Runtime仍执行/审批真源。共享Custom仅新增post_sse及单槽后台framing，原Responses路径不改；没有改现有其他Provider/Core/Gateway/Broker/workflow。正式范围同步V3/Gateway/验收矩阵，新增[Chat设计验收](docs/CAIdex-Custom-Chat-Completions-设计与验收.md)，[路由报告修订2](docs/CAIdex-Provider-路由离线兼容性报告-V1.md)现有9Adapter18个route/方言，只授ProtocolFixture/Experimental，不自动挂Registry或授真实能力。

已本地验证：新Adapter15、Custom9（旧7+新2）、固定Runtime新增3项全部通过；最终workspace639/0/83，固定Runtime完整81/0/0（1个普通fixture测试由workspace覆盖），全workspace/all-targets Clippy -D warnings、fmt/diff、fixture语法、160个本地Markdown目标通过。共享原始SSE接口缺失编译RED→GREEN；未消费双帧cancel socket不关闭真实RED→后台单槽交付GREEN。实际Classic/Lite原生批准后隔离执行、完整参数/结果及disk resume逐值一致、不重跑；Cancel/等待审批interrupt/迟到Accept不执行。首次workspace构建因磁盘满失败，清理本项目可重建Runtime编译缓存5.0GiB后完整重跑成功，未删源码/用户文件。

限制：无推理Runtime profile仅显式允许固定optional encrypted include空结果，非none effort/summary/签名/opaque仍拒绝；grammar默认拒绝，显式prompt映射没有原生强制约束。SSE终态缓冲受Runtime idle与累计投影预算限制；JSON context headers不透传，native_model返回名必须精确匹配，不猜别名。普通媒体/结构输出/状态/未知控制拒绝，模型能力/限额仍Unknown/None；商业真实验收由用户全项目完成后自验，fixture不代商业兼容性或生产Host。

准确下一步：提交上述已复验修复，核验新精确CI全部三job/17steps及完整日志，逐名比较旧CI38086078152集合+15Chat+2Custom+3Runtime；处理真实新失败后完整相关复验。并继续对V2第14–19节/V3 F/G逐要求核对，区分离线已验、用户最终商业验收与H/J/P等后续阶段，不因新增Adapter自动宣布F/G整体通过。完成后把精确证据、残余边界及下一步写入本文件/验收矩阵/Chat文档，提交push纯文档收尾；不重复实现已验任务或擅自改变A–R顺序。

## 既有阶段与用户顺序

用户确认真实模型验收在全项目完成后自行执行。开发端继续既定实现/离线验证，不询问Key/API、不读取用户Key或调用商业API；F/G商业门槛仍未验，fixture不授LiveRuntime/Full。当前先完成F/G要求级审计，未选H实现；后续H–R按V3，生产Host不能由临时Runtime fixture代验。每重要步骤更新本文件，代码沿定向→完整本地→diff→提交/push→精确三平台CI，不重复已完成任务。

## 本轮已实施与本地验证

- Anthropic：复用两既有固定Runtime用例，Classic重启第四POST逐值回放第三native回复全组；Lite实际rollout调用身份/input与完整结果逐值转换native text，并断言Key读取2次。无生产/fixture改动；定向各1/0/0、workspace621/0/73、Runtime71/0/0、Clippy/fmt/diff通过。
- Gemini：缺prompt usage下界、thought-only STOP phase分别真实RED→GREEN；共享逻辑最小修复，计数缺失仍未知，cached不重复添加；自动回放拒绝保持。新增完整JSON/SSE组双向互换，Classic/Lite两目标入口在Key/POST前400；旧native/投影取消/Drop用例增强到未消费多帧分支，cancel先socket关闭再排空缓存，无成功/工具done/载体，permit复用。Google94/0/0、workspace623/0/73、Runtime71/0/0、Clippy/fmt/diff通过。
- Compaction：读固定d27764b的tasks/compact.rs、model-provider/src/provider.rs和compact_remote_v2*.rs后按实际V2 wire实现测试。新增Classic/Lite远端compaction_trigger→opaque compaction、实际checkpoint、实际app-server重启/disk resume完整item逐值承接；Lite本地摘要复用原Classic流程另设测试。远端显式loopback模式仅用OpenAI身份选择上游固定V2，合成认证，不改生产能力。定向远端1/0/0、本地2/0/0，完整workspace623/0/75、Runtime73/0/0、Clippy/fmt/diff通过。新增2测试，最终diff/fixture语法已检查。
- [Provider路由离线报告V1](docs/CAIdex-Provider-路由离线兼容性报告-V1.md)：八Adapter16个正例route/方言逐profile、fixture版本、限制、来源绑定；四条切换route沿用[切换报告V1](docs/CAIdex-模型切换-离线兼容性报告-V1.md)。仅ProtocolFixture/Experimental，不自动挂Registry，不改Unknown能力或模型限额。16条记录/JSON字段验证通过；阶段初始20份Markdown/145链接/22锚点，删除过时交接后最终140链接/22锚点通过。
- 核对、Registry/Router、Classic/Lite切换、跨Provider显式文本交接均沿旧已验实现，不重复。生产执行器/审批、依赖、workflow未改。

## 精确CI证据

源码`47b6b519d69955328218b2b7953794641090de52`/[CI38086078152](https://github.com/bboytang/CAIdex/actions/runs/38086078152)整体completed/success，三job各17steps成功或预期跳过，完整日志精确旧CI38085110360集合+1自动压缩Runtime。Linux/Windows/macOS workspace623/618/622（忽略80/78/78）、固定Runtime78/77/77（无忽略），全部0失败；raw2314/2000/2011行、函数通过名701/694/698，另Secret doctest1。旧Provider/本地与远端手动及非成功/idle/执行审批回归无遗漏或重复，watch与下载exit0。

本阶段本地workspace623/0/80、Runtime78/0/0、Clippy/fmt/diff/fixture语法和143个Markdown目标/锚点通过；225个非任务tracked文件与d4471cb逐字节保持。纯文档收尾HEAD以实际Git为准。

源码`d72c88a292a4ab41775e38c95c73e7f62d2f7cda`/[CI38085110360](https://github.com/bboytang/CAIdex/actions/runs/38085110360)整体completed/success，三job各17steps成功或预期跳过，完整日志精确旧CI38084218039集合+1本地压缩Runtime。Linux/Windows/macOS workspace623/618/622（忽略79/77/77）、固定Runtime77/76/76（无忽略），全部0失败；raw2312/1998/2009行、函数通过名700/693/697，另Secret doctest1。旧Provider/远端compaction/idle/执行与审批回归无遗漏或重复，watch及完整下载exit0。

本阶段本地workspace623/0/79、Runtime77/0/0、Clippy/fmt/diff/fixture语法、143个Markdown目标/锚点通过；225个非任务tracked文件与bada37e逐字节保持。纯文档收尾HEAD以实际Git为准。

源码`74bc3205ca52c129a8a723624cb453b97fbfccea`/[CI38084218039](https://github.com/bboytang/CAIdex/actions/runs/38084218039)整体completed/success，三job各17steps成功或预期跳过，完整日志精确旧CI38083310797集合+1Runtime。Linux/Windows/macOS workspace623/618/622（忽略78/76/76）、固定Runtime76/75/75（无忽略），全部0失败；raw2310/1996/2007行、函数通过名699/692/696，另Secret doctest1。旧Provider、OpenAI idle、远端压缩及审批回归无遗漏/重复，watch和下载exit0。

本次最终143个Markdown本地目标及锚点通过；224个非任务tracked文件与4f3fd25逐字节一致。文档收尾HEAD以实际Git为准，无源码修改不重跑Rust CI。

源码`8c3ecc86d2b212522bb5aeecf26ad8e272a8320f`/[CI38083310797](https://github.com/bboytang/CAIdex/actions/runs/38083310797)整体completed/success，三job各17steps成功或预期跳过。完整日志精确旧CI38081995688通过集合+2Runtime，无遗漏/重复；Linux/Windows/macOS workspace623/618/622（忽略77/75/75），固定Runtime75/74/74（无忽略），全部0失败。raw2308/1994/2005行、函数通过名698/691/695，另Secret doctest1；watch与日志下载exit0。

本次141个Markdown本地目标及锚点检查通过；225个非任务tracked文件与e258828基线逐字节保持。纯文档收尾提交以实际HEAD为准，无源码变更不重跑Rust CI。

每个已验运行均整体completed/success，三个job各17steps成功或预期条件跳过，完整raw日志逐名核验，新旧回归无遗漏/重复；workspace ignored不能记为通过，固定Runtime另显式执行。Linux原生凭据及Secret compile-fail doctest保持。

| 范围 / 源码 | 精确CI | workspace Linux/Windows/macOS（通过） | 固定Runtime Linux/Windows/macOS（通过） | 状态 |
| --- | --- | --- | --- | --- |
| 本轮compaction / 9b0b48f6705c3847449ccd8cf0e1c3761300f4b1 | [38081995688](https://github.com/bboytang/CAIdex/actions/runs/38081995688) | 623/618/622 | 73/72/72 | 已完整核验，旧集合+2Runtime |
| 本轮Gemini / 57451e9996aee201954ee5d404dd02e22dba8dd2 | [38081567944](https://github.com/bboytang/CAIdex/actions/runs/38081567944) | 623/618/622 | 71/70/70 | 已完整核验，旧集合+2Google |
| 本轮Anthropic / 9db1fe7edeca3ce1e262f6cbdbafdc6b2261609f | [38081159725](https://github.com/bboytang/CAIdex/actions/runs/38081159725) | 621/616/620 | 71/70/70 | 已完整核验，旧集合保持 |
| 核对/Google单调用 / d9b0d1a49d2aa78822d4110d07a5dc91a22e0800 | [38079737190](https://github.com/bboytang/CAIdex/actions/runs/38079737190) | 621/616/620 | 71/70/70 | 已完整核验，旧集合+2Google |
| Lite切换 / 3a57bcbfb8f4aa942019c081404b6833daa57642 | [38072098372](https://github.com/bboytang/CAIdex/actions/runs/38072098372) | 619/614/618 | 71/70/70 | 已完整核验 |
| 跨Provider文本交接 / b5e7fca6b620eb45e0689eca5a8c2eee36c93562 | [38071015251](https://github.com/bboytang/CAIdex/actions/runs/38071015251) | 619/614/618 | 69/68/68 | 已完整核验 |
| Classic切换 / 430305d5a0ae323b9d739bc6db05ae2dfc30c3bf | [38069700504](https://github.com/bboytang/CAIdex/actions/runs/38069700504) | 619/614/618 | 67/66/66 | 已完整核验 |
| Registry/Router / e2f3039c6c4f82898f2a30246e6bd1981ffec671 | [38063184946](https://github.com/bboytang/CAIdex/actions/runs/38063184946) | 619/614/618 | 64/63/63 | 已完整核验 |

前轮压缩CI：Linux/Windows/macOS job114300649493/114300649731/114300649671；raw2304/1990/2001行、函数通过名696/689/693（另Secret doctest1），workspace ignored75/73/73、失败0，Runtime73/72/72（0失败/忽略）；完整集合精确Gemini旧CI+2、533个Provider函数保持。watch与全部下载exit0，20份Markdown/140本地链接/22锚点检查通过，215个其他tracked文件保持恢复基线，架构/授权正文原样保持。

Gemini已验：Linux/Windows/macOS job114299375039/114299374956/114299375049；raw2300/1986/1997行、函数通过名694/687/691（另Secret doctest1），workspace ignored73/71/71、失败0。Google94、Anthropic123、Custom7/OpenAI11/Ollama69/DeepSeek57/Qwen72/OpenRouter100；所有旧通过名保持，新增2名每平台一次，Runtime集合保持。Anthropic上轮job114298181143/114298181177/114298180978；raw2298/1984/1995行，workspace ignored73/71/71、失败0。

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

- 真实商业模型/API签名/限额/费用/推理、Ollama实际daemon和模型/性能由用户最终验证，当前未验；生产Host/多端恢复未实现，不能用固定Runtime的临时测试冒充。
- H的持久journal、快照/sequence、结果未知与提交幂等、审批多端竞争、活动轮次模型边界及跨Provider持久关联/一般历史适配待实现。稳定thread/start无原始history；实验thread/resume.history为云专用禁止入口，未使用。
- I账户/PostgreSQL/Memory/同步/邮件和J–O客户端、L SSH、P完整CLI、Q Relay、R实际平台/终端/认证/多端验收仍待。apps/cli现有doctor、credentials status/set/remove、version/help，不冒称CLI-01～34/账户55项/UI16项完成。
- 未开放语义仍明确拒绝：各Provider差异见验收/route报告；未知Runtime扩展不保证类型化持久化全量往返。native载体是JSON一致性门控而非签名真实性/来源认证，backend配置不是实际endpoint证明；完整前缀二次增长/预算、文本终态缓冲保持。
- compaction手动正常、本地/远端Classic/Lite失败/取消，以及Total scope采样前自动阈值与磁盘恢复已离线验证；其他scope/轮末/TokenBudget/无限历史或生产恢复未验，不由已验路径认领。同步SecretStore开始后不能强停，仅保证取消后不POST；OpenAI Classic/Lite Runtime下游idle已验，Gemini Classic/Lite纯comment下游idle时序已专门离线验证，但不承诺跨Gateway keepalive支持。jsonschema0.58.6有离线字节/regex限额但无硬CPU抢占，H隔离待验。
- 无当前权限/推送阻塞；新Chat首次CI失败已定位且本地修复，新CI尚待验证；旧自动审批拒绝已由持续授权解除，不能当成当前阻塞。

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
