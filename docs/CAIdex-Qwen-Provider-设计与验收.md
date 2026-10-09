# CAIdex Qwen Provider：设计与验收

阶段 F/G；2026-10-09。代码 `model/providers/qwen`。当前包括原生目录/基础 Responses、显式 Runtime 控制/逐模型推理和原生summary绑定历史、function/namespace和显式custom映射接入，沿用既定 ModelProvider/Custom transport/Credential Broker/Gateway；不另造 HTTP 框架、Agent 或工具执行器。完整 Qwen Codex 兼容性、商业 Live/Full 和生产 Host **尚未验收**，不能因目录或文本请求成功升级兼容等级。

## 官方契约与执行端配置

只读核对：[原生模型目录](https://www.alibabacloud.com/help/en/model-studio/list-models)、[Responses](https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-openai-responses)、[地域与计划端点](https://www.alibabacloud.com/help/en/model-studio/base-url)。实际服务会更新，后续工具/推理接入需重新核对；固定 Codex 仍为 0.160.1 / d27764b，不以供应商接入教程授予 Full。

- `QwenConfig::new(base, CredentialRef)` 要求执行端明确选择地域/工作区/计费计划对应的基址和 `provider=qwen, kind=api-key`。生产基址是官方域名根，例如 `https://<workspace>.cn-beijing.maas.aliyuncs.com/`；不能传 SDK 的 `/compatible-mode/v1` 前缀。显式可信代理可带统一路径前缀。
- 从同一基址派生 `api/v1/models` 与 `compatible-mode/v1/responses`，不自动换地区、工作区、旧路径或 Provider，不探测环境 Key。不自动选择计费计划；工作区/地域 Key 和计划可用性由执行端配置及将来的显式连接测试核对。
- 沿用 HTTPS/TLS 验证、无重定向/自动 POST 重试、HTTP 仅 literal loopback、URL 禁 userinfo/query/fragment；当前 Key 只由执行端 Broker 解析。Debug 不显示端点/配置/原生 wire。
- 目录 GET 与推理共用限流、取消、绝对期限、响应字节限制及安全错误。共享 Custom 新增 `get_json_with_query`，只为执行端非秘密目录参数编码，不放宽 Endpoint 校验，不接收模型输入选址；旧 `get_json/post_json` 保持原行为。

## 原生目录与六方法

目录是 `success/output.total/page_no/page_size/models`，模型主标识为 `model`，不是 OpenAI `object/data/id/created/owned_by`。官方样例缺部分可选声明并允许 null，未消费的属性、价格、时间、大数和未知扩展原样保留在 NativeModel.wire，序列化仍是原生模型对象。

分页固定每页20项，按总数取完整目录；最多256页/5120项及聚合响应字节预算，全过程共享一个总期限。页号/页大小/数量不符、重复ID、分页过程中总数变化、坏success/模型ID或超限均拒绝，不返回部分清单或缓存成功，不无限重新扫描。该上限是客户端资源界限，不是官方总模型容量承诺。

ModelProvider 六方法全部接入：

- `discover_models(context)` 返回完整原生目录；`list_models()`只返回目录与已配置路由的交集，source为ProviderCatalog。
- `metadata/capabilities/credential_requirements` 是执行端配置的只读查询，不读秘密或发请求。未知模型拒绝，未知能力保留Unknown；没有兼容报告时不编造。
- `create_response/stream_response` 共用已配置模型别名→native_model路由、Broker认证、Custom JSON/SSE 和取消/背压。仅Classic路由；声明不支持文本或streaming时在Key前拒绝。

## 基础请求与输出边界

本步支持文本字符串或user/assistant/system/developer消息、instructions、stream、max_output_tokens及合法temperature/top_p。结构化文本按角色校验，developer保持原角色和顺序，不沿用DeepSeek的降级规则；完整assistant输出消息要求id/status/completed/content数组，半成品历史拒绝。

服务会忽略未列控制且默认store=true，因此本步采用允许字段校验并显式写store=false。背景false只表示本地前台语义，在发送前消费；默认策略下，store=true/background=true、服务端conversation/previous_response_id、tools/choice/parallel、reasoning控制、text/verbosity/结构输出、include、client/cache元数据、媒体及未知字段均在Key前拒绝（以下显式策略仅开放列出的本地控制/推理映射），不能静默变成普通聊天。源请求与补充store后的请求各自受字节预算约束；原生max_output_tokens最低16。nullable普通可选参数保持原值，store null仍编译false。

原生message/reasoning summary、usage和未知扩展按完整JSON/SSE保留。未声明function/custom/MCP/内置工具调用及相关事件拒绝，流关闭真实socket/释放slot，不交付可执行工具；不把reasoning summary当OpenAI加密载体。原生turn-state头拒绝，未绑定reasoning输入不能用于继续线程。本步不建立签名、历史来源认证或原生历史回放。

共享核心仍区分completed/incomplete/failed；非流式HTTP200返回failed/incomplete也不表示生成成功，调用方须检查真实终态。SSE缺完成、坏协议/[DONE]不作为成功；错误诊断脱敏，原生正文不做破坏性替换。Gateway token与模型Bearer独立，不能转发给供应商。

**已知原生限制：Responses会自动截断超出其输入预算的上下文，约为模型context window的80%。** 当前基础字节预算不是token预算，也不能证明服务未截断。后续真实Runtime/长上下文接线须按模型/版本核对输入预算、token计算与压缩/拒绝策略；未验证之前不能承诺长历史无损或授Full。目录的context/max_input字段不直接成为Codex实测证据。

## 本步验证与证据

夹具全部是隔离loopback与合成Qwen Key，无真实API/用户秘密/付费模型。测试覆盖：

| 范围 | 断言 |
| --- | --- |
| 目录与六方法 | 两页完整scan、精确GET路径/query/Bearer、null/未知大数/原生序列化、配置交集/Unknown/无Full |
| 异常目录 | OpenAI格式、坏success/分页/ID、重复与总数变化、空目录、页数/聚合预算拒绝 |
| JSON/SSE | developer优先级、消息历史、原生summary/正文/usage/扩展、精确native模型/store=false、无Lite头 |
| Key前边界 | ignored/unimplemented字段、媒体/未绑定reasoning、坏文本/半成品消息、能力不支持、错误provider/owner/Key缺失、源与编译后超限、取消/过期 |
| 传输生命周期 | 分页不续总期限、query编码及URL保护、取消/Drop真实断连、占槽与释放、HTTP/Retry-After/无重定向、未知工具拒绝 |
| Gateway | 独立监听token、执行端Key只去native、流终态及无秘密回显 |

首次46479未编译成功（夹具误用现有Gateway API和reqwest helper），不算有效RED；第二轮38347为8通过/3夹具期望失败（空Registry、非法Lite字符串、坏SSE同chunk不保证先交付created），按既有契约修正。35171定向13/0/0通过；随后补充能力门控/完整assistant消息用例，最终workspace54851 exit0：452通过/0失败/52ignored，Qwen14每名一次、DeepSeek57及Custom7保持；全workspace/all-targets Clippy-D warnings86201 exit0，fmt/diff通过。实际Runtime源码未改，本机不重复执行旧实际Runtime全套；本步独立源码三平台CI已按下列精确提交验收，旧CI不代验。日志 `/tmp/caidex-qwen-basic-{first,green,boundaries,workspace,clippy}.log`；检查和CI恢复点见[HANDOFF](../HANDOFF.md)。尚未完成的检查不得按上轮结果冒称通过。

精确源码`3638d13863b339e230aa884820a8ff4b840526e6`/[CI37888877980](https://github.com/bboytang/CAIdex/actions/runs/37888877980)已completed/success。Linux113685036842、Windows113685037198、macOS113685037053各17steps成功或条件跳过，完整raw2059/1745/1756行；workspace452/447/451（0failed，ignored52/50/50）、既有固定Runtime50/49/49（0failed/ignored）、Qwen14每名每平台一次，DeepSeek57保持。全部通过名503/496/500等于旧精确37886226991集合加14新名，无遗漏/重复；watch84155、状态/完整日志下载及逐名checker均exit0。检查脚本`/tmp/caidex-qwen-basic-ci-check.py`，结果同前缀ci-result.json，日志`/tmp/caidex-ci-37888877980-{linux,windows,macos}-raw.log`及status.json/watch.log。固定Runtime回归不是实际Qwen工具/历史接线验收，商业Live/Full仍未验。


## 显式Runtime控制与逐模型推理（已验子阶段）

配置入口独立opt-in，默认严格路径不变：

- `with_runtime_context()`仅在执行端消费`session_id/x-client-request-id/x-codex-turn-metadata`及body `client_metadata/prompt_cache_key`。metadata只能是字符串值对象，cache key须合法非空字符串，可选null不启用功能；取消token和绝对deadline保留。普通模型目录可消费同一attribution，绝不转发身份给供应商、不启用DashScope服务端缓存。输入/输出未绑定turn-state仍拒绝。
- 本地中性`text.format={type:text}`在该策略下消费，不宣称结构化输出。`with_verbosity_instruction(level, instruction)`接受low/medium/high到非空执行端指令的一次映射，追加既有instructions，保留原顺序和developer角色；不是原生verbosity尺度保证。未知/重复映射、JSON格式或未知text字段拒绝。没有Runtime策略时，单独verbosity配置不接受中性format或local metadata。
- `with_reasoning_effort_mapping(model, source, native)`绑定已配置的route/model ID，不按名称猜测、不对其他模型继承。源与目标只允许none/minimal/low/medium/high/xhigh/max；缺失、未知、重复、未映射值或summary/context控制在Key前拒绝。官方原生模型可能进一步归一化这些值；执行端须按实际model/version建立兼容报告，显式映射不是等效推理强度保证。已声明reasoning Unsupported时拒绝非none，none可明确关闭；Unknown仍不提升为Supported/Full。不替用户编造默认effort，也不使用已弃用的enable_thinking/thinking_budget。
- 原请求在消费控制前先做字节预算，补充store/映射/指令后再检查；native model替换仍受共享传输预算。未知安全、工具、历史、加密include不会因任何单独策略而被忽略。本步不实现工具或summary历史回放。

新增7项合成loopback测试覆盖JSON/SSE完整native保留、nullable控制、所有7级及两个route不同映射/无回退、能力禁用、坏控制与不完整策略Key/POST为零、配置失败、原始/扩展预算、目录及Gateway元数据/Bearer隔离、deadline/取消/Drop真实socket。首次37935测试误用错误字段名导致编译失败，不算RED；修正后50156编译成功实际400 qwen_unsupported_context，为有效RED。47136为20/1（取消分类误期望499），39269为20/1（stall模式不发header导致超时），按共享现有契约修正夹具；48999定向21/0/0通过。日志`/tmp/caidex-qwen-controls-{red,valid-red,green,boundaries,boundaries-fixed,workspace,clippy}.log`；两处新增if风格按Clippy要求修正，最终workspace75208 exit0：459/0/52、Qwen21每名一次、原452通过名保持；全workspace/all-targets Clippy-D warnings exit0，fmt/diff及7任务路径/16Markdown/58本地链接/21锚点检查通过。源码/依赖/workflow及其他文档与01f70ad逐字节相同（本任务7路径除外）；CLI34待实施、A–R、旧CI证据保持。checker `/tmp/caidex-qwen-controls-local-check.py`及同前缀local-result.json。新源码精确三平台CI已按下列证据收尾，不借14项旧CI代验21项。

精确源码`311224bfcf0cf8019f511cf03e48397f45216715`/[CI37890668777](https://github.com/bboytang/CAIdex/actions/runs/37890668777) completed/success。Linux113690641367、Windows113690641093、macOS113690641378各17steps成功或条件跳过，完整raw2066/1752/1763行；workspace459/454/458（0failed，ignored52/50/50）、既有固定Runtime50/49/49（0failed/ignored）、Qwen21及新增7每名每平台一次，DeepSeek57保持。全通过名510/503/507等于精确旧37888877980集合+7，无遗漏/重复。watch34953、状态/下载和逐名checker均exit0；`/tmp/caidex-qwen-controls-ci-check.py`/同前缀ci-result.json，完整日志`/tmp/caidex-ci-37890668777-{linux,windows,macos}-raw.log`及status.json/watch.log。此Runtime回归不是实际Qwen工具/历史接线或商业模型验收，不授Full。

## 显式原生summary绑定历史（已验子阶段）

`with_native_history()`独立启用Classic无工具历史，默认未绑定reasoning/工具拒绝不变；不开放服务端conversation/previous_response_id、summary/context/include控制或Lite。复用原有Custom传输/Broker，不执行工具、另建Agent或变更真实Runtime。

- NativeHistory v1保存完整native request/response；SSE同时保存所有原生JSON chunks及source类型，包括未知扩展、大数、summary分段和非秘密执行端引用。载体绑定base、CredentialRef.owner/provider/profile/kind、native模型、编译后的input/instructions及完整显示输出组。序列化再加载仍检查，跨端点/Profile/model或截断、重排、修改显示组在Key/POST前拒绝。它是敏感明文JSON，不是加密、签名或来源认证，不自动进入账户Memory/云同步。
- native reasoning的summary数组原样保存；每个reasoning item在显示端投影一个拼接summary_text，统一reasoning载体置于output_index=0。SSE flat reasoning_text无需content_index，映射summary_index；正文保留真实content_index，非reasoning output偏移一位。交错/尚未到达的前置位置有界等待，未知indexed扩展留在载体而不伪造canonical索引；默认未启用history时仍保留原生wire。
- 完整completed终态重建验证added/delta/done、ID/索引、正文和summary一致后才交付载体；native failed/incomplete仍保留真实状态而不生成成功载体。坏流、EOF、超限、取消/截止及Drop关闭native socket并释放共享slot。流仍增量交付显示，不等全部推理才开始显示。
- 原生已知message只接受assistant/合法文本，拒绝角色抬升、重复/冲突ID及混用content reasoning/加密历史；未知item的字段整对象保留/比对，不按未来id/status猜语义。输出工具发现/MCP也属于未授权工具，不能当普通扩展交付。generation effort在本次请求只映射一次，旧native请求不重新映射；改变compiled instructions/前缀必须新建历史。
- byte预算覆盖源、编译/展开、完整历史与投影、raw chunks/待交付事件和frame；不是token预算，不保证原生服务未截断。载体嵌套前缀增长在H持久化前由有界预算拒绝，不静默截掉历史；实际长上下文、真实模型和Runtime接线仍待验证。

本地新增11项/共32项：JSON/SSE完整回放、三轮及序列化、两reasoning item/多summary part/交错、21种坏流及真实slot/socket、端点/Profile/模型/compiled前缀/整组篡改、一次effort与优先指令、取消/Drop/deadline、预算/非成功终态、角色/ID/坏summary、默认工具发现拒绝及Gateway两轮独立token。有效RED51870为编译成功但缺载体；首次实现编译错误不算功能RED，夹具索引/尾部断言错误按真实流修正。新unknown id/status正例进一步暴露回放误约束，已修正已知/未知类型边界。最终workspace470/0/52、Clippy全workspace/all-targets-D warnings/fmt/diff通过；全部原459通过名+11精确保持，新源码`a3e7e6de88145c780c25698cdb0e5bf2d79d3cb0`/[CI37929549601](https://github.com/bboytang/CAIdex/actions/runs/37929549601)三平台完整通过，不使用旧21项CI代验。工具声明/choice/结果、Lite、实际Qwen Runtime及商业Live/Full均未验。

精确a3e7e6de88145c780c25698cdb0e5bf2d79d3cb0/[CI37929549601](https://github.com/bboytang/CAIdex/actions/runs/37929549601)整体completed/success；Linux113816614618、Windows113816614855、macOS113816614917各17steps成功或条件跳过，完整raw2077/1763/1774行。workspace470/465/469（0failed，ignored52/50/50）、既有固定Runtime50/49/49（0failed/ignored）、Qwen32和DeepSeek57每名每平台一次。全通过名521/514/518精确等于旧CI37890668777集合+11新名，无遗漏/重复；watch16858 exit0、状态/完整日志下载和逐名checker通过，全部句柄结束。checker `/tmp/caidex-qwen-history-ci-check.py`及同前缀ci-result.json；日志`/tmp/caidex-ci-37929549601-{linux,windows,macos}-raw.log`及status.json/watch.log。checker首次仅误从单文件按async计DeepSeek57，修正为固定源码46项+lite模块11项后精确逐名通过；不算生产失败，不修改原日志。此Runtime回归仍不是实际Qwen Runtime接线或商业Live/Full验收。

## 显式function/namespace与工具历史（已验子阶段）

`with_native_tools()`独立启用Classic原生function并启用绑定历史，沿用Custom传输/Broker；不执行工具或改变真实Runtime审批。默认路径及仅with_native_history仍拒绝tools。单独该策略不开放custom、deferred、服务商MCP/内置工具或Lite，这些能力不能由文本成功或命令名称推断已验。

- 函数名按原生ASCII字母/数字/下划线/连字符与64字节界限校验，参数对象原样保留；description缺省补空字符串，namespace说明追加到成员说明。namespace成员按原声明顺序编译为唯一`caidex_ns_<index>`，重名/alias冲突在Key前拒绝；返回端恢复源name/namespace、完整arguments及未知JSON。`strict=true`、`defer_loading=true`及未知声明字段拒绝；不宣称原生Schema强制验证。
- `auto`、`none`和nullable choice保持原生语义；`required`只有一项原生声明时接受。指定function选择器编译为单项`allowed_tools/required`并限制原生声明集；源allowed_tools的auto/required按所选已声明身份编译，required仍限定单项。空/重复/未知/多项required选择在Key前拒绝；返回必须属于所选集合，none不交付调用，completed必选却未调用也拒绝，不替模型编造调用。
- 原生不列parallel_tool_calls；仅省略/null/true作为不限制并行的本地策略消费，false在Key前拒绝，后续Lite本地单调用需要独立实现/验收，不能伪称原生执行了false。已声明native_tools Unsupported时在Key前拒绝，Unknown不提升兼容等级。
- 工具回放为独立NativeHistory v2，包含source tools/choice/parallel政策与实际编译后native request/response，SSE保留所有原生chunks。逐次载体检验执行端/Profile/端点/model、编译前缀、原声明/策略、实际native声明/choice及完整投影组；v1/v2不自动迁移或松绑，工具策略/声明改变需要新历史。载体是敏感明文、不是加密或来源认证，也不进入账户Memory/普通同步。
- 源结果可为字符串或纯input_text/output_text数组（按原顺序换行拼接）；媒体/未知结果项拒绝。每组按原call顺序整理为紧邻call/result对，保留期间所有native非call显示项的相对顺序；拒绝孤儿、重复、错类型、未完成、坏参数JSON、未归属声明的调用及跨未解决调用插入user/system/developer。每个后续载体前与最终展开时使用相同整理，三轮及重装载前缀不会误用重排前的顺序。此编译不执行工具、不重试未知执行结果。
- SSE继续增量显示正文/summary；function added/arguments delta/done/raw item done暂扣，完整completed终态校验声明、ID/call_id、选择、added/delta/done参数及真实native输出后，才交付投影调用和载体。坏流/截断/非成功含调用/重复ID/超限/取消/期限/Drop不交付调用，关闭真实socket并释放共享slot。原生无调用的error事件保持真实错误语义；服务端工具发现/MCP仍拒绝。固定Responses形式的参数事件已由合成夹具验证，供应商实际事件/模型能力仍须Live核对。
- 原始source（含工具）、namespace说明编译扩展、展开/载体/raw chunk/frame均有预算；旧effort只在本次源请求映射一次。Gateway token和Runtime attribution不发送到native，模型Key仍只由执行端Broker读取。本步不改共享Core/Custom/Gateway/Runtime或第三方依赖。

本地新增11项/共43项：JSON/SSE namespace及序列化三轮、named/allowed/string选择、21种非法source、13种坏native JSON/SSE、13种坏参数流及真实socket/slot、14种policy/完整组/载体篡改Key前拒绝、direct成对结果、工具半流取消/Drop/期限、原始/编译预算/能力/默认及原生error、Gateway两轮身份隔离。有效RED89161编译成功实际qwen_unsupported_request；33项GREEN56077、38项边界77970、43项安全61856通过。新测试括号错误未编译不算功能RED；自查取消测试早期heartbeat后已补强。追加三轮69255编译成功复现qwen_history_prefix_mismatch，最小修正每个载体前统一pair，最终workspace58356 exit0：481/0/52、Qwen43每名一次、旧470通过名+11精确保持；Clippy全workspace/all-targets-D warnings52143 exit0、fmt/diff及9路径/16Markdown/64本地链接/22锚点检查通过，非任务源码/依赖/workflow/CLI34/A–R/历史CI保持。精确源码`c5c021daf999fa1b4cff755b3633c106d88266f4`/[CI37932689731](https://github.com/bboytang/CAIdex/actions/runs/37932689731)三平台已完整验收，不借旧CI；checker `/tmp/caidex-qwen-tools-local-check.py`及local-result.json，最终日志workspace-third-turn-fixed/clippy-final。证据`/tmp/caidex-qwen-tools-*`与[HANDOFF](../HANDOFF.md)。旧a3e7e6d/CI37929549601不代验本步，custom/Lite/实际Qwen Runtime/商业Live/Full未验。

精确c5c021daf999fa1b4cff755b3633c106d88266f4/[CI37932689731](https://github.com/bboytang/CAIdex/actions/runs/37932689731)整体completed/success。Linux113827035486、Windows113827035140、macOS113827035343各17steps成功或条件跳过，完整raw2088/1774/1785行；workspace481/476/480（0failed，ignored52/50/50）、既有固定Runtime50/49/49（0failed/ignored）、Qwen43及DeepSeek57每名每平台一次。全通过名532/525/529精确等于旧37929549601集合+11新名，无遗漏/重复；watch73323 exit0，状态/完整原始日志下载及逐名checker通过，全部句柄结束。checker `/tmp/caidex-qwen-tools-ci-check.py`及ci-result.json，日志`/tmp/caidex-ci-37932689731-{linux,windows,macos}-raw.log`及status.json/watch.log。此Runtime回归不是实际Qwen Runtime接线或商业Live/Full验收。

## 显式custom→function与v3历史（当前子阶段）

2026-10-09核对官方Responses页面：原生function声明/结果契约明确，SSE列表包含custom input事件，但未完整建立generic custom声明、约束及回放契约。不能声称原生custom一定不存在，也不能据事件名称直接开放。采用CAIdex明确的`with_custom_tool_mapping()`：复用当前ToolMap、绑定历史与Custom传输，不新建HTTP层/Agent/执行器；默认、单独history/native_tools仍拒绝custom。

- 开启该策略同时启用native_tools/history。根/namespace内custom可无format或使用text、lark/regex grammar；格式字段、非空definition、名称、重复身份/alias、selector类型均校验。编译为native function，仅接受`{input: string}`，required input、additionalProperties=false；格式与原说明只进入指导文本，**不承诺服务端grammar强制约束**。strict/parameters等无效custom字段、deferred和parallel=false仍拒绝。
- 指定custom选择器及allowed_tools复用实际function选择策略，required仍单项；不能用function身份冒充custom或反向冒充。返回验证必须是已授权native function，arguments必须准确包含一个string input，拒绝额外字段、坏JSON、未授权name/namespace/ID、非完成调用与重复call_id。验证通过后还原原custom_tool_call/input/name/namespace，普通function保持原形；Unicode、换行、空格、引号/反斜杠和未知native扩展保留。
- direct custom调用/结果按同call_id和源类型编译为native function对；孤儿、错类型、重复、未完成、媒体结果拒绝。纯文本结果保持既有换行拼接/紧邻配对；回放与显示不执行工具。真实Runtime负责freeform解析、安全审批和执行，模型指导不放宽其规则。
- 新载体`caidex.qwen.native-history.v3:`绑定`custom_as_function=true`、原始tools/format/choice/parallel、native schema及完整请求/结果/显示组，SSE保留raw chunks；继续绑定执行端/Profile/Endpoint/model/前缀。v1/v2/v3双向拒绝策略混用，不自动升级。载体仍是敏感明文JSON，不是加密或来源认证，不进入账户Memory/云同步。
- 复用已有SSE校验/终态交付：参数流暂扣，完整completed重建后才产生投影custom调用和成功载体；raw原生custom事件仍作为未验证契约拒绝。坏流/半流取消/Drop/截止/预算溢出不交付工具，真实socket断开/slot释放；源和编译/载体预算及能力门控保持，Gateway token/Runtime attribution不去native，模型Key只在执行端Broker。

新增11项（含中断前1项），共54项，全部隔离loopback/合成Key：JSON/SSE及raw chunks、序列化三轮、text/lark/regex/namespace/choice、18种非法source、15种坏native JSON/SSE、15种坏参数流/raw custom及slot、18种policy/载体/显示篡改与v1/v2/v3双向隔离、direct结果、取消/Drop/deadline、源/编译预算/能力/error、Gateway两轮凭据隔离。中断前RED为编译成功实际qwen_unsupported_tools，44项GREEN仅早期定向；本次定向32610为54/0/0，首次Clippy94766仅范围pattern风格失败，等价改为1..=3后60531 exit0（全workspace/all-targets-D warnings）。最终workspace52456 exit0：492/0/52，原481通过名+11精确保持，Qwen54每名一次；8任务路径/16Markdown/66本地链接/22锚点、非任务tracked文件/依赖/workflow/CLI34/A–R/旧CI保留及fmt/diff通过。证据/tmp/caidex-qwen-custom-resume/{first,workspace-final,clippy-final}.log及local-check.py/local-result.json；精确源码三平台CI尚待，旧43项CI不代验。本步不改Core/Custom/Gateway/Runtime/依赖/workflow，Lite/实际Qwen Runtime/商业Live/Full仍待。

## 后续实施顺序

1. 本步14项、workspace/Clippy及3638d13精确三平台CI已完成；保留证据，不重复已验基础适配，不借此授Full。
2. 显式Runtime attribution/正文控制和逐模型推理参数21项、workspace/Clippy及311224b精确三平台已验；原生summary绑定历史32项及a3e7e6d/CI37929549601精确三平台已收尾；原生function/namespace、tool choice与成对结果及v2工具历史43项和c5c021d/CI37932689731精确三平台已收尾；本轮显式custom映射/v3历史54项/最终workspace492/0/52、Clippy及最终本地核对通过，尚待独立源码/精确CI；收尾后继续Lite与实际固定Runtime。不盲复制DeepSeek明文content或Gemini签名契约。
3. 单独核对Lite/Code Mode、并行与本地交付策略及native能力边界；固定Runtime的summary/context/include等组合目前仍拒绝，接线前需单独核对和显式适配，再接固定真实Codex Classic/Lite审批/执行/取消/磁盘恢复测试；实验/网页/模型服务端工具不得冒充Runtime工具。
4. Qwen之后OpenRouter，再按V3推进H/I/Windows/SSH/iOS/CLI/Relay/R；生产Host审批竞争/持久化在H，GUI/账户仍按既定阶段。真实商业模型测试需明确授权，本步不读用户Key/下载模型或部署。
