# CAIdex Ollama Provider：设计与验收

阶段 F/G，沿 V3；本轮是经典 stateless Responses 的离线协议阶段。代码在 `model/providers/ollama`，stateless/show/thinking/native history/图片/非严格格式累计32项及完整回归通过，最新源码ad86a34/[CI37767000333](https://github.com/bboytang/CAIdex/actions/runs/37767000333)三平台已验收，各阶段范围/证据见后续节。尚未连接真实 Ollama daemon/模型，也未验证固定 Runtime 使用本 Adapter，不授予任何模型 Codex Full/Compatible。

## 原生依据与当前范围

2026-10-08 核对 [官方兼容文档](https://docs.ollama.com/api/openai-compatibility) 与 [固定官方源码 e3cddc3e](https://github.com/ollama/ollama/blob/e3cddc3e897d8414a60a46e23f5ef3a99be2eb81/openai/responses.go)。文档标明 Responses 从 v0.13.3 加入，只有无状态请求；本机部署需另行确认实际版本/模型。文档中“Stateful requests”列表项与明确不支持 previous_response_id/conversation 的说明矛盾，以后者及实现为当前边界。

原生实现解析文本、function call/result；部分控制会被忽略或仅回显。工具 choice/并发数量、strict、推理 effort、媒体、动态工具等不能只因字段被接收就认领保证。当前 Adapter 限定文本和扁平 function 工具，不否认新版本本机服务已有其他能力；后续按实际版本与模型验收再开放。

## 执行端配置与共享实现

- `OllamaConfig::new(base, credential)` 要求显式 `/v1` base，例如 `http://127.0.0.1:11434/v1`；支持路径前缀，无 URL 猜测、端口探测、模型下载或环境 Key 搜索。
- 无认证时 GET/POST 不添加 Authorization，不读取 Broker。明确配置 Bearer 时 reference 必须 `provider=ollama`，ApiKey/AccessToken；owner/profile/kind 继续由执行端 Broker 隔离。无需伪装 OpenAI 凭据，不导入 `ollama signin` 状态。
- URL/TLS/limits/cancellation/slot/HTTP/SSE/错误脱敏复用现有 Custom Responses；明文只允许 literal loopback，HTTPS 校验保持，无代理/redirect/POST 重试。`ClientOptions` 延续显式信任根，未新建 HTTP 栈或执行器。
- 原 OpenAI Models parser 移入 Custom crate，由 OpenAI 重新导出同一 `NativeModel`，Ollama 共用；OpenAI 校验和公开类型行为保持。只移动 serde 依赖归属，未引入/升级第三方包。
- 原生 `/models` ID/object/created/owned_by 及未知元数据精确保留、按 ID 排序、拒绝重复/坏格式；Ollama 的 created 是修改时间，不是推理证据。`list_models` 只取执行端显式配置与目录的交集，source=ProviderCatalog；本地 metadata/capabilities/credential_requirements 不联网或读秘密。不从模型名猜能力，Full/Compatible 仍需真实 LiveRuntime 报告。
- 配置只允许 Classic；Lite route 在构造时拒绝，调用端 Lite 在网络前拒绝。Gateway 可以通过既有 `start_with_provider` 注入该 Adapter，启动时不读 Key/发推理。

## 请求编译与明确拒绝

接受 input 字符串或按顺序排列的 user/system/assistant 文本消息；文本可为 string 或 input_text/output_text 数组。允许完整 native message 的 id/completed status、空 annotations/logprobs 回放。instructions 保持字符串和位置；temperature 0–2、top_p 0–1、正整数 max_output_tokens 保留。数值范围是此阶段配置边界，不声称覆盖 Ollama 所有扩展。

工具仅接受唯一名称的扁平 function 声明和 object parameters，可带 description/strict=false/defer_loading=false；schema 与 arguments 原文只经 JSON 编码，不由 Adapter 改写，也不承诺模型严格遵守 schema。历史 function_call 必须有 name/call_id/object arguments，结果按尚待结果的 call_id 配对且无重复/遗漏；完成配对后允许后续轮次复用该 ID；文本结果保持原值/顺序。整个历史前缀留在每个无状态 POST 中，Adapter 不执行任何函数。

明确 false/null 的 store/background 删除为原生前台无存储默认；显式 tool_choice=auto、parallel_tool_calls=true 删除为原生默认允许。这几项是语义等价的已知缺省控制，不处理任意未知字段。true storage/background、required/none tool choice、parallel=false、previous_response_id/conversation 在 Key/POST 前拒绝。

同样拒绝未编译的 reasoning summary、include、metadata/cache/service tier、truncation、developer role、custom/namespace/web/tool_search、strict/defer=true、未映射附件、未知 input/嵌套字段。thinking控制、显式native history及媒体/格式的新范围见后续节；默认仍不接受reasoning历史。收到无法回放的 output 仍完整保留给调用方；下一请求明确报错，不能偷偷删 history 继续。既有 ModelCapabilities 中明确 Unsupported 的 text/native_tools/streaming 拦在网络前，Unknown 不自动转 Supported。

非空 RequestContext headers 当前全部拒绝，包括实际 Runtime 的本地 headers；服务端 x-codex-turn-state 亦在交付前拒绝，不假造 native session/state。context/header 放置、developer/system 语义和实际 Runtime 支持是下阶段需验证的接线，当前 Gateway 协议 fixture 不能代替它。

## 回复、状态与取消

JSON/SSE 都委托同一已验 bounded client；response/items/usage/未知字段/大整数/参数字符串保持，HTTP 200 failed/incomplete 不转换成功，流需真实 typed terminal，EOF/[DONE] 不合成 completed。SSE 原 data 在完整事件边界保留。

原生 reasoning 的 encrypted_content 在所核对 Ollama 实现中是明文 thinking；默认仅保留native wire，显式native history的新绑定/回放范围见末节。均不宣称加密、签名、隐藏思维或跨 Provider 可复用。

已取消/过期与不支持请求在 SecretStore/网络前返回安全 code；流 Drop/取消/整体 deadline 关闭 native socket，释放唯一 in-flight slot。共享 client 的同步 SecretStore 读无法强制中止、有限背压/TLS 等既有边界不变。

## 当前验证与恢复点

- `tests/provider.rs` 10 项实际 loopback 回归：显式配置/错误凭据种类/Lite；模型目录/六方法/无认证与 Ollama Bearer；函数调用结果和 native 文本三轮精确 POST 回放；typed SSE/原生明文 reasoning；26 组语义拒绝分别 JSON/SSE；坏参数/历史配对/声明能力；取消/过期/错误 owner/slot/Drop 与实际 EOF；Gateway token 隔离/提前拒绝；坏目录/HTTP/429/redirect/turn-state；failed/incomplete/截断。
- 门控测试先在无 guard 的合法运行版本失败（错误控制成功 POST），加统一 compiler 后通过；`/tmp/caidex-ollama-guards-{red,green}.log`。自查另以第三轮合法复用已完成 call_id 的精确回放复现误拒绝（`/tmp/caidex-ollama-reused-call-red.log`），pending 配对最小修复后完整重验已GREEN；其他新增协议测试只认领通过，不夸大独立 RED。
- 当前本地 10/0/0；日志 `/tmp/caidex-ollama-provider-final.log`。完整 workspace322/0/39、现有实际 Runtime37/0/0、Clippy -D warnings/fmt/diff已通过，精确源码三平台结果见下方收尾证据，不用旧 Gemini CI 代证新代码。

下一步：接 Ollama 模型 `/api/show` 能力/推理、native history/媒体/结构/工具路径及固定 Runtime，随后兼容厂商 DeepSeek/Qwen/OpenRouter。DeepSeek `/models` 当前不保证 created，因此不能照搬此目录 parser；Responses developer/input/扩展语义须按其官方契约单独编译。继续离线，不读取用户模型 Key、下载模型或发付费调用。整体 F/G、生产 Host/Chat/UI/iOS 尚未完成。

本地环境曾因39G磁盘满中断实际Runtime构建，新测试文件写入受截断；仅清理本项目target可再生缓存释放16.9GiB并恢复测试，全部10个名字核对后重新跑完整检查。中断运行不计通过，恢复后完整本地复验已通过，最终日志 `/tmp/caidex-ollama-{provider-final,workspace-final,all-real-final,clippy-final}.log`；三平台结果如下。


## 精确源码三平台收尾

源码 `95b0f6b3101b39e891b6ab30bb097665fced8bc0` / [CI37719887330](https://github.com/bboytang/CAIdex/actions/runs/37719887330) 的 Linux、Windows、macOS 三job均completed/success，全部step成功或条件跳过；watch exit0，三job日志下载exit0。逐名核对Ollama10/OpenAI11/Custom7/Google90及全部实际Runtime每个名字各一次：workspace Linux322/Windows317/macOS321，0失败、ignored39/37/37；实际Runtime Linux37/其他36，0失败/0ignored。Linux隔离native credentials1、fmt/Clippy/schema/doctor均通过。此次37/36是既有Runtime回归，不代表Ollama实际Runtime接线已支持。

日志 `/tmp/caidex-ci-37719887330-{status.json,watch.log,linux.log,windows.log,macos.log}`，逐名脚本 `/tmp/caidex-ollama-ci-check.py` exit0。阶段源码/依赖/最终差异已自行核对；未派新子agent审查，不把自查写成独立审查。Ollama高级控制、native reasoning归属/回放、实际daemon/模型与商业Full仍未验。

## 模型详细信息与精确推理控制（已三平台验收）

2026-10-08继续核对[官方模型详细信息](https://docs.ollama.com/api-reference/show-model-details)、[thinking声明](https://docs.ollama.com/capabilities/thinking)以及固定源码[e3cddc3e thinking类型](https://github.com/ollama/ollama/blob/e3cddc3e897d8414a60a46e23f5ef3a99be2eb81/model/thinking.go)、[Responses编译](https://github.com/ollama/ollama/blob/e3cddc3e897d8414a60a46e23f5ef3a99be2eb81/openai/responses.go)。show是POST；声明缺省不能推断无thinking；未声明named值可能回退default，因此只按明确声明编译。

- `OllamaConfig::with_show_endpoint`须显式完整同origin地址（scheme/host/port匹配），如`http://127.0.0.1:11434/api/show`；不猜/剥`/v1`或代理前缀。默认无show端点，不会自动请求daemon。`show_model(alias, context)`只发送配置绑定的native model，body不带verbose，不允许响应的remote_host选目的地/凭据。
- 复用Custom新增bounded `post_json`及GET共用实现：同TLS/Auth/slot/时间/取消/HTTP脱敏/no redirect/no retry；body大小在读Key之前限制，返回JSON受响应大小限制。show与目录/推理共用单一in-flight池；Drop/取消/timeout关闭socket，释放slot。
- `ModelDetails`保留capabilities、thinking.values/default、template/parameters/model_info及未知原文/大数/高精度小数，Debug不显示内容。有效descriptor须非空、bool或非空string、无重复，default在values内；坏descriptor/明确error返回静态安全502。Go nullable metadata map允许null；缺省字段维持Unknown。`[false]`为无thinking；unknown capability名称保留但不自动授予新功能。
- `with_model_details`安装执行端拥有的固定snapshot，alias须配置存在、native model精确相同、不可重复安装；本地metadata/capabilities与list交集共用声明。已显式Unsupported不放松；catalog声明仅为原生能力，vision声明不代表当前Adapter可编译图片，streaming/parallel/结构/搜索仍不从目录推断。model_info的架构context_length不冒充实际num_ctx；兼容性报告不生成/升级。
- `reasoning.effort`只有精确advertised named值可转native `think`；`none`仅在声明false时转false。无minimal→low/xhigh→max别名；布尔开关只接受明确`think=true/false`，不冒充high等精细等级。直接think同样要求values含精确值；两种非空控制同时出现拒绝，不依赖native覆盖优先级。
- 不请求控制、think=null、reasoning=null/空对象/effort=null保持native默认，不强制合成开关；summary/未知reasoning字段拒绝。明确reasoning Unsupported拒绝开启/等级，但允许声明false时关闭；无控制不会替执行端改native默认。所有无效/未声明控制在Broker/POST前拒绝，JSON/SSE走同一compiler。
- snapshot仅绑定模型ID，是可信执行端配置输入；不是签名、来源认证、digest/version锁或实时刷新。服务端同名模型变更后声明可能过时，需要执行端重新查询并重建Provider；不能据catalog/fixtures授予LiveRuntime/Full。

`tests/models/mod.rs`复用既有loopback harness新增6项：metadata精度/未知/坏descriptor；同origin/path/native ID/无认证及Bearer；JSON/SSE exact等级和禁止fallback；bool/default/Unknown/Unsupported/重复绑定；POST大小/坏JSON/HTTP/重定向/脱敏/no retry；预取消/过期/headers/未知alias、同推理slot、取消/Drop/header deadline与body idle。现有10项保持通过，本地16/0/0；有效RED（旧compiler拒绝high）→GREEN日志`/tmp/caidex-ollama-thinking-{red,green}.log`，最终定向日志`/tmp/caidex-ollama-show-provider-final.log`。首次完整构建发现测试文件误识别为独立入口，已移入子模块后完整重验，该失败不计通过。workspace328/0/39、旧实际固定Runtime37/0/0、Clippy -D warnings/fmt/diff均通过，日志`/tmp/caidex-ollama-show-{workspace-final,all-real-final,clippy-final}.log`；Ollama16个名字在workspace各一次。

源码`a1f7d6c27c33abf02af3d36fea0dbb3a0e926a75`已提交/push，[CI37722533837](https://github.com/bboytang/CAIdex/actions/runs/37722533837)精确head三job全部completed/success，每个step成功或条件跳过；watch exit0、三日志下载exit0、逐名脚本exit0。Ollama16/OpenAI11/Custom7/Google90及所有旧实际Runtime名字逐平台各一次。workspace Linux328/Windows323/macOS327，0失败、ignored39/37/37；旧实际Runtime Linux37/其他36，0失败/0ignored；Linux native credentials1与fmt/Clippy/schema/doctor通过。37/36是既有Runtime回归，不是本Adapter新Runtime证据。

日志`/tmp/caidex-ci-37722533837-{status.json,watch.log,linux.log,windows.log,macos.log}`，逐名脚本`/tmp/caidex-ollama-show-ci-check.py`（传run与精确SHA）exit0。自行检查最终源码/依赖差异，无新增第三方依赖；未派新agent审查，不把自查写成独立审查。

show阶段结束时的恢复点：native reasoning历史归属/回放（现已完成当前协议范围，见下节）、媒体/结构/工具路径和固定Runtime，随后其他兼容厂商。fixed Responses源码将reasoning.encrypted_content直接作为明文thinking附给下个assistant/function call，末尾形成thinking-only assistant；须在既有模型/前缀绑定模式下校验多轮顺序并完整回放，不能直接接受他方载体或宣称加密。未连接真实daemon/模型，未下载模型或调用商业API，整体F/G及H–R未完成。

## 原生历史、JSON/SSE回放（三平台离线通过）

- `with_native_history()`显式启用，默认行为保持。仍只Classic，不增加Lite/Runtime/context/developer/media等支持，不生成兼容性报告。代码`src/history.rs`与`history_stream.rs`；共用现有Custom HTTP/TLS/Broker/socket/slot，不新建worker或执行器。
- 敏感JSON载体前缀`caidex.ollama.native-history.v1:`，保留实际native model请求、完整native回复、SSE所收到的JSON事件及未知字段/大数/arguments字符串。profile绑定归一化base（含代理前缀）与凭据引用owner/provider/profile/kind，无秘密值；同时核对native model、实际compiled prefix和完整display group。此为执行端配置/结构校验，不是来源认证、签名、加密、模型digest/version锁或实时证据；可信Host存储/访问控制仍留H/I。
- 仅model/input/instructions/tools绑定前缀，温度/think/stream等新轮控制可改变。简单string input只规范为user文本消息，其他表示严格匹配，不自动重写argument JSON或未知字段。执行端先编译已恢复native前缀，再与旧请求比较；不能拿capsule自己的request当expected。端点/凭据引用/native model/工具/历史前缀改变、删改/重排display、外来版本/明文reasoning在Key/POST前拒绝；允许Runtime省略有效item ID/completed status，语义数据不变。
- display以一个reasoning carrier及原native非reasoning输出构成，原reasoning顺序、字段、明文thinking全部留在载体；summary只是合并的原生展示。回放恢复原始response.output而非渲染文本，载体不发给native endpoint。未映射的native output/未知输入字段虽然保留，仍按compiler显式拒绝，不因载体被校验就授予新语义。
- thinking必须附给正确assistant/function；consecutive reasoning或跨user/tool result的歧义拒绝。思考-only回合在恢复组末附空assistant封闭，避免native pending把它挪到后来的user之后。schema/原文/status仍保持；不承诺模型遵循function JSON schema，不执行工具。
- SSE重写序列/output index并保留text/summary增量；工具arguments/done与完整载体等typed terminal。记录的output_item.added身份及done原文须与最终output一致，chunks须重构同一terminal；坏/截断/不一致不交付可执行done或载体。failed/incomplete保留真实终态、不给function done；完整native回复仍在载体中。Drop/取消/截止释放共享socket/slot，待交付事件亦受取消/绝对deadline约束。
- 沿Custom现有契约在typed terminal后停止读取native；没有HTTP clean EOF/终态后新chunk验证保证，不冒用Google的EOF证据。记录的是JSON事件值，不是SSE注释/id/retry/字节空白的存档；原native JSON字段/精确数值仍留在载体。
- capsule、展开后的请求、投影回复受request/response预算限制，流还受累计native JSON/逐帧预算约束；过限安全失败，debug省略wire。非法caller history为400，native坏载体/不一致为安全502；没有POST自动重试。full prefix快照有O(n²)会话增长，预算封顶，H存储将来需要时可去重。

新增10项（共26）：codec JSON/绑定/display/原文/精度/坏version/JSON&SSE终态/chunks；实际HTTP JSON3轮精确POST和混入拒绝、thinking-only封闭与拒绝歧义；SSE2轮精确回放/载体先于工具/重写序列、无native reasoning时文本进度与index、失败/截断/不一致/超帧不放工具、取消/Drop/busy。stream保护有合法RED（旧SSE交付plainthinking）→GREEN，日志`/tmp/caidex-ollama-history-stream-{red,green}.log`；其他新增检查只认领通过，不夸大TDD。Clippy发现新测试不必要Vec及单分支match，最小修正后已通过。

当前本地Ollama26/0/0、workspace338/0/39、旧实际Runtime37/0/0、Clippy -D warnings/fmt/diff通过；Ollama每名在workspace各一次。日志`/tmp/caidex-ollama-history-{codec,json,provider-final,workspace-final,all-real-final,clippy-final}.log`。源码ad3a49176b4dc6044454d800a09b4c7131dc9d86已提交/push，[CI37728309340](https://github.com/bboytang/CAIdex/actions/runs/37728309340)精确head三平台已验收；前一阶段a1f7d6c CI不代证本轮；未派新agent审查。真实daemon/模型与本Adapter固定Runtime仍未验证，不认领商业Full。

下一恢复点：按已确认顺序补媒体/结构/其余工具及固定Runtime（包括真实Runtime会变动的item表示/本地headers/developer/include/summary）。不提前忽略差异以冒充接线成功，随后兼容厂商与H–R。

本轮CI收尾：精确源码head `ad3a49176b4dc6044454d800a09b4c7131dc9d86`，三个job均completed/success、全部step成功或条件跳过；watch exit0、日志下载exit0、逐名脚本exit0。Ollama26/OpenAI11/Custom7/Google90及旧实际Runtime每名逐平台各一次。workspace Linux338/Windows333/macOS337，0失败、ignored39/37/37；旧实际Runtime37/36/36，0失败/0ignored；Linux native凭据1与fmt/Clippy/schema/doctor通过。仍不是本Adapter新Runtime、真实daemon或Full证据。

日志`/tmp/caidex-ci-37728309340-{status.json,watch.log,linux.log,windows.log,macos.log}`及逐名脚本`/tmp/caidex-ollama-history-ci-check.py`。第一次逐名检查发现macOS CLI缓存protocol段提前截断；直接下载job113151419136完整原始日志，保留`macos-{raw,cli-truncated}.log`，仅以原始完整protocol段替换对应段后重验成功，不把首次不完整检查计为通过。本次收尾仅文档，代码验收对应上述源码head。


## 内联图片与非严格结构输出（三平台离线通过）

- 依据固定官方e3cddc3e的[Responses转换器](https://github.com/ollama/ollama/blob/e3cddc3e897d8414a60a46e23f5ef3a99be2eb81/openai/responses.go)及[图片解码器](https://github.com/ollama/ollama/blob/e3cddc3e897d8414a60a46e23f5ef3a99be2eb81/openai/openai.go)：图片只支持内联Base64，FileID无映射，远程URL拒绝，detail不参与转换；native text.format仅json_schema.schema进入ChatRequest.Format，json_object和strict不会实施。官方[兼容说明](https://docs.ollama.com/api/openai-compatibility)/[结构输出](https://docs.ollama.com/capabilities/structured-outputs)辅助核对；不以文档能力清单代替固定源码或真实模型证据。
- `with_images()`显式启用message content/function_call_output内的input_image；PNG/JPEG/JPG/WebP及native空MIME的data URI仅校验标准Base64和非空/字节预算，不重编码/改写原文、字段、文本/图片次序。不是图像解码/真实模型识别验收，MIME/实际像素格式仍由native image processor处理。外部URL、file_id、其他媒体类型、未知字段、low/high/original detail拒绝，auto/省略/null为已知原生默认。Adapter不抓取远端图片或读取本机路径，不新增上传服务。
- `with_structured_output()`显式启用原生非严格json_schema；name按canonical名称形状校验，schema须object，schema/约束/引用/未知注释/精确数值原文保留；严格Schema语义验证和生成后的强校验未完成，nonstrict仅原生best-effort，不宣称所有约束被模型执行。该阶段strict=true拒绝（本次严格交付校验见下节），false/省略/null允许；wrapper description非null、verbosity/未知text字段拒绝，schema内description不变。Plain text格式及空/null text为已知默认，不要求opt-in。
- `json_object`编为`json_schema` + object schema，使格式实际进入原生grammar而不是被Responses转换器忽略；本地请求/载体记录此实际compiled wire。显式vision/structured_output Unsupported不被opt-in放松，Unknown不伪改Supported；catalog/fixtures不授予Full。
- JSON/SSE共用compiler及Custom HTTP；完整body含所有encoded图片/schema在Broker前受request_bytes限制，Base64单项亦有上限，因此累计decoded数据受同一预算封顶。回放恢复原始图片/工具结果，既有v1载体仍保存完整actual native request/response；text.format是可逐轮改变的生成控制，不强绑旧轮输出格式为新轮。响应格式/真实终态不变，没有执行器、第二HTTP栈或新第三方版本；仅关联已存在的base64=0.22.1。

复用loopback harness新增6项（Ollama共32）：5种native MIME输入在JSON/SSE完整POST保留；带图工具结果与格式改变的绑定回放；18组source/detail/Base64/格式拒绝分别JSON/SSE且Key/POST计数零；opt-in与明确Unsupported独立；有效大图片/schema的body预算前置；plaintext/空/null格式默认。有效RED（原compiler拒绝合法图片/格式）→GREEN，`/tmp/caidex-ollama-content-{red,green}.log`。初次锁关联遗漏已有base64多版本标识，通过Cargo --offline纠正为base64 0.22.1；该失败不计通过。

本地Ollama32/0/0、workspace344/0/39、旧固定Runtime37/0/0、Clippy -D warnings/fmt/diff通过；Ollama32每名在workspace各一次。日志`/tmp/caidex-ollama-content-{provider-final,workspace-final,all-real-final,clippy-final}.log`；源码ad86a343a9c5adf5cd2ab0f52065a780fcc8b714已提交/push，[CI37767000333](https://github.com/bboytang/CAIdex/actions/runs/37767000333)精确head三平台已通过，前一阶段CI不代证本轮。未做实际Ollama Runtime/daemon/模型下载/商业推理，未派新agent独立审查。严格输出、其余工具、context/developer及Lite按交接顺序继续，整体F/G及H–R未完成。


本轮CI收尾：源码`ad86a343a9c5adf5cd2ab0f52065a780fcc8b714`，Linux/Windows/macOS三个job均completed/success、全部step成功或条件跳过。逐名核对Ollama32/OpenAI11/Custom7/Google90及旧Runtime各平台每名一次；workspace344/339/343，0失败、ignored39/37/37；旧Runtime37/36/36，0失败/0ignored；Linux native credentials1、fmt/Clippy/schema/doctor通过。不是本Adapter新Runtime或真实模型证据，严格Schema与其余工具/实际Runtime仍待。

日志`/tmp/caidex-ci-37767000333-{status.json,watch.log,linux.log,windows.log,macos.log}`。三平台直接取job API完整`*-raw.log`、保留原文；`/tmp/caidex-ollama-content-ci-normalize.py`仅按实际Run命令添加step标签，`/tmp/caidex-ollama-content-ci-check.py`核对精确SHA/全部终态/逐名/数量，均exit0，watch和三日志下载exit0。本轮断开前git add因额度限制审核无法完成；恢复后先读HANDOFF/Git/完整差异与原始验证证据，修正交接中过时的阻塞解除状态，再经原审批链提交/push/CI，无绕过审批。收尾仅文档，代码对应上述SHA。


## 严格结构输出（三平台离线通过）

- 沿用`with_structured_output()`，默认及显式Unsupported仍拒绝；`text.format.json_schema.strict=true`启用本地交付校验，原生schema/strict wire不改写。Ollama固定Responses转换器只转发schema，不实施strict；本Adapter校验收到的完整输出，不保证模型必定生成合规结果，也不重试或修补模型回答。function工具声明的strict=true仍拒绝，wrapper description仍无映射。
- 仓库无通用实例校验器；使用固定`jsonschema=0.58.6`，关闭默认功能、只开arbitrary-precision，并强制Offline retriever。Schema在Broker/slot/POST前编译，失效或未解析引用返回安全400 `ollama_invalid_output_schema`；HTTP/file/custom元Schema外部读取被拒绝。标准draft由库识别，内建metaSchema、本地引用/递归及标准约束可离线求值；未知注释保持原文，未知format拒绝，已知format执行校验，库未支持的必需vocabulary不假认支持。精确整数/小数不降为f64；Regex回溯上限10,000。Cargo新增41包，无既有包升级/删除；现有hashbrown启用关联依赖，bit-vec/r-efi仅多版本标识更新。
- Completed assistant message的output_text按顺序拼接后解析完整JSON、逐消息校验，失败返回安全502 `ollama_invalid_structured_output`，错误不带Schema/回答/秘密。纯function调用轮次及明确refusal是单独结果，不声称其为合规JSON；refusal与非空文本混合、空/仅thinking终态、未映射output、错误message形状均拒绝。Failed/incomplete保留真实终态与部分数据，不伪造结构输出成功；Schema不用于校验工具arguments。
- JSON在原生历史投影/交付前验证完整回答，投影后再核对调用方取消/截止时间。SSE复用原HistoryStream及既有chunk/terminal一致性检查，无新HTTP worker/执行器；增量文本/思考是临时进度，所有done和function arguments等可执行交付等终态校验通过。history关闭时不制造载体、不改变native item/output indices；history开启保留原profile/model/前缀绑定。坏Schema输出、不一致done/terminal、截断、取消/超时/Drop及字节超限不释放可执行done，释放原transport/slot。typed terminal不是HTTP clean EOF保证。
- 字节/帧和Regex回溯有上限；Schema编译/求值是同步CPU工作，不承诺硬CPU抢占时间，进程隔离留Host安全策略。不是实际Ollama daemon/模型生成/新Ollama固定Runtime或Full证据。
- 新7项fixture：精确大数/小数、required/extra/format、local refs/循环/递归/draft/regex；原生历史回放及JSON原文；无Key/POST的外部/无效Schema；JSON&SSE calls/refusal/失败状态及歧义拒绝；临时文本/延迟工具/history开关；不一致工具/截断及native reasoning保留；pending取消/截止/Drop/预算与slot复用。首次测试文件布局/SSE stream遗漏及Context复制的编译问题已修正，不计作成功；有效RED编译成功后旧拒绝400，GREEN及全workspace通过。当前本地Ollama39、workspace351/0/39，旧固定Runtime37/0/0、Clippy/fmt/diff通过（最终交付guard调整后已复核）；日志`/tmp/caidex-ollama-strict-{red,green,workspace-final,clippy-final,all-real-final}.log`。源码`bd3995e5ed0536771710abc21ea33555efc075a0`已提交/push；[CI37770508610](https://github.com/bboytang/CAIdex/actions/runs/37770508610)精确head三平台成功，详以下收尾证据。

- 精确源码三平台收尾：CI37770508610的head与上述源码完全相同，3job及所有step终态成功/条件跳过；逐名原始日志核对Ollama39/OpenAI11/Custom7/Google90和既有Runtime各平台各一次。workspace Linux351/Windows346/macOS350，0失败，ignored39/37/37；固定Runtime37/36/36，0失败/0ignored；Linux系统凭据1、fmt/Clippy/schema/doctor通过。不是Ollama新Runtime/daemon或Full证据。
- 完整原始job日志：Linux113288652937（1884行）、Windows113288652900（1579行）、macOS113288652380（1591行），直接GitHub API下载、未截断。`/tmp/caidex-ci-37770508610-{status.json,watch.log,linux-raw.log,windows-raw.log,macos-raw.log}`及同名三平台step标注日志；`/tmp/caidex-ollama-strict-ci-{normalize,check}.py`仅按实际Run命令标注step、不修改原始payload，watch/下载/check exit0。跨机器以已提交文档和原CI为准，/tmp不保证保留。
- 下一步：其余Ollama工具路径及固定Runtime（item表示/本地headers/developer/include/summary），继续复用共享transport和现有历史codec，不把原生回显参数或fixture当LiveRuntime/Full。


## 固定 Runtime 请求入口（三平台离线通过）

- 先用固定 Codex 0.160.1 的真实 app-server 捕获经典/Lite 请求，未经过本Adapter，也未调用商业模型。经典请求含leading developer、本地归属字段、verbosity及custom apply_patch/tool_search/web_search；Lite还有additional_tools、namespaced custom exec、parallel=false、reasoning.context。捕获只证明实际请求表示，不授予Ollama兼容性。
- `with_runtime_context()`显式消费本地client_metadata/prompt_cache_key及session_id/x-client-request-id/x-codex-turn-metadata三个头，六方法中的目录/show/JSON/SSE传输均不转发这些头；不承诺持久化或原生cache。client_metadata须string值object、cache标记须非空且无控制字符；turn-state与未知头仍拒绝。
- leading developer按明确策略转换为system，保持内容/ID/顺序；会话开始后的developer拒绝。转换在native history前缀绑定之前，旧载体必须匹配当前执行端编译前缀。include仅允许reasoning.encrypted_content，summary仅auto、context仅all_turns，均要求native history；auto明确显示原生完整thinking，不承诺原生摘要长度或加密。精确effort继续按声明转换，Unsupported不放松。
- `with_verbosity_instruction(level, instruction)`仅允许low/medium/high的执行端明确指令，追加到instructions；不伪称native verbosity刻度。重复/空配置拒绝。verbosity映射变更影响compiled prefix，旧历史必须拒绝；format/strict校验仍独立保持。默认或未映射参数继续Key/POST前拒绝。
- 原始body在移除本地字段之前检查预算；展开指导和history后仍受已有native request预算约束。新增6项覆盖JSON/SSE精确wire、header/Bearer、回放绑定、拒绝/Unsupported、取消/deadline、双层预算及strict格式组合；定向6/0/0。有效RED使用旧header guard（编译成功后400拒绝），恢复实现后GREEN通过；最初fixture API/错误码断言问题已更正，不将失败构建计入验收。
- 复用真实Runtime Harness新增1项、三个真实请求模式：默认classic、本Adapter未支持Lite、partial context经典含未映射工具，全部turn failed且零凭据读取/native POST/审批；1/0/0。此项是Ollama Adapter真实Runtime负例，仍无正例、daemon或Live模型证据。只新增Runtime测试到已有Ollama crate的path依赖，lock只增加这一关联，无第三方升级。
- 日志`/tmp/caidex-ollama-runtime-request-{red,green,real-negative}.log`；直接wire捕获`/tmp/caidex-ollama-runtime-{classic,lite}-wire.json`及probe.log只保留本机，不将内部完整prompt公开归档。完整本地workspace357/0/40（Ollama45每名各一次）、固定Runtime38/0/0逐名各一次、Clippy全targets -D warnings/fmt/diff通过，日志同前缀{workspace-final,clippy-final,all-real-final}.log。源码`d934fe165d49b1638b1879d8e61ad32d60993c8f`已提交/push，[CI37775057618](https://github.com/bboytang/CAIdex/actions/runs/37775057618)精确head三平台已通过，详以下收尾证据。下一步custom/namespace/discovery/Lite item路径与正例，保持一个Runtime/共享transport，不用请求入口成功冒充完整接线。F/G及H–R仍未完成。

- 精确源码三平台收尾：3job全部completed/success、所有step成功或条件跳过；Ollama45/OpenAI11/Custom7/Google90及完整固定Runtime逐名各平台各一次。workspace Linux357/Windows352/macOS356，0失败、ignored40/38/38；固定Runtime38/37/37，0失败/0ignored；Linux隔离native credentials1及fmt/Clippy/schema/doctor通过。新增Runtime测试只证明真实请求拒绝边界，仍无Ollama工具正例、daemon或Full。
- 完整原始job日志Linux113303703210（1896行）、Windows113303703627（1584行）、macOS113303703461（1596行），直接GitHub API下载、未截断。`/tmp/caidex-ci-37775057618-{status.json,watch.log,linux-raw.log,windows-raw.log,macos-raw.log}`及同名step标注日志；`/tmp/caidex-ollama-runtime-request-ci-{normalize,check}.py`仅标注实际Run命令、不改payload，watch/下载/check exit0。跨机器以提交文档和原CI为准，/tmp不保证保留；本次收尾只三文档，不改变已验源码。


## 原生 namespace 与客户端工具搜索（三平台离线通过）

- 固定官方Responses转换器支持namespace function、client tool_search及其历史items；Model-visible search结果保留声明，Runtime执行发现/工具。`with_native_tools()`显式启用此范围并同时启用v1 native history；默认行为/Lite/custom/web未放松，不新增HTTP栈/执行器或第三方依赖。
- namespace wrapper说明原生忽略，编译时将说明加入成员description并移除wrapper说明，之后绑定compiled prefix；原始body及展开指导预算仍在Broker前检查。未知/嵌套namespace、strict/defer_loading=true、server search与未映射custom/web拒绝。保留原生namespace/member身份，不使用别名哈希或依赖另一Provider。
- 按固定源码的dot/underscore/colon解析规则检查原生别名碰撞，包括扁平工具、namespace成员和tool_search保留名。历史按顺序建立可用工具集；client search必须call/result配对且completed，新增声明随后可调用，重复同身份只允许完全一致声明。孤立/错种类/歧义结果、调用前尚未发现的工具、重复call ID在Key/POST前拒绝。无call_id结果必须唯一匹配pending function且ToolName为精确原生主名，冒号等解析别名不得冒充named result；有call_id的可选身份仍核对。
- JSON及共享history SSE终态前检查原生调用是否属于当前声明、arguments是否object、namespace/name是否是明确canonical身份、item/call ID是否唯一及call ID是否已出现。坏原生工具502安全错误，无工具完成/载体交付。新增search与旧function同样在failed/incomplete时不发送可执行done，保留真实失败终态；工具/载体仍等已验证typed terminal，不承诺clean EOF。取消/截止继续共享入口与交付guard，无新worker。
- 新6项fixture：namespace JSON/SSE两轮精确args/中文/大数与legacy result、说明与前缀变更；search JSON/SSE四轮发现/调用/重复声明及delta拒绝；默认/碰撞/Unsupported/错序/孤立/错种类/预算；坏原生工具和重复ID零done/载体；failed/incomplete search真实状态；capability/预取消零Key/POST。有效RED旧flat compiler拒绝namespace400，GREEN6/0/0；初次fixture枚举笔误及Clippy helper折叠已修正，不计作成功验收。
- 复用实际固定Runtime拒绝测试增加第四模式（context + native tools显式启用），完整真实默认请求仍含未映射custom/web，必须失败且零Key/POST/审批。四模式完整真实Runtime回归通过；没有Ollama Runtime工具正例、daemon、商业Live或Full，custom/freeform/Lite下一阶段继续。
- 日志`/tmp/caidex-ollama-native-tools-{red,green,workspace-final,clippy-final,all-real-final}.log`；最终本地workspace363/0/40（Ollama51逐名各一次）、固定Runtime38/0/0每名各一次、Clippy全targets-D warnings/fmt/diff通过；源码`40edb9a40975ade7690a946596e85a8fdb7cc56e`已提交/push，[CI37779554830](https://github.com/bboytang/CAIdex/actions/runs/37779554830)精确head三平台成功，详以下收尾证据。保持既定F/G及H–R范围，不以namespace/发现fixture代证实际模型能力。

- 三平台收尾：CI精确head 40edb9a，三个job全部completed/success、所有step成功或条件跳过；Ollama51/OpenAI11/Custom7/Google90及完整固定Runtime每个名字各平台各一次。workspace Linux363/Windows358/macOS362，0失败、ignored40/38/38；固定Runtime38/37/37，0失败/0ignored；Linux隔离native credentials1与fmt/Clippy/schema/doctor通过。Runtime第四模式仍只证明拒绝未映射请求，无Ollama工具正例、daemon或Full。
- GitHub API完整原始日志：Linux113318904267（1910行）、Windows113318904133（1591行）、macOS113318903828（1602行）；macOS排队后完成，无重启/重跑。`/tmp/caidex-ci-37779554830-{status.json,watch.log,linux-raw.log,windows-raw.log,macos-raw.log}`及同名step标注日志；`/tmp/caidex-ollama-native-tools-ci-{normalize,check}.py`只按实际Run命令标注、不改原payload，watch/下载/check exit0。跨机器以已提交文档/原CI为准，/tmp不保证保留。
- 下一步custom/freeform、deferred/Lite及固定Runtime正例：固定源码只声明DeferLoading而不实施可见性；native-only v1无法证明映射前custom kind/声明绑定，须复用已有codec模式增加明确绑定且保持旧v1，不自动升级/松绑。不以本轮namespace/search fixture代证全部默认Runtime或真实模型，整体F/G及H–R未完成。

## Custom/freeform 映射与 v2 历史（三平台离线通过）

`with_custom_tools_as_functions()` 显式启用 custom→function，并启用 native_tools/native_history。原始 flat 或 namespace custom 编成同名原生 function，只允许一个必填字符串 `input`，`additionalProperties=false`；namespace 指导沿用既有成员说明编译。text/缺省格式原样文本，lark/regex grammar 原文加入明确标记的指导，不承诺 Ollama constrained decoding 或 grammar 生成正确性。defer_loading=true、strict、自定义未知字段、web 和 Lite 仍拒绝；工具执行、grammar 解析与审批仍属于固定 Runtime。

v2 明文 JSON 载体新增 original tools 与按序 client search results 快照，同时核对实际编译的 native tools/发现声明、原生请求前缀和完整展示组。每个历史响应只绑定其请求前缀已有的发现子集，允许以后发现新 custom；重复声明须与原始声明一致。custom/function kind 变化即使生成相同原生 schema 也拒绝。v1 继续可用，两种 profile 不自动升级、交换或混用；需要新线程/关联分支。仍非加密、签名或来源认证。

JSON/SSE 在 typed terminal 前校验 custom arguments 为且仅为 `{input:string}`，再还原 custom_tool_call 和 input；SSE 使用 response.custom_tool_call_input.delta/done，failed/incomplete 不生成可执行 custom done。完整原生 arguments 字符串、工具名空间、未知字段及 SSE chunks 保存在 v2，回放不以重序列化的字符串替代。caller 自行提供的 custom call/result 单独编译，call/result kind、身份、顺序和唯一 ID 沿用既有 NativeTools 校验；custom result 要求 call_id，不能伪装成 function 或 legacy named result。仍共用 Custom bounded transport/Broker，无第二执行器、新依赖或新的 HTTP EOF 承诺。

复用既有 loopback fixture 新增7项：namespace/mixed function JSON&SSE三轮原生字符串回放、多行grammar/source/kind/展示与破坏载体拒绝；动态 custom 搜索4轮、发现子集及重复声明；坏 arguments 不交付工具/载体/成功并释放 slot；默认/声明/配对/原始与编译预算/预取消均零Key/POST；v1/v2双向拒绝；flat text custom 输入事件与failed/incomplete状态；服务端直接发送custom input delta/done必须拒绝且关闭native socket。后者复核发现共享stream默认分支会提前转发，新增第7项有效RED→拒绝守卫GREEN；日志 `/tmp/caidex-ollama-custom-native-event-{red,green}.log`。有效RED旧compiler400→GREEN，最终Ollama58逐名一次、workspace370/0/40、固定Runtime38/0/0、Clippy全targets-D warnings/fmt/diff通过。日志 `/tmp/caidex-ollama-custom-{red,green,workspace-final,all-real-final,clippy-final}.log`。补取消测试时误用不可Clone的RequestContext，已修正并完整重验，该失败不计通过。

本轮固定Runtime38项只为已有回归，尚无本Adapter实际工具执行正例；真实daemon/模型/Live/Full未验。首版8540ecd/CI37785263569被修正版替代并cancelled，不代验最终版本；修正版源码f481fc8f94a746a27f0cbb7f05227cfa43f1abbb已提交/push，[CI37787010246](https://github.com/bboytang/CAIdex/actions/runs/37787010246)精确head三平台已通过，随后deferred/Lite item转换及固定Runtime正例；整体F/G与H–R未完成。

修正版三平台收尾：精确源码f481fc8f94a746a27f0cbb7f05227cfa43f1abbb/CI37787010246三个job均completed/success，所有step成功或条件跳过，watch/下载/逐名脚本exit0。完整raw逐名Ollama58/OpenAI11/Custom7/Google90及固定Runtime各一次；workspace Linux370/Windows365/macOS369（0failed，ignored40/38/38），固定Runtime38/37/37（0failed/ignored），Linux native credentials1及fmt/Clippy/schema/doctor通过。raw日志 `/tmp/caidex-ci-37787010246-{linux,windows,macos}-raw.log` 共1911/1598/1609行，另有status.json/watch.log与step标注日志，脚本 `/tmp/caidex-ollama-custom-ci-{normalize,check}.py`。macOS排队后正常完成，未用未修复首版CI代验；未派新子agent审查。下一步deferred/Lite及本Adapter固定Runtime工具正例，真实daemon/Live/Full及整体F/G与H–R未完成。

## Deferred 可见性与固定 Classic Runtime MCP（本地通过，三平台待验）

`with_deferred_tool_search()` 显式启用 custom mapping/native_tools/native_history，并将policy保存到v2 source。复用既有编译器先验证完整catalog的parameters对象、strict和别名（隐藏声明也不能绕过）；root defer_loading=true成员从实际native清单移出，须有client tool_search，原生namespace可保留空成员。只有按序匹配的client搜索结果才开放工具；原生忽略的defer flag不透传。重复声明可改变此加载标记，其余kind/schema/description必须相同；original root和每个original search result分别保存、回放仍精确绑定原始flags。未启用policy继续拒绝defer=true；新policy不能与旧v2 profile互换，不自动升级v1。Web/strict/Lite范围未放松，无新依赖、HTTP栈、模型下载或执行器。

新增4项JSON/SSE：5轮隐藏→function/custom发现与原始arguments回放/重复flag变更；默认、无search、隐藏坏parameters/strict/未知字段/别名和提前调用零Key/POST；native提前调用不交付工具/载体并释放slot；相同native tools下policy及original flags变更仍拒绝。有效RED旧400→GREEN。同时实际Codex回放证明其会补reasoning.content=null，新增codec专项只将synthetic reasoning carrier此字段的缺省/null视为等价，数组/非空content/其他item仍拒绝，原生内容和未知字段不改写；旧400的有效RED→GREEN，诊断输出已移除。

固定Codex0.160.1新增Classic正例：执行端config显式web_search=disabled、runtime context/verbosity/thinking snapshot与上述policy；实际ToolSearch客户端发现local MCP echo，调用执行并保存模型原文结果，停app-server后按磁盘resume。4次真实Gateway→Ollama Responses POST和合成Key读取，首轮MCP只调用echo一次，重启不重复；每轮raw native reasoning/call/message、arguments原字符串及未知大数、落盘完整response均逐项精确核对，root工具/指令及旧native前缀不变，native不收到Lite头、归属metadata或载体字符串。复用已有隔离Harness、MCP和restart helper，fixture不提供真实模型；默认cached web/未启用Lite的4模式拒绝回归仍保留，不能据此授予商业Full或实际daemon通过。

本地Ollama63逐名一次、workspace375/0/41、固定Runtime39/0/0、Clippy全targets-D warnings/fmt/Python compile/diff通过。日志 `/tmp/caidex-ollama-deferred-{red,green,canonical-red,canonical-green,real-first,real-green,workspace-final,all-real-final,clippy-final}.log`。首次真实Runtime在null差异处失败后修复重验，失败不计通过。源码1a2668fc2f7688b316d8a4b361b22123c19089d2已提交/push；[CI37809165149](https://github.com/bboytang/CAIdex/actions/runs/37809165149)精确head的Linux/Windows全部17steps及完整日志已核对通过，workspace375/370、固定Runtime39/38（均0failed）；macOS前两次因hosted runner容量不足零steps取消，attempt3仍queued，三平台验收未完成；下一步Lite additional_tools/parallel=false与实际Code Mode执行/审批/取消，整体F/G、daemon/Live/Full及H–R未完成。
