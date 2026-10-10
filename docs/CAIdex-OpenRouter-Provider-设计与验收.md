# CAIdex OpenRouter Provider：设计与验收

阶段F/G；2026-10-10。源码`model/providers/openrouter`，复用ModelProvider、CustomResponses传输、执行端Credential Broker和Gateway。基础文本、显式上下文/effort/正文/backend路由及逐route Classic平面function工具已分别三平台离线验收；平面阶段Provider51项；本轮namespace/custom新增12项、Provider63已精确三平台验收。本轮完整原生历史载体新增19项定向已验，Provider82；完整本地已通过，新源码CI待验。Lite、实际固定OpenRouter Runtime及商业Live/Full尚未验，不修改Codex唯一执行/审批真源或V3阶段顺序。

## 官方契约与默认入口范围

已核对[Responses概览](https://openrouter.ai/docs/api_reference/responses/overview)、[基础文本与多轮](https://openrouter.ai/docs/api_reference/responses/basic-usage)、[请求参考](https://openrouter.ai/docs/api/api-reference/responses/create-responses)、[目录](https://openrouter.ai/docs/api/api-reference/models/list-all-models-and-their-properties)及[提供商路由](https://openrouter.ai/docs/guides/routing/provider-selection)。API及模型后端会变化，后续接线仍须重新核对，不以OpenAI兼容说明授Full。

- `OpenRouterConfig::new`使用官方`https://openrouter.ai/api/v1/`，仅接受执行端`provider=openrouter, kind=api-key`引用；显式`with_base_url`复用HTTPS/字面loopback安全策略，拒绝userinfo/query/fragment。固定派生models/responses端点，不读取环境Key，不接受模型提供URL，不新增代理或TLS关闭选项。
- 原生目录是`data`数组，不要求OpenAI的object=list/model、owned_by或created。仅验证对象/有效唯一ID并排序，完整原始条目（价格/architecture/supported_parameters/null/未知扩展/大数）保留和序列化；这些字段未转换为已验能力。list_models返回执行端配置与目录交集，source=ProviderCatalog，配置Unknown及兼容等级保持，不提供失败后的缓存或名称猜测。
- 六方法沿用共享接口。入口仅Classic，基础接受文本或system/developer/user/assistant文本消息数组、字符串instructions、有效max_output_tokens/temperature/top_p；assistant typed message可保留有效ID/completed状态和空annotations/logprobs。未知参数、工具/媒体/结构输出/推理输入、Runtime attribution/cache元数据、Lite及调用方路由覆盖在Key/POST前拒绝。
- OpenRouter Responses没有服务端会话状态。每次携带完整已支持文本历史，强制store=false；true/畸形store、background=true或非空previous_response_id拒绝。完整原生输出、usage和opaque reasoning可保留，但当前不接受reasoning输入或声明其绑定/回放兼容性。HTTP200的failed/incomplete不表示成功生成。
- 编译固定provider.require_parameters=true和allow_fallbacks=false，拒绝caller provider/models/plugins等绕过。共享传输无自动重试；这些政策不固定初始后端、不证明不同轮次选择同一供应商，也不保证其数据留存政策。绑定推理历史前须另验明确后端作用域，不能用model slug冒充稳定原生会话。
- 基础context headers全部拒绝，Runtime内部身份不转发native；原生turn-state拒绝。不提供Decline/执行工具之类新引擎。原生未请求function/custom/MCP/tool-search调用及参数事件拒绝，终态store=true拒绝；坏流不会作为成功交付。正文未知非工具扩展保留。
- 源预算先于本地编译，编译后请求及响应/frame预算、TLS、禁止重定向、单次认证、并发slot、deadline/取消/Drop与实际socket关闭均复用Custom；错误是静态安全分类，不附原生错误正文、Key、请求或URL。同步SecretStore开始后仍不能强停，取消后不POST。

## 离线验收

测试`model/providers/openrouter/tests/provider.rs`复用既有原生Adapter的隔离HTTP fixture模式，只用合成Key，无用户配置/商业API/模型下载。14项覆盖：

| 范围 | 验证 |
| --- | --- |
| 配置、目录、六方法 | Key引用/端点/Classic门控，完整目录JSON及配置交集，无能力/Full升级，坏ID/重复/空目录 |
| 文本请求与SSE | 两轮完整文本前缀，developer/instructions保留，native model替换，router政策/store=false，JSON/SSE未知字段/大数/opaque输出及三种终态 |
| 请求信任边界 | 状态/路由覆盖/未知控制/工具/媒体/推理/坏标量/消息门控，Key与POST为0，源与编译预算 |
| 原生输出 | 未请求调用、存储与turn-state拒绝，不交付成功或可执行调用 |
| 传输与凭据 | 缺Key/跨owner/预取消/到期，错误正文脱敏、重定向/content-type/大小拒绝，header/body timeout与cancel实际断连，Drop后slot复用 |
| Gateway | 无/坏监听token拒绝，内部context Key前拒绝，监听token与模型Bearer分离，不发送cookie/API-Key/组织等调用方header |

初次可执行定向13通过/1失败来自custom/tool-search测试夹具缺少共享Core要求的身份字段；补齐合法输入后最终14/0/0。早期编译错误仅新文件函数放置/测试SecretKind与reqwest既有功能调用已修正；不新增依赖功能或修改共享实现。日志`/tmp/caidex-openrouter-basic/{check,focused-first,focused-final}.log`。完整本地workspace524/0/59、既有固定Runtime57/0/0、全workspace/all-targets Clippy -D warnings、fmt/diff及逐名边界检查通过，workspace旧510+14、既有Runtime通过名完全保持。无新外部package/version，仅Cargo新增内部crate；共享Core/Custom/Gateway/Runtime/Broker及其他Provider源码/workflow未改。本次精确源码三平台CI已独立完整核验，见下段；不能借Qwen旧CI代验，既有Runtime回归不是实际OpenRouter接线。

精确源码`9563df09607a71da409466ba3d0c69b9a09c3ca6`/[CI38004040230](https://github.com/bboytang/CAIdex/actions/runs/38004040230)整体completed/success。Linux114068611307/Windows114068611369/macOS114068611031各17steps成功或条件跳过；完整raw2166/1852/1863行，workspace524/519/523（failed0，ignored59/57/57）、既有固定Runtime57/56/56（failed/ignored0），OpenRouter14、Qwen72、DeepSeek57每名每平台一次。全通过名582/575/579精确为旧CI37982550340集合+14，无遗漏/重复；既有Runtime通过名完全保持。watch及三份完整日志下载exit0，ci-check通过；日志`/tmp/caidex-ci-38004040230-{linux,windows,macos}-raw.log`及status.json，checker/result在`/tmp/caidex-openrouter-basic/`。这些Runtime回归不代验实际OpenRouter接线或商业Live/Full，也不代验iOS应用。

## 后续顺序

1. 本步完整本地检查、差异审查、源码9563df0推送与CI38004040230精确三平台已完成；保留证据，不重复基础适配。
2. 显式上下文/neutral text/effort已三平台收尾；逐route正文控制也已三平台收尾，平面function及namespace/custom也已精确三平台收尾，下一步完整政策绑定历史；官方推理/工具/路由契约已核对，不复制Qwen summary或DeepSeek明文规则。
3. 再验Lite与实际固定Classic/Lite Runtime审批/执行/取消/磁盘恢复；商业模型兼容性另需授权。
4. 保持V3 F/G→H/I→Windows/SSH/iOS/CLI/Relay/R；生产Host、GUI、账户/记忆及完整CLI均仍待实现。


## 显式Runtime上下文与逐route effort（本步）

2026-10-09，依据固定Codex client构造与实际Classic合成wire，核对[Responses推理](https://openrouter.ai/docs/api_reference/responses/reasoning)和[推理参数及模型差异](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens)。不是Qwen summary或DeepSeek reasoning_text映射。

- `with_runtime_context`显式消费session_id/x-client-request-id/x-codex-turn-metadata、字符串map的client_metadata与有效prompt_cache_key，不转发身份或提供缓存承诺；保留取消/deadline。未知header与turn-state仍拒绝，discovery使用相同身份隔离。
- 只消费neutral text：null/空对象、format.type=text、缺省/null verbosity；有意义verbosity、结构输出、未知子字段拒绝。typed developer/user消息允许有效ID，保留完整input；system ID、缺type、空/控制字符/非字符串ID与用户status拒绝。原assistant规则保持。
- `with_reasoning_effort_mapping(route, source, native)`逐route执行端显式配置、拒绝未知route/非法值/重复source，source只转换一次。词汇none/minimal/low/medium/high/xhigh/max仅为配置词汇，不是各后端支持承诺；OpenRouter可能继续映射预算或模型原生等级。缺配置拒绝，明确Unsupported仅允许映射结果none，Unknown不提升为已验能力。
- 请求只接受reasoning.effort，不默默丢弃summary/context/include/max_tokens/exclude/enabled，推理输入和工具历史仍拒绝。强制require_parameters=true/allow_fallbacks=false，不保证初始后端固定、精确计算量或数据留存；后续后端绑定历史必须单独核验。
- 编译前源预算与编译后共享预算保持；没有新HTTP栈、执行器、全局环境Key或外部依赖。默认入口保留旧14项行为。

新增9项隔离HTTP验收：身份消费与GET/POST隔离、默认/turn-state拒绝、坏正文/ID/历史控制、逐route与映射一次、配置/Unsupported门控、源预算先于消费、SSE组合无损、7种显式词汇及none/能力不升级、Gateway监听与模型身份隔离。定向23/0/0已通过；完整本地workspace533/0/59、既有固定Runtime57/0/0、全workspace/all-targets Clippy -D warnings、fmt/diff通过；workspace逐名精确旧524+9、旧Runtime名完全保持，旧14项测试逐字保留，依赖/共享/其他Provider/workflow不变。本步精确源码三平台CI已验，见下段。日志`/tmp/caidex-openrouter-context/`。现阶段不认领工具历史/Lite/实际固定OpenRouter Runtime/商业Live或Full。

精确源码`92d0612a50bd23dd13c0d69e2536636e7c6320b1`/[CI38005373396](https://github.com/bboytang/CAIdex/actions/runs/38005373396)整体completed/success。Linux114072825747/Windows114072825607/macOS114072825701各17steps成功或条件跳过；完整raw2175/1861/1872行，workspace533/528/532（failed0，ignored59/57/57）、既有固定Runtime57/56/56（failed/ignored0），OpenRouter23、Qwen72、DeepSeek57每名每平台一次。全通过名591/584/588精确为旧CI38004040230集合+9，无遗漏/重复；既有Runtime通过名完全保持。watch及三份完整日志下载exit0，ci-check通过；日志`/tmp/caidex-ci-38005373396-{linux,windows,macos}-raw.log`及status.json，checker/result在`/tmp/caidex-openrouter-context/`。这些Runtime回归不代验实际OpenRouter接线或商业Live/Full，也不代验iOS应用。


## 逐route正文控制（本步）

2026-10-09；依据固定Runtime text/verbosity与service_tier构造，以及[Responses请求参考](https://openrouter.ai/docs/api/api-reference/responses/create-responses)和[Service Tiers](https://openrouter.ai/docs/guides/features/service-tiers)。直接请求参考页面本轮404，官方搜索缓存可读；实时Service Tiers明确支持Responses及实际tier/路由差异，不用Chat兼容性推测完整Responses。

- `with_verbosity_instruction(route, source, instruction)`仅接受low/medium/high和非空指导，route必须配置且source不重复。原instructions保留，追加一次换行+指导；缺省/null/空instructions直接使用指导，不重复编译或改变input/稳定ID。此为执行端显式文本指导，不是原生verbosity精确等级、结构输出或输出长度承诺。只消费当前已支持的text形状，未知字段/JSON结构输出拒绝；没有runtime_context时身份与text.format仍拒绝。
- `with_service_tier_mapping(route, source, native)`允许auto/default组内部显式映射、flex同值、priority/fast别名组、ultrafast同值；不将priority/flex显式请求静默降级default，scale/未知/跨组/重复/空/未配置拒绝。Null不提供隐式默认；只映射一次，原生返回实际service_tier保留，JSON/SSE不伪造SLA、账单或供应商身份。
- tier值是请求政策，不保证有对应模型端点或实际按请求档服务；官方有按可用池改变实际服务tier的行为。require_parameters=true/allow_fallbacks=false保持，不把tier或model slug当稳定后端；后端身份/原生工具/推理历史另验。无真实模型调用或计费。
- 源预算先于身份消费/指导追加，编译后共享预算仍在Key/POST前校验。默认未配置text/tier行为、取消/deadline、turn-state拒绝、summary/context/include/原生推理输入/工具门控保持；仅OpenRouter源码/测试，不改共享传输/Broker/Gateway/Runtime/其他Provider/依赖/workflow。

新增8项覆盖逐route/三种指导/原instructions、非Runtime身份/结构输出拒绝、配置/坏值/重复/跨档门控、源与编译预算先于凭据、tier别名映射一次及实际返回保留、context+effort+正文SSE组合、默认/Null拒绝。首轮30/1为SSE夹具未设置stream=true，补齐后定向31/0/0；旧23项测试逐字保留。完整本地workspace541/0/59、既有固定Runtime57/0/0、全workspace/all-targets Clippy -D warnings、fmt/diff通过，workspace旧533通过名精确+8、旧Runtime完全保持；依赖/共享/其他Provider/Runtime/workflow未改。本步精确源码三平台CI已验，见下段，日志`/tmp/caidex-openrouter-body/`；不认领原生工具历史/Lite/实际OpenRouter Runtime/商业Live/Full。

精确源码`2799a4cbe75371528955ca8d24efe5660cd54429`/[CI38006780427](https://github.com/bboytang/CAIdex/actions/runs/38006780427)整体completed/success。Linux114077274589/Windows114077274731/macOS114077274576各17steps成功或条件跳过；完整raw2183/1869/1880行，workspace541/536/540（failed0，ignored59/57/57）、既有固定Runtime57/56/56（failed/ignored0），OpenRouter31、Qwen72、DeepSeek57每名每平台一次。全通过名599/592/596精确为旧CI38005373396集合+8，无遗漏/重复；既有Runtime通过名完全保持。watch及三份完整日志下载exit0，ci-check通过；日志`/tmp/caidex-ci-38006780427-{linux,windows,macos}-raw.log`及status.json，checker/result在`/tmp/caidex-openrouter-body/`。这些Runtime回归不代验实际OpenRouter接线或商业Live/Full，也不代验iOS应用。


## 显式后端路由前置步骤（2026-10-10）

`with_backend_selection(route, backend)`由执行端逐route配置一个原生provider slug，拒绝未知route、重复配置、空路径段和非ASCII字母数字/点/下划线/短横线的段；不猜测模型或目录能力，不验证商业端点是否可用。编译后写入`provider.only=[backend]`，保留`require_parameters=true/allow_fallbacks=false/store=false`。未配置route保持旧行为；调用方provider/models/session_id、工具、summary/context/include与推理历史仍拒绝。源预算与注入后共享预算均先于凭据和POST；不改HTTP栈、Runtime或依赖。

[官方路由契约](https://openrouter.ai/docs/guides/routing/provider-selection)说明base slug覆盖供应方多个variant/region；full slug限制指定变体。故本步只建立路由政策，不证明实际服务endpoint稳定。[Router Metadata](https://openrouter.ai/docs/guides/features/router-metadata)显示名也不能冒充精确backend身份；JSON/SSE已有metadata原样保留，不用请求slug覆盖实际输出。原生工具与完整政策/前缀绑定历史仍是下一步，[Responses工具契约](https://openrouter.ai/docs/api_reference/responses/tool-calling)已核对；不套用Qwen summary-only或DeepSeek明文规则。

新增5项：逐route与默认隔离/元数据不升级/原生JSON保留；配置坏值/重复/未知route；请求路由、工具与历史门控；注入后预算Key/POST前拒绝；context+effort+verbosity+tier+backend的SSE组合及实际metadata保留。定向36/0/0一次通过；完整本地workspace546/0/59、既有固定Runtime57/0/0、全workspace/all-targets Clippy -D warnings、fmt/diff通过。逐名精确旧workspace541+5、既有Runtime名完全保持，旧31测试正文逐字不变、Cargo.lock不变；18份Markdown/91本地链接/22锚点通过。精确源码三平台CI已验，见下段。日志`/tmp/caidex-openrouter-backend/`。本步不是原生工具/绑定历史/Lite/实际OpenRouter Runtime或商业Live/Full验收。


精确源码`2c9954c80fd37950a188b9799ffc9c428b730fd4`/[CI38008446175](https://github.com/bboytang/CAIdex/actions/runs/38008446175)整体completed/success。linux job114082598633，workspace546/0/59、既有固定Runtime57/0/0，raw2188行/通过名604；windows job114082598878，workspace541/0/57、既有固定Runtime56/0/0，raw1874行/通过名597；macos job114082598892，workspace545/0/57、既有固定Runtime56/0/0，raw1885行/通过名601。各17steps成功或条件跳过，OpenRouter36/Qwen72/DeepSeek57每名每平台一次；全通过名精确旧CI38006780427+5，无遗漏/重复，旧Runtime名完全保持。watch和完整日志下载exit0，ci-check通过；/tmp/caidex-openrouter-backend/及/tmp/caidex-ci-38008446175-{linux,windows,macos}-raw.log只作补充，仓库证据足够跨机器恢复。这些既有Runtime回归不是实际OpenRouter接线或商业Live/Full。


## Classic原生平面function工具（2026-10-10）

`with_native_tools(route)`须先逐route配置`with_backend_selection`，拒绝缺配置/未知route/重复启用；默认及其他route仍拒绝工具。工具仍是数据，Codex 0.160.1是唯一执行与审批真源，不新增执行器/HTTP栈/依赖或改变Runtime。显式Unsupported拒绝声明或parallel=true；Unknown不升级兼容性。

- [官方Responses工具契约](https://openrouter.ai/docs/api_reference/responses/tool-calling)的平面function声明、auto/none/named选择、call_id配对和文本结果已核对；required模式也作为显式策略向native转发，并在完成响应中本地检查至少一个合法调用。声明唯一name/parameters对象；strict=true、defer_loading=true、namespace/custom/allowed_tools/server tools仍拒绝。此阶段不宣称JSON Schema约束验收。
- 声明、选择及parallel控制原样转发一次；parallel=false或明确Unsupported时最多交付一个调用，多调用JSON/SSE均拒绝，不拆请求/重试/执行。保留原输入顺序：并行call组/逆序成对结果、string或input_text数组、合法可选ID/状态；重复、未配对、缺结果、跨未完成call的用户消息、畸形参数及未声明调用Key/POST前拒绝。
- JSON终态验证完成状态、声明/选择、JSON object参数、call_id/item ID唯一及旧工具身份复用后交付。opaque reasoning、未知输出扩展、usage/metadata原样保留，但其输入回放仍关闭；这里只是明文工具配对校验，不是完整原生历史绑定或来源认证。已含未知原生call扩展的无损输入回放须后续载体支持，不能用本步已支持的明文字段替代。
- SSE在预算内缓冲模型事件，到终态核对完整调用身份/索引、参数delta/arguments.done/output_item.done与最终输出后原样交付；支持交错Unicode参数流、done-only与terminal-only，不提前交付任何模型/工具事件，传输Heartbeat继续传递。此选择会延迟文本显示；实际Runtime、commercial和下游长流体验仍待验。累计decoded事件预算、取消503/provider_cancelled、deadline504/provider_timeout、Drop、错误后满槽释放及完成后取消待交付队列均有定向证据，不自动重连或补跑工具。
- 源预算在工具字段提取前；编译后共享预算仍Key/POST前。client_metadata/Runtime header仅本地消费，provider.only/禁fallback/store=false继续强制。summary/context/include/原生推理输入、Lite仍关闭；显式backend只是路由政策，不证明实际endpoint稳定。

新增15项独立`tests/tools/mod.rs`，旧36测试正文逐字保持。初期RequestContext非Clone及测试编译问题已修正；首可执行47/1、补充50/1为取消旧断言499不符共享503契约，统一后51/0/0。最终差异核对发现新call_id复用旧item ID，负例先失败再最小修复，51/0/0重验通过。Clippy首轮仅新测试初始化写法已修正。最终workspace561/0/59、既有固定Runtime57/0/0、全workspace/all-targets Clippy -D warnings、fmt/diff通过；完整通过名精确旧workspace546+15、旧Runtime名完全保持，旧36测试正文逐字保留、Cargo.lock不变、共享/其他Provider/Runtime/workflow未改。未知item大整数ID夹具无损保留，不把工具身份约束套到未知输出上。18份Markdown/91本地链接/22锚点及差异边界通过；精确源码三平台CI已验，见下段；既有固定Runtime57/0/0已通过，未改生产/fixture，不是实际OpenRouter接线。日志`/tmp/caidex-openrouter-tools/`。

当前恢复顺序见末节；完整政策绑定历史→Lite→实际Classic/Lite固定Runtime审批/执行/取消/磁盘恢复；不照搬Qwen summary-only或DeepSeek明文规则，不调用商业API或读取真实Key。


精确源码`65001cd0952685d635d61e4d6d682e174928eb01`/[CI38010128508](https://github.com/bboytang/CAIdex/actions/runs/38010128508)整体completed/success。linux job114087959146，workspace561/0/59、既有固定Runtime57/0/0，raw2203行/通过名619；windows job114087959047，workspace556/0/57、既有固定Runtime56/0/0，raw1889行/通过名612；macos job114087959157，workspace560/0/57、既有固定Runtime56/0/0，raw1900行/通过名616。各17steps成功或条件跳过，OpenRouter51/Qwen72/DeepSeek57每名每平台一次；全通过名精确旧CI38008446175+15，无遗漏/重复，旧Runtime名完全保持。watch和完整日志下载exit0，ci-check通过；/tmp/caidex-openrouter-tools/及/tmp/caidex-ci-38010128508-{linux,windows,macos}-raw.log只作补充，仓库证据足够跨机器恢复。这些既有Runtime回归不是实际OpenRouter接线或商业Live/Full。


## OpenRouter原生namespace/custom与选择子集（三平台离线已验）

- 官方Python SDK固定`de9aa273aa0ba658bdbd55e56f2f999c2dce782e`的[请求工具union](https://github.com/OpenRouterTeam/python-sdk/blob/de9aa273aa0ba658bdbd55e56f2f999c2dce782e/docs/components/responsesrequesttoolunion.mdx)、[namespace](https://github.com/OpenRouterTeam/python-sdk/blob/de9aa273aa0ba658bdbd55e56f2f999c2dce782e/src/openrouter/components/namespacetool.py)、[custom](https://github.com/OpenRouterTeam/python-sdk/blob/de9aa273aa0ba658bdbd55e56f2f999c2dce782e/src/openrouter/components/customtool.py)和[custom SSE](https://github.com/OpenRouterTeam/python-sdk/blob/de9aa273aa0ba658bdbd55e56f2f999c2dce782e/src/openrouter/components/customtoolcallinputdeltaevent.py)已核对；真实后端支持仍须逐模型另验。
- `with_backend_selection`→`with_native_tools`→`with_advanced_tools`逐route显式启用。默认/仅平面route仍关闭高级声明。原生namespace分组、描述及function/custom身份直接保留，不生成alias、不把freeform改JSON function。name/namespace为1–64 ASCII字母数字/下划线/短横线；同qualified name不能跨kind重复，namespace组唯一且非空/有字符串描述；不接受嵌套namespace或server工具。
- custom format缺省、text或lark/regex grammar原样转发，不做本地grammar/JSON Schema匹配承诺。strict/deferred/async=true、programmatic/server/subagent工具拒绝；返回已知server/subagent字段拒绝，其他未知非工具输出扩展/opaque reasoning/大整数仍保留。
- [官方choice union](https://github.com/OpenRouterTeam/python-sdk/blob/de9aa273aa0ba658bdbd55e56f2f999c2dce782e/src/openrouter/components/openairesponsestoolchoice_union.py)未定义具名custom/namespace字段；具名高级及allowed_tools精确限制原生声明子集，再编译一次auto/required，保留namespace描述/声明顺序。不假设未定义的原生selector格式；完成响应仍检查qualified身份、kind、必需/并行政策。旧平面named choice不变。
- 同轮function/custom call_id与item ID共同唯一，拒绝复用；custom结果须custom_tool_call_output，function须function_call_output，文本字符串/input_text数组原顺序保留。这里只做明文成对校验；未知call扩展输入及reasoning输入回放仍关闭，不能冒充完整绑定历史。
- 混合SSE分别累积function arguments/custom input，added/delta/done/terminal的kind/namespace/id/call_id/name/内容校验；交付前拒绝损坏流，终态/仅done路径保留；复用原缓冲预算/cancel/deadline/Drop/Heartbeat。工具route文本仍延迟到终态，Provider不执行工具。
- 新增12项，最终Provider63/0/0；旧51项正文保持，source/compiled预算先于Broker/POST，Gateway原生custom交付与身份隔离通过。本轮workspace573/0/59、Clippy全workspace/all-targets -D warnings、既有固定Runtime57/0/0、fmt/diff及18份Markdown/95本地链接/22锚点通过，旧workspace通过名精确+12、旧Runtime保持；新源码精确三平台已验，证据见下段；旧CI不代验本轮，商业API/实际OpenRouter Runtime未验。

恢复顺序：完整执行端/profile/endpoint/backend政策/model/compiled前缀/instructions/verbosity/tier/effort/工具政策绑定历史→Lite→实际固定Classic/Lite Runtime。不复制Qwen summary-only或DeepSeek明文规则，不据base slug/metadata显示名认领实际稳定endpoint。


精确源码`ca2cf5870e691183dc888657773e2ae993d52e8e`/[CI38047726929](https://github.com/bboytang/CAIdex/actions/runs/38047726929)整体completed/success。linux job114200540663，workspace573/0/59、既有固定Runtime57/0/0，raw2215行/通过名631；windows job114200540664，workspace568/0/57、既有固定Runtime56/0/0，raw1901行/通过名624；macos job114200540515，workspace572/0/57、既有固定Runtime56/0/0，raw1912行/通过名628；各17steps成功或条件跳过，OpenRouter63/Qwen72/DeepSeek57每名每平台一次；全通过名精确旧CI38010128508+12，无遗漏/重复，旧Runtime名完全保持。watch和三份完整日志下载exit0，ci-check通过；/tmp/caidex-openrouter-advanced/及/tmp/caidex-ci-38047726929-{linux,windows,macos}-raw.log与status.json只作补充，仓库证据足够跨机器恢复。这些既有Runtime回归不是实际OpenRouter接线、商业Live/Full或iOS应用构建。


## OpenRouter完整政策绑定原生历史（本轮定向已验）

- [官方无状态契约](https://openrouter.ai/docs/api_reference/responses/overview)、[手动reasoning历史](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens)和固定SDK de9aa273的[reasoning item](https://github.com/OpenRouterTeam/python-sdk/blob/de9aa273aa0ba658bdbd55e56f2f999c2dce782e/src/openrouter/components/outputreasoningitem.py)/[reasoning_text SSE](https://github.com/OpenRouterTeam/python-sdk/blob/de9aa273aa0ba658bdbd55e56f2f999c2dce782e/src/openrouter/components/reasoningdeltaevent.py)已核对。完整原生id/summary/content/encrypted_content/signature/format及未知非执行扩展、annotations、usage/metadata/大整数保存在v1载体内，SSE保留完整解析事件；不照搬Qwen summary-only或DeepSeek明文规则。
- `with_backend_selection`之后显式`with_native_history(route)`，独立于native_tools/runtime_context；默认与未启用route仍拒绝历史carrier/裸reasoning。凭据归属/profile/kind与规范配置endpoint、route/native model、能力、backend政策、runtime/native/advanced开关、整个route effort/verbosity/tier映射、原正文控制/工具政策均精确绑定。所有正文控制改变（含instructions/采样/预算/选择）须新分支；stream和显式本地消费的归因/cache变化不改政策。base slug可能覆盖region/variant，绑定配置与政策不证明实际供应实例相同。
- [固定Runtime ResponseItem/ContentItem](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/protocol/src/models.rs)不会保留所有未知字段。输出采用完整敏感wire的reasoning载体+Runtime可保留的文本/原生function/custom投影，未知项只留载体。回放核对整个投影组及已展开的完整原生前缀，允许Runtime省略有效ID/status及reasoning content:null，然后按原顺序恢复原始输出，载体本身不发native。工具结果继续精确kind/call_id/声明配对，已知server/deferred/async/subagent仍拒绝。
- 每个载体重新核对内部编译控制、配置与原生response/known reasoning类型、调用身份/选择/旧ID；SSE核对known文本与reasoning_text/summary_text分片、item索引/身份/done/terminal以及工具参数，坏流在任何模型投影前拒绝。失败/未完成不创建carrier，合法原生失败事件保留；部分可执行调用不交付。共享HTTP/Broker/Gateway与Codex唯一执行/审批实现不变。
- source、expanded/compiled、carrier/response、原生累计SSE及投影frame/累计输出预算均保留，超限拒绝。取消/deadline/Drop/Heartbeat和队列取消遵循共享契约；错误或Drop后关闭连接、释放单slot。文本仍缓冲到终态，完整前缀在多载体中会二次增长，预算限制不能当精简/自动截断。载体不是加密、签名认证或来源证明，可能含敏感推理/配置；Debug隐藏wire，无Key读取/保存。
- 新增19项、Provider82/0/0，旧63测试正文保持。三轮JSON→原生前缀恢复、JSON→SSE、全部原始wire/投影/政策与内部篡改、预算、取消/连接释放/Heartbeat、Gateway身份隔离定向通过。本轮workspace592/0/59、既有固定Runtime57/0/0、全workspace/all-targets Clippy -D warnings、fmt/diff及18份Markdown/99链接/22锚点通过；全通过名精确旧workspace573+19，旧Runtime57及旧63测试正文逐字保持，Cargo.lock/共享/其他Provider/Runtime/workflow未改。新源码三平台待验。summary/context/include策略、Lite和实际OpenRouter Classic/Lite Runtime另待后续；商业Live/Full未授权且未测试。
