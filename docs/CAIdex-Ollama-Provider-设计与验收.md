# CAIdex Ollama Provider：设计与验收

阶段 F/G，沿 V3；本轮是经典 stateless Responses 的离线协议阶段。代码在 `model/providers/ollama`，本地 10 项及完整回归通过，源码95b0f6b已push，[CI37719887330](https://github.com/bboytang/CAIdex/actions/runs/37719887330)三平台已验收。尚未连接真实 Ollama daemon/模型，也未验证固定 Runtime 使用本 Adapter，不授予任何模型 Codex Full/Compatible。

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

同样拒绝此阶段未编译的 reasoning history/summary、include、metadata/cache/service tier/text formatting、truncation、developer role、custom/namespace/web/tool_search、strict/defer=true、媒体/附件、未知 input/嵌套字段。thinking控制的新增范围见下节。收到无法回放的 output 仍完整保留给调用方；下一请求明确报错，不能偷偷删 history 继续。既有 ModelCapabilities 中明确 Unsupported 的 text/native_tools/streaming 拦在网络前，Unknown 不自动转 Supported。

非空 RequestContext headers 当前全部拒绝，包括实际 Runtime 的本地 headers；服务端 x-codex-turn-state 亦在交付前拒绝，不假造 native session/state。context/header 放置、developer/system 语义和实际 Runtime 支持是下阶段需验证的接线，当前 Gateway 协议 fixture 不能代替它。

## 回复、状态与取消

JSON/SSE 都委托同一已验 bounded client；response/items/usage/未知字段/大整数/参数字符串保持，HTTP 200 failed/incomplete 不转换成功，流需真实 typed terminal，EOF/[DONE] 不合成 completed。SSE 原 data 在完整事件边界保留。

原生 reasoning 的 encrypted_content 在所核对 Ollama 实现中是明文 thinking，当前只保留收到的 wire；未实现此项的安全历史归属/回放，不宣称加密、签名、隐藏思维或跨 Provider 可复用。

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

下一恢复点：native reasoning历史归属/回放、媒体/结构/工具路径和固定Runtime，随后其他兼容厂商。fixed Responses源码将reasoning.encrypted_content直接作为明文thinking附给下个assistant/function call，末尾形成thinking-only assistant；须在既有模型/前缀绑定模式下校验多轮顺序并完整回放，不能直接接受他方载体或宣称加密。未连接真实daemon/模型，未下载模型或调用商业API，整体F/G及H–R未完成。
