# CAIdex F/G 离线验收核对 V1

核对日期：2026-10-10；恢复基线 main / origin/main=`a9502451bbe392a5fd00bd39e073caef17c20960`，工作区干净；最新源码 `3a57bcbfb8f4aa942019c081404b6833daa57642`。依据[实施计划 V3](CAIdex-实施计划-V3.md)、[Gateway 验收](CAIdex-Model-Gateway-设计与验收.md)、各 Provider 验收、实际源码/测试及精确 CI。此文是剩余门槛索引，不是商业模型兼容性报告，也不宣布 F/G 完成。

## 基线证据

重新读取 [CI38072098372](https://github.com/bboytang/CAIdex/actions/runs/38072098372) 的精确 head、三个 job/全部17steps与完整日志；整体 completed/success，各步骤成功或条件跳过。Linux/Windows/macOS workspace 分别619/614/618通过（0失败、73/71/71忽略），固定 Runtime 71/70/70通过（0失败/忽略）。忽略项由独立 Runtime/native credential 步骤补验，不能算作 workspace 通过。下面八个 Adapter 的529个测试函数名在三平台日志分别匹配通过；这是既有回归证据，不是新增测试或真实模型验证。

| Adapter | 基线离线测试数 | 已实现/已验证范围入口 | 仍不能认领 |
| --- | --- | --- | --- |
| Custom | 7 | [独立传输测试](../model/providers/custom/tests/provider.rs)，Responses/TLS/六方法 | 未知兼容端点的真实能力 |
| OpenAI | 11 | [验收](CAIdex-OpenAI-Provider-设计与验收.md)，Models/Responses透传、固定Classic/Lite两轮及interrupt | 商业版本、Runtime未知扩展完整往返 |
| Anthropic | 123 | [验收](CAIdex-Anthropic-Provider-设计与验收.md)，原生签名/v1–v4、动态MCP、Lite真实审批执行 | 默认cached web、签名真实性、完整grammar |
| Gemini | 90 | [验收](CAIdex-Gemini-Provider-设计与验收.md)，JSON/SSE/v1–v2、工具/媒体/推理、Classic/Lite真实Runtime | discovery/web/deferred/后置system等明确拒绝项、商业Full |
| Ollama | 69 | [验收](CAIdex-Ollama-Provider-设计与验收.md)，history/工具/结构输出、Classic MCP与Lite执行/恢复 | 实际daemon版本、模型下载/推理及性能 |
| DeepSeek | 57 | [验收](CAIdex-DeepSeek-Provider-设计与验收.md)，逐route effort/history/custom/Lite、真实Runtime7项 | 实际模型、通用custom/媒体等未开放语义 |
| Qwen | 72 | [验收](CAIdex-Qwen-Provider-设计与验收.md)，namespace/custom映射/history/Lite、真实Runtime7项 | 真实地域/计划/API、截断/grammar硬约束 |
| OpenRouter | 100 | [验收](CAIdex-OpenRouter-Provider-设计与验收.md)，显式backend、原生namespace/custom/history/Lite、真实Runtime7项 | 实际后端/版本证明、商业Full |

上述范围须以对应文档最后的已验子阶段为准；较早段落描述当时状态，不能拿历史“尚未实现”重复开发。Fixture只验证协议/编译/传输与固定执行器，不验证模型智能、商业限额、真实签名、生产Host。

## V3 门槛与剩余工作

| 门槛 | 当前事实与证据 | 剩余分类 |
| --- | --- | --- |
| F Responses/取消/工具/opaque/错误 | Core、Gateway及各Adapter已有离线回归；Runtime执行与审批保持固定0.160.1/d27764b | 已验证具体范围；不是所有Provider×参数×时序的穷举证明 |
| G Registry/ModelRouter | [Registry](../model/core/src/registry.rs)、[Router测试7项](../model/core/tests/router.rs)、[Gateway Router测试2项](../model/gateway/tests/router/mod.rs)：显式ID、目录归属、漂移拒绝、未知模型/方言/streaming门控，无fallback | 已实现/离线已验；不新建Registry或HTTP栈 |
| Unknown/Unsupported展示与拒绝 | ModelMetadata/六方法公开三态及未知限额；目录不升级能力/报告。Router检查dialect/streaming，其余请求语义由Adapter门控；Provider测试含Key/POST前拒绝 | 核心数据与拒绝已验；最终GUI/CLI展示归J/P，不伪造当前产品UI |
| 每模型版本化报告 | CompatibilityReport schemaVersion=1、reference/testedModelVersion/source/level门控已有；[切换报告V1](CAIdex-模型切换-离线兼容性报告-V1.md)仅四条合成route | 各Provider文档是分阶段证据，尚非每个商业版本报告；八Adapter16个正例route/方言已有[路由离线报告V1](CAIdex-Provider-路由离线兼容性报告-V1.md)，逐profile/版本/限制绑定，不批量赋Full或自动挂生产Registry |
| 模型切换 | Classic3、Lite2与跨Provider2项真实Runtime测试已验；同Provider只限两个明确同方言route组合；跨Provider只显式可见文本新线程及foreign reasoning拒绝 | 不重复既有测试；完整工具状态/其他组合未验，活动轮次约束/持久关联/通用历史适配归H，不能用云专用history入口替代 |
| Provider工具/推理边界 | 配置、签名载体和完整前缀绑定已有；Gemini单调用3分支已补专项；Anthropic恢复/完整结果精确断言已三平台验证，详下表 | 补已有契约的离线证据；未开放功能继续明确拒绝，不按slug猜支持 |
| F真实模型验收与G商业报告 | 当前没有授权的真实Key/API/daemon证据 | 用户确认全项目完成后自行验收；当前继续可离线任务，真实模型仍未验，不宣称商业Compatible/Full |

## 已确认缺口与顺序

| 项目 | 核对起点发现（不是当前待办） | 本轮完成或保留限制 |
| --- | --- | --- |
| Gemini单调用thought豁免/无tools/未opt-in none | [provider测试](../model/providers/google/tests/http/provider.rs)已有opt-in auto/none/多调用/MAX_TOKENS；缺上述三个专项。[请求编译器](../model/providers/google/src/request.rs)已有0/1/无限分支，[数量校验](../model/providers/google/src/provider.rs)排除thought调用 | 本次新增两项回归覆盖三个分支，JSON/SSE×Classic/Lite本地已验；实现未改动，精确新CI三平台已验 |
| Gemini重复/部分SSE usage | [history测试](../model/providers/google/tests/history.rs)已有专项更新与raw保留 | 旧缺口已覆盖，不重复补 |
| Gemini缺prompt的usage下界、thought-only call文本phase | normalized_usage提前返回null；outcome看到任意functionCall即ToolCall，投影phase沿outcome | 两项分别真实RED→GREEN；共享usage下界/非thought执行分类最小修复，本地JSON/SSE/旧回放拒绝通过；源码57451e9/[CI38081567944](https://github.com/bboytang/CAIdex/actions/runs/38081567944)三平台已完整核验 |
| Gemini整组载体互换/满槽取消 | 现有组内编辑/错model/request拒绝；投影取消测试先消费到text，native满槽已有deadline测试 | 本轮完整JSON/SSE组双向互换在Key/POST前400拒绝；原生/投影未消费多帧cancel/Drop专项已三平台精确CI38081567944验证；未暴露需修改生产的载体/传输漏洞 |
| Gemini整数/空ID | catalog只接受canonical整数，content拒绝显式空ID | 已记录兼容表示限制，无实际端点新依据，不泛化codec |
| Anthropic重启第三轮/完整Lite结果 | [Runtime测试](../runtime/bridge/tests/real_runtime.rs)重启只比旧请求前缀；Lite结果只contains marker，没有完整canonical/disk→native结果逐值核对 | 本轮复用两个既有用例补齐：完整第三native回复、Lite落盘调用及完整结果逐值对照；定向各1/0/0，workspace621/0/73、固定Runtime71/0/0、Clippy/fmt/diff通过；精确源码9db1fe7/[CI38081159725](https://github.com/bboytang/CAIdex/actions/runs/38081159725)三平台完整日志保持旧通过名，已验 |
| Runtime远端opaque compaction/Lite | 核对起点仅Classic手动摘要证据；当前范围见[能力对照](CAIdex-Runtime-能力对照.md) | 固定wire独立核实；Classic/Lite远端opaque压缩+磁盘恢复、新Lite本地摘要共新增2Runtime测试，本地73/0/0通过；源码9b0b48f/[CI38081995688](https://github.com/bboytang/CAIdex/actions/runs/38081995688)三平台完整核验，Runtime73/72/72、0失败 |

本表针对交接点名门槛和源码分支，不声称全仓穷举或全时序形式验证。后续按确认缺口逐项推进；商业/Host边界保持V3最终方案。

## 本次执行计划与验收

沿既定V3细化验收，不重新设计：

1. 在既有 `model/providers/google/tests/http/provider.rs` 增加thought调用不占可执行单调用额度的正例；JSON/SSE×Classic/Lite，完整native history往返，只有一个真实function_call交付。
2. 增加无tools/未opt-in none的零调用额度负例；JSON/SSE×Classic/Lite，负例502且不交付call/载体/成功终态，认证与原生POST各一次。
3. 定向→Google完整→workspace与固定Runtime完整→Clippy/fmt→最终diff；不修改生产实现/依赖/fixture/workflow，除非新测试实际暴露既有缺陷。
4. 本地通过后按已有授权commit/push main；核对精确源码三平台CI全部job/步骤/完整测试名，更新本报告、Provider文档及HANDOFF。本地已验：定向3/0/0（含旧单调用测试）、Google92/0/0、workspace621/0/73、固定Runtime71/0/0、Clippy workspace/all-targets-D warnings、fmt/diff、固定schema。workspace函数通过名精确旧CI+2，Runtime通过名保持，无遗漏/重复。精确源码`d9b0d1a49d2aa78822d4110d07a5dc91a22e0800`/[CI38079737190](https://github.com/bboytang/CAIdex/actions/runs/38079737190)三平台已验。首次沙箱拒绝loopback监听，允许本地socket的验证环境复跑通过；不是生产缺陷RED。

固定Runtime本会话首次缺少项目二进制，71项NotFound；按CI安装隔离0.160.1、版本/schema校验后重跑完整通过，未使用全局0.162.1。唯一独立只读审查无Critical/Important，文档中多余的“无调用正例”承诺已删除。225个其他tracked文件保持恢复基线；旧Provider/Runtime用例正文未改。本次只有本核对文档、Gemini验收、Google测试与HANDOFF变更。

## 本次完成证据与准确下一步

源码`d9b0d1a49d2aa78822d4110d07a5dc91a22e0800`/[CI38079737190](https://github.com/bboytang/CAIdex/actions/runs/38079737190)整体completed/success；三job各17steps成功或条件跳过，完整日志逐名核验通过。Linux/Windows/macOS workspace621/616/620（0失败、忽略73/71/71）、固定Runtime71/70/70（0失败/忽略）；Google92逐名每平台一次。全通过名693/686/690精确旧基线CI+2，无旧名遗漏/重复；raw日志2298/1984/1995行，watch与下载exit0。仅Google新增2测试，其余Adapter及固定Runtime保持。上述执行计划已经完成；纯文档收尾不重复Rust CI。

下一步：Anthropic精确断言已三平台CI核验；Gemini四类边界已三平台精确CI核验；16个route/profile离线报告已写，compaction两个新增用例本地及精确三平台CI通过。已确认的可离线补齐缺口均已实施/验证，没有据此认领全参数/全时序形式证明。商业真实模型与生产Host仍未验，F/G不标商业整体完成；离线复核与压缩精确CI已完成，按用户确认顺序继续H独立开发，不能用fixture认领商业门槛。


## 最终离线复核（2026-10-10）

源码`9b0b48f6705c3847449ccd8cf0e1c3761300f4b1`/[CI38081995688](https://github.com/bboytang/CAIdex/actions/runs/38081995688)三平台完整核验：Linux/Windows/macOS workspace623/618/622（ignored75/73/73）、固定Runtime73/72/72，全部failed0；三job各17steps成功或预期条件跳过。完整通过名精确Gemini CI38081567944旧集合+2Runtime；533个Provider函数、原生凭据和Secret doctest保持，raw2304/1990/2001行。20份Markdown/140本地链接/22锚点、16条报告记录/JSON字段、fixture语法和最终diff通过；215个非任务tracked文件保持恢复基线。


## 续轮要求级核对恢复点

2026-10-10从实际e258828恢复后继续F/G完成核对，不能将前轮“确认缺口已补”视为全F/G证明或直接转H。新增Runtime idle Classic/Lite，以及远端压缩Classic/Lite失败/取消四路径，定向各1/0/0通过，详[Runtime能力对照](CAIdex-Runtime-能力对照.md)。完整回归/提交/精确CI状态见HANDOFF。

下一步继续核对Gemini纯comment/下游idle、本地压缩非成功与自动阈值是否属于既定离线门槛，并逐项核对Registry/Router、报告和各Provider明确拒绝范围；尚未确认整体离线闭环。真实商业模型按用户要求留到项目最后自行验收，仍未验证，不用fixture代验。


源码`8c3ecc86d2b212522bb5aeecf26ad8e272a8320f`/[CI38083310797](https://github.com/bboytang/CAIdex/actions/runs/38083310797)整体completed/success，三job各17steps成功或预期跳过。完整日志精确旧CI38081995688通过集合+2Runtime，无遗漏/重复；Linux/Windows/macOS workspace623/618/622（忽略77/75/75），固定Runtime75/74/74（无忽略），全部0失败。raw2308/1994/2005行、函数通过名698/691/695，另Secret doctest1；watch与日志下载exit0。 本阶段离线证据已验证；后续未验范围保持上述说明。


Gemini comment/下游idle续轮：复用已验idle恢复流程，新增Classic/Lite纯comment持续输入专项，定向1/0/0通过；当前完整回归/精确CI状态见HANDOFF。该原明确证据缺口已实施，不改变生产行为；后续核对本地compaction非成功、自动阈值及整体要求。真实模型仍由用户最后验收。


源码`74bc3205ca52c129a8a723624cb453b97fbfccea`/[CI38084218039](https://github.com/bboytang/CAIdex/actions/runs/38084218039)整体completed/success，三job各17steps成功或预期跳过，完整日志精确旧CI38083310797集合+1Runtime。Linux/Windows/macOS workspace623/618/622（忽略78/76/76）、固定Runtime76/75/75（无忽略），全部0失败；raw2310/1996/2007行、函数通过名699/692/696，另Secret doctest1。旧Provider、OpenAI idle、远端压缩及审批回归无遗漏/重复，watch和下载exit0。 本专项离线验证完成，商业模型/生产Host未验。


本地compaction续轮：新增Classic/Lite失败/取消及disk resume四路径，实际摘要wire、非成功终态、socket关闭、无成功checkpoint、完整history及Lite工具声明恢复定向1/0/0通过；生产不变。完整回归/精确CI见HANDOFF。与远端v2证据独立，下一步自动阈值及全F/G要求级审计；真实模型仍由用户项目最后验证。


源码`d72c88a292a4ab41775e38c95c73e7f62d2f7cda`/[CI38085110360](https://github.com/bboytang/CAIdex/actions/runs/38085110360)整体completed/success，三job各17steps成功或预期跳过，完整日志精确旧CI38084218039集合+1本地压缩Runtime。Linux/Windows/macOS workspace623/618/622（忽略79/77/77）、固定Runtime77/76/76（无忽略），全部0失败；raw2312/1998/2009行、函数通过名700/693/697，另Secret doctest1。旧Provider/远端compaction/idle/执行与审批回归无遗漏或重复，watch及完整下载exit0。 本地非成功专项已完整离线验证；自动阈值、商业模型及生产Host仍未验。


自动阈值续轮：四条Classic/Lite本地摘要/远端V2路径以显式预算和合成usage触发固定Runtime采样前检查，无手动compact/start；真实checkpoint、完整摘要/opaque重放、实际disk resume及无再压缩定向1/0/0通过。完整回归/精确CI状态见HANDOFF。此项明确测试范围不代全scope/全时序或商业模型，下一步继续要求级完成审计。


源码`47b6b519d69955328218b2b7953794641090de52`/[CI38086078152](https://github.com/bboytang/CAIdex/actions/runs/38086078152)整体completed/success，三job各17steps成功或预期跳过，完整日志精确旧CI38085110360集合+1自动压缩Runtime。Linux/Windows/macOS workspace623/618/622（忽略80/78/78）、固定Runtime78/77/77（无忽略），全部0失败；raw2314/2000/2011行、函数通过名701/694/698，另Secret doctest1。旧Provider/本地与远端手动及非成功/idle/执行审批回归无遗漏或重复，watch与下载exit0。 本专项已完整离线验证；整体F/G审计未完成，商业模型与生产Host未验。


## 要求级整体审计恢复点（2026-10-10，整体未完成）

恢复实际main/origin/main=`64176a2edd5b5bedf7f8d4fef00fa0c0113de604`，工作区干净。本轮重新读取V3 F/G、第2节与原V2第14–19节、当前六方法接口/Registry/Router、Custom及兼容API实际端点、Provider/route报告与固定Runtime证据；从GitHub重新读取精确源码47b6b519d69955328218b2b7953794641090de52的[CI38086078152](https://github.com/bboytang/CAIdex/actions/runs/38086078152)元数据、三job全部17steps和完整日志，不沿/tmp旧状态恢复。

以下是门槛状态及证据范围，不将未知或后续阶段项计为已通过：

| 要求 / 权威来源 | 当前可证事实 | 状态与准确剩余 |
| --- | --- | --- |
| V3 F：Responses、工具 normalization、opaque、错误/限流 | [Core视图与协议测试](../model/core/tests/responses.rs)、[终态/流测试](../model/core/tests/stream.rs)、[Gateway HTTP](../model/gateway/tests/http.rs)：经典/Lite、工具参数原值/文本及媒体结果、usage缺值、opaque/未知字段、大数字、终态区别；429/HTTP-date、认证/TLS/预算及关闭路径已有回归 | 明确协议范围已离线验；不是每个未知商业端点支持证明 |
| V2第16节 / V3 G：六方法最终接口 | [ModelProvider](../model/core/src/provider.rs)六方法已定义；八Adapter及ModelRouter实现同一trait，编译/测试通过；Gateway复用注入接口，不执行工具 | 已实现/离线已验；通用Chat Completions Custom是否属本次范围见末行 |
| V3 G：模型注册及显式路由 | [Registry](../model/core/src/registry.rs)、[Router](../model/core/src/router.rs)校验固定ID/能力/版本报告；目录只归属显式注册Adapter，重复/漂移拒绝，未知路由/方言/streaming拒绝，无自动fallback/推理重试 | Core Router7项及Gateway Router2项三平台通过；不重复实现 |
| V2第18–19节 / V3 G：能力与兼容等级 | 三态、context/output未知限额、prompt profile、版本/来源及四级兼容性公开；配置/目录不授报告，fixture Full/Compatible拒绝，漂移报告拒绝 | 数据及门控已验；最终GUI/CLI展示归J/P；商业能力未知，不能用模型slug或fixture赋支持 |
| V3 G：逐模型版本报告 | [16个route/方言报告](CAIdex-Provider-路由离线兼容性报告-V1.md)与[4条切换route报告](CAIdex-模型切换-离线兼容性报告-V1.md)绑定fixture/profile版本、来源与限制 | 上述离线路由有版本化证据；尚无每个真实商业版本报告，不自动挂生产Registry或授LiveRuntime/Full |
| V3 G：各Provider工具/推理 | 原有Adapter表及各专属文档保留准确配置、native历史绑定、工具/推理/媒体/结构输出转换、Key/POST前明确拒绝范围 | 八个现有Adapter具体范围已实现/离线验；未开放功能不是隐含支持，不能推给任意兼容端点 |
| V3 G：切换与不透明历史 | 同Provider Classic/Lite明确组合；跨Provider显式文本新线程/foreign reasoning拒绝；本地/远端手动成功、失败/取消及Total采样前自动阈值/实际disk resume均有固定Runtime证据 | 当前离线路径已验；活动轮次边界、持久关联与通用历史适配按V3归H，其他压缩scope/轮末/TokenBudget未由当前路径认领 |
| V3 F：取消、超时、恢复 | Provider/Gateway guard/Drop/背压、实际socket与permit；OpenAI Classic/Lite Runtime idle、Gemini持续comment下游idle及显式恢复已独立验证 | 当前路径已验；同步SecretStore已开始读取不能强停，但取消后不POST；不承诺所有时序穷举 |
| V3：固定原生Runtime执行/审批 | 固定0.160.1/d27764b；实际批准执行、取消不执行、拒绝前无副作用、多轮工具/rollout/disk resume，Runtime为执行/审批真源 | 已有离线真实进程证据；生产Host及客户端端到端分别归H/K/O/P/R，fixture不代验 |
| V3 F真实模型门槛 / G商业报告 | 没有本轮商业Key/API、真实Ollama daemon/模型/性能或商业签名推理证据 | 用户明确全项目完成后自行验证；保持未验，不再询问Key/商业费用，不因该用户顺序停止独立开发 |
| V2第17节两类Custom / V3第2节接入范围 | 原V2分列Custom OpenAI-compatible与Custom Responses-compatible；V3明确自定义Responses及DeepSeek/Qwen/OpenRouter等兼容API。实际Custom仅CustomResponses，DeepSeek/OpenRouter端点responses、Qwen端点compatible-mode/v1/responses；不存在通用chat/completions Adapter | **范围待澄清，整体未完成**。已向用户询问是否仍要求通用Chat Completions自定义入口；未收到答复前不以现有原生Responses Adapter代验，也不擅自新增/删除最终范围 |

精确CI重新核验：整体completed/success；三平台Core29、Gateway24、Provider533个源码测试函数各出现一次。Provider计数为Custom7/OpenAI11/Anthropic123/Google94/Ollama69/DeepSeek57/Qwen72/OpenRouter100；这些是现有范围回归，并不证明商业支持。Linux/Windows/macOS workspace623/618/622（忽略80/78/78），固定Runtime78/77/77（无忽略），全部0失败；函数通过名701/694/698、raw2314/2000/2011行。全部17steps成功或仅非Linux跳过两项Linux原生凭据步骤，完整下载exit0。

准确下一步：获取Custom入口范围答复；若保留两类，先核实原生Chat Completions请求/回复/流与现有共享传输复用边界，再实施既定Adapter及独立证据；若用户确认当前F/G以V3自定义Responses范围为准，记录该确认后继续未决要求的整体审计。未作出上述范围选择，不能宣布F/G全部完成或跳到H。已有离线验证不重做；纯文档审计不重新触发完整Rust CI。
