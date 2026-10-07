# Anthropic 原生 Provider：实现与恢复点

## 当前范围

`model/providers/anthropic` 已实现原生 Messages/Models、HTTP/SSE client、Responses 请求转换、回复增量投影和版本化历史回放；AnthropicProvider 六方法承接现有推理/结构化输出/Runtime 字段门控。本轮接入执行端本地上下文策略和原生响应关联头，验证状态见 HANDOFF.md。Gateway、请求前缀/账户绑定及完整 Runtime 互操作尚未完成。

`AnthropicConfig` 固定执行端 API Key 引用（provider=anthropic、kind=ApiKey）、基址和可选 workspace。默认 HTTPS；显式代理/本地 fixture 复用既有 endpoint 安全策略，仅 literal loopback 允许 HTTP，不接受 URL 用户密码/query/fragment。HTTP client 保持 TLS 验证、禁用代理自动发现、重定向和自动重试；支持显式额外信任根。

Broker 只在发送时解析指定引用，不自动读取环境密钥。发送 `x-api-key`、`anthropic-version: 2023-06-01`，workspace 由执行端配置，均不来自请求 JSON。认证值标记 sensitive；错误只返回静态分类和安全 Retry-After 元数据，不传播 URL、原始 HTTP 错误或第三方错误 body。原生消息和事件是调用者数据，不是已脱敏日志；其 Debug 隐去 wire。

`discover_models(max_models, context)` 访问配置基址下的 Models 路径，使用 `limit=1000` 和 URL 编码的 `after_id`。完整分页共享单个并发许可、总 deadline、累计响应字节预算；重复 ID、坏游标、不完整结果和数量上限报错，不返回部分模型清单。能力仅由 Models 已声明的 image_input/thinking/structured_outputs 支持标记及正数 token 上限映射；缺失/null/零上限保持未知。名称不推导能力或 Codex 兼容性。

`create_message(model, native_json, context)` 发送非流式原生 Messages JSON：明确 max_tokens，固定 model 参数覆盖 body model，stream=false；保留原生消息内容及未知字段，限制请求/响应大小。它不是 Responses 请求转换，未全面验证提供商的所有输入 schema，其他参数是否合法仍由原生服务判定。context headers 默认拒绝；执行端显式启用本地 Runtime 策略时仅允许下文三项本地字段，不外发。并发、认证/header timeout、响应 idle timeout、总 deadline、取消和 drop 不触发重试；已开始的同步 SecretStore 读取不能强制中止，取消后不发送请求。

原生回复按 content 原顺序保留，包括 thinking/signature、redacted_thinking、工具及未来块；普通回放直接使用原内容，原生 fallback 的 echo 过滤见下节，完整 wire 始终保留。签名不做密码学验证，缺少完整签名的 thinking 当前拒绝回放。SSE 增量重建文本、签名、工具 JSON 和 citations，严格校验 lifecycle/index/usage；usage 累计值替换而非相加，换模型时已知计数重新归属。未知事件/块保留；无法重建的未知 delta 在 block stop 报错，避免伪造无损 history。`Completed` 只表示收到完整 message_stop，具体 end_turn/tool_use/max_tokens/pause/refusal 等由 MessageOutcome 区分。

`stream_message` 与非流式共用认证、HTTP 状态和 media-type 检查，使用 stream=true 和 text/event-stream。返回原生事件流，只有解析器完成 message_stop 后才提供独立 Completed(NativeMessage) 数据项；保留原生签名、工具内容和累计 usage，不执行工具。原生 error 事件被替换为静态 ProviderError，不输出第三方诊断文本、URL 或原始 frame。

事件队列容量为 1，读取和发送均受取消/总 deadline 约束。I/O worker 持有并发许可；流无人读取时也会因超时/取消关闭 socket、释放许可，并独立保存错误，待已有队列项排空后交付。Drop 终止 worker。正常完成主动关闭上游连接，不等待 HTTP EOF；截断、非法 SSE、超限和错误不生成完整回复。

`NativeMessage::to_responses` 保留完整原生回复作为带 provider/version/model 的回放数据，并投影文本与可执行的原生客户端 function tool_use；fallback 仅投影最终交接后的客户端调用。载体使用 Runtime 已保留的 reasoning.encrypted_content 字段，带 CAIdex 专用前缀；其中 JSON **不是加密密文**，也不是可提交到 OpenAI 的 reasoning。它只用于 CAIdex Anthropic 边界，不宣称签名密码学校验、访问控制或历史 E2EE。调用者须按敏感原生历史存储，跨提供商切换不得原样转发该载体。

`from_responses_output` 仅恢复完整回复组：检查前缀/provider/version、大小、原生 schema、准确模型版本，以及展示文本/工具/推理与载体的一致性，拒绝丢失、修改或错误作用域的投影。允许 Runtime 展示 ID/status 变化和等价工具 JSON 格式；结构一致性校验不防止同时伪造载体和展示内容，提供商仍负责验证 thinking 签名。原生字段、内容顺序、signature/redacted data、citations/未来块/usage 保留在载体，不能从展示文本重建。

文本、命名空间/function/custom 工具、工具结果与 Responses SSE 转换已实现；服务端工具/未知块保持原生数据，绝不投影为 Runtime 可执行客户端工具。引用展示及其他尚未支持的原生语义仍需独立映射。stop reason 分开保留；max_tokens/context 上限/pause/未知原因转 incomplete。refusal/tool_use 的 completed 仅是生成结束，不代表任务成功。usage 输入归一化为未缓存+缓存读+缓存写，缺失保持未知，累计值不重复相加；原始 usage 单独完整保存。

## 工具映射与历史绑定

`ToolMap` 支持 function/custom 和一层 namespace。原生名称为工具 kind/namespace/name 身份的 SHA-256 稳定别名，不受声明顺序、增删其他工具或 schema 修改影响；同名不同 namespace 分开映射。function 保留 JSON schema 和 strict，custom 将完整文本放入唯一 `input` 字符串属性，恢复时保留空白、转义及 Unicode。工具执行仍由 Runtime 负责；服务端工具不会变成客户端调用。

原始声明及未知字段由 `source()` 完整保留，未知字段不代表已映射的原生参数。custom Lark/regex 格式只写入原生描述，JSON Schema 不实施该语法；`has_grammar_tools()` 明确暴露此限制，不授予硬约束或完整 Code Mode 兼容性。

`to_responses_with_tools` 使用 v2 专用前缀，把本次工具声明随完整 native message 保存。回放只用该历史快照重建映射，不用当前请求声明重新解释旧调用；严格校验 function namespace/name/call_id/JSON 与 custom 原文。v1 仍可回放，v1 不接受 tools 扩展。v2 同样只是敏感 JSON 载体，不是密码学认证或加密；同时伪造载体与投影仍不在结构一致性检查的防护范围。

新增 7 项工具测试覆盖稳定别名/同名隔离/并行 ID/大整数/custom 原文、缺字段安全拒绝、坏声明与上限，以及 v2 在经典/Lite canonical wire 中的往返、历史身份/文本/声明篡改拒绝。实际 Runtime 的既有载体测试针对 v1；v2 namespace/custom 在真实 Runtime 的工具执行和整套 Adapter 接入尚未验收。

## 验证

- 12 项协议测试：所有字节切分及逐字节 UTF-8/SSE、签名和 opaque 顺序、工具输入、累计 usage、各 stop reason、未知字段/大整数、截断/取消/错误/预算、Models 分页与能力证据。
- 7 项基础真实 loopback HTTP 测试：认证/版本/workspace、游标编码、两轮原生签名回放、429 安全分类且不重试、取消及 missing Key 不发送、实际 socket EOF/timeout/并发许可释放、跨页字节和数量上限、配置拒绝。
- 使用合成秘密与离线 fixture，未读取用户模型 Key、调用商业 API或执行工具。源码 `95df007` 的 [CI 37575612338](https://github.com/bboytang/CAIdex/actions/runs/37575612338) 在 Linux/Windows/macOS 全部成功：每个平台 12 项协议、7 项 HTTP 测试通过，既有固定 Runtime 回归 Linux 23、Windows/macOS 各 22 项通过。该 Runtime 回归尚不验证 Anthropic 互操作。

新增 6 项原生 SSE socket 测试：签名/工具/累计 usage、完成后主动关闭、静态错误分类、EOF/大小上限、Drop/取消、满队列 deadline/错误保存/许可释放、idle timeout/content-type。源码 `4b856b8` 的 [CI 37576472343](https://github.com/bboytang/CAIdex/actions/runs/37576472343) 三平台全部成功，逐平台确认协议 12/HTTP 13 项通过；既有 Runtime Linux 23、Windows/macOS 各 22 项回归通过。尚不验证 Anthropic Runtime 互操作。

回复投影/回放新增 7 项测试与固定 Runtime 经典/Lite 各一项：[CI 37578134958](https://github.com/bboytang/CAIdex/actions/runs/37578134958) 三平台全部成功（回复源码 a0de05f、fixture 修正 1fc0d3c）。每个平台协议 12/HTTP 13/投影 7 项通过，Runtime 累计 Linux 25、Windows/macOS 各 24 项。首轮 Linux native keyring 就绪检查触发 D-Bus 自动激活，已改为不激活的 owner 查询及 unlocked collection 门槛；新 CI 原生 keyring 通过，未改生产凭据逻辑。载体测试不是完整 Anthropic 请求/Gateway 互操作、商业模型或 Code Mode 工具验收。

工具映射/历史绑定源码 `bfaebb5` 的 [CI 37579438958](https://github.com/bboytang/CAIdex/actions/runs/37579438958) 三平台全部 success：各平台工具 7/投影 7/协议 12/HTTP 13 项，workspace/fmt/Clippy、native keyring/schema/doctor 通过；既有真实 Runtime Linux 25、Windows/macOS 各 24 项通过。v2 工具测试覆盖 canonical wire 与原生回复还原，完整请求转换/Gateway 和真实 Runtime 工具执行仍待接入验收。

## Responses 请求基础转换

`MessagesRequest::from_responses` 编译原生请求并保留完整收到的 Responses source。native model 与正数 max_tokens 由执行端显式提供，不按别名猜 token 上限；输入和原生输出 JSON 均检查字节预算。经典读取顶层 tools/instructions，Lite 仅读取首项 developer additional_tools；初始 system/developer 文本进入 native system，保留顺序。中途 system/developer 通过 from_responses_with_system_messages 的执行端显式能力开关接入；默认方法仍拒绝，不从名称或请求 JSON 判断支持。保持原位置，不能静默提升到所有历史之前。

v1/v2 原生历史以完整投影组恢复，原 signed thinking/未知块不改动。function/custom 调用与结果通过 call_id/kind 关联；旧式只有 name/namespace 的 function 结果仅在待完成集合内唯一匹配时接受。未知/重复/错误身份、未齐的并行结果、结果与下一批调用交错明确拒绝。结果先放入紧接 assistant 的 user 内容，再允许普通 user 文本；此层不执行工具。

用户与工具结果图片按原顺序转换：HTTPS URL 引用、JPEG/PNG/GIF/WebP base64 data URL；不抓取网络图片或本地文件，base64 格式校验后保留原数据。OpenAI file_id 不可用于另一提供商；非 auto detail 尚未建立等价语义，明确拒绝。图片实际格式/尺寸/模型限制仍由提供商验证。

原生 tool_choice 映射 auto/required/none，parallel_tool_calls 映射 disable_parallel_tool_use。基础构造入口默认关闭额外能力；reasoning、summary/context、strict:true 结构化输出和 Runtime 字段通过 RequestOptions 显式门控，详见下文。其他未映射语义明确报 unsupported；完整 source 保留不等于原生语义支持。完整 Adapter/Gateway 和真实 Runtime 工具执行尚未完成。

基础请求转换源码 `d774ff5` 的 [CI 37580529092](https://github.com/bboytang/CAIdex/actions/runs/37580529092) 三平台全部 success：各平台请求 8/HTTP 14/协议 12/投影 7/工具 7 项通过，workspace/fmt/Clippy/native keyring/schema/doctor 通过。真实 Runtime 既有 Linux 25、Windows/macOS 24 项通过。新增 HTTP 两轮验证编译结果通过原生传输回放签名和 custom 结果，不代表完整 Gateway 或 Runtime Anthropic 工具执行已验收。

中途指令新增执行端显式开关及原生位置门控：连续 system 内容保持顺序；前面必须是 user（包括完整工具结果）或以已识别 server result 结尾的 native assistant，后面必须是 assistant 或请求结束。不会在客户端工具调用与未完成结果之间插入指令，不把图片或工具输出提升为系统指令。clear_at/per-message output_config 仍明确拒绝，相关 beta 语义待专门映射。开关只是配置契约，不代表发现或实际模型验收。新增 4 项请求测试及 HTTP 经典/Lite 两轮位置检查本机通过，[CI 37581359918](https://github.com/bboytang/CAIdex/actions/runs/37581359918)（源码 2ccbe38）三平台全部 success：各平台请求 12/HTTP 14/协议 12/投影 7/工具 7、workspace/fmt/Clippy/native keyring/schema/doctor 通过；既有真实 Runtime Linux 25、Windows/macOS 24 项通过。尚未验证完整 Anthropic Runtime 工具执行或商业 API。

## 推理参数映射

`ReasoningMapping` 由执行端声明 Responses effort 与原生 output_config.effort/thinking 的对应关系；没有按名称猜测或全模型默认映射。不同提供商 effort 不是等价标尺，配置声明不等于兼容性验收。支持原生 adaptive、disabled、between_tools，以及无 interleaved beta 的 enabled 手动预算；display 可固定 summarized/omitted，字段形状校验不代表任意模型都支持该模式。

手动预算至少 1024 且小于执行端 max_tokens；不从请求 JSON 接受预算。最终 assistant 历史须以原生 thinking/redacted_thinking 开头；不插入合成签名。不支持大于 max_tokens 的 interleaved beta 预算，后续必须单独映射认证/能力契约。显式 active thinking 拒绝 required/any 强制工具；between_tools 与 xhigh/max 组合拒绝，其他模型特有组合仍需配置与提供商验收。

默认转换仅允许空/未设置的 reasoning；实际 effort 必须找到唯一映射。非空 summary/context、未知字段、坏类型明确拒绝，不能静默丢弃；null 已知可选字段视为未指定。原始请求 source 保留。更改 effort/模式可能影响缓存和签名上下文；当前尚未实现持久化配置版本/换模型承接，不宣称变化后的原生签名兼容已验证。

新增 5 项推理回归及经典/Lite HTTP 两轮参数检查，完整 workspace/Clippy/fmt/diff 已通过。[CI 37582398993](https://github.com/bboytang/CAIdex/actions/runs/37582398993)（源码 25081d8）三平台全部 success：各平台推理 5/请求 12/HTTP 14/协议 12/投影 7/工具 7 项通过，workspace/fmt/Clippy/native keyring/schema/doctor 通过；既有 Runtime Linux 25、Windows/macOS 24 项通过。尚未完成完整 Provider/Gateway 或真实 Runtime Anthropic 工具执行。

## 后续顺序

1. 实现请求前缀/账户绑定，覆盖 mode/tool/trim/resume；复用已验收的原生传输/请求转换/SSE 投影/六方法及上下文头。
2. 接 Gateway，经典/Lite 分别验证；不静默丢弃未映射语义。
3. 固定 Runtime 经典/Lite 多轮/工具/interrupt 离线验收；必要的 Runtime 修改保持最小范围。
4. 再进入 Gemini。真实提供商兼容性与付费调用须另行明确授权，离线成功不授予 Full 标签。

## 官方契约依据

- [Messages API](https://platform.claude.com/docs/en/api/messages/create)：原生请求、认证版本及不修改签名内容的回放要求。
- [Models API](https://platform.claude.com/docs/en/api/models/list)：after_id 分页、模型元数据和声明的能力。
- [Streaming](https://platform.claude.com/docs/en/build-with-claude/streaming)：事件生命周期、工具 JSON/签名增量、累计 usage。
- [Stop reasons](https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons)：暂停、拒绝、上限和正常结束应区分。

- [OpenAI reasoning](https://developers.openai.com/api/docs/guides/reasoning#preserve-reasoning-without-stored-responses)：Responses 的 opaque 回放字段；CAIdex 原生载体仅复用固定 Runtime 的传输槽位，不是 OpenAI 密文。
- [Anthropic prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching#tracking-cache-performance)：总输入计数为三项相加，不能只取非缓存 input_tokens。

- [Images and vision](https://platform.claude.com/docs/en/build-with-claude/vision)：原生 URL/base64 图片内容形状与格式支持；CAIdex 不代替模型的图片尺寸/格式验收。

- [Mid-conversation system messages](https://platform.claude.com/docs/en/build-with-claude/mid-conversation-system-messages)：部分模型支持且限制放置位置；已实现显式能力开关及位置验证，默认基础转换仍拒绝。
- [Effort](https://platform.claude.com/docs/en/build-with-claude/effort)：output_config.effort 与 thinking 模式不同，后续逐项映射，不猜模型能力。

- [Extended thinking](https://platform.claude.com/docs/en/build-with-claude/extended-thinking)：手动预算下限与 max_tokens 关系，interleaved beta 的例外需独立接入。

- [Structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs)：下一步接入 output_config.format 时与 effort 合并，保留 schema 约束，不能静默缩减。

## 结构化输出请求转换

执行端 `RequestOptions.supports_structured_outputs` 显式启用后，将 Responses `text.format` 的 `json_schema` / `strict:true` 转为 `output_config.format`。name 是原始格式身份，保留于 source；schema 原样发送，包括引用、注释、未知关键词和任意精度数值，不执行 SDK 的约束删减。先应用 reasoning，再添加 format，保留同一 output_config 中的 effort。旧构造入口默认关闭此能力。

本次支持空/普通 text 配置。strict:false、strict 缺省/null、json_object、非空 wrapper description、非空 verbosity 和未知配置字段明确报不支持，仍需后续语义映射。执行端开关及 HTTP fixture 只证明请求转换，不证明模型实际支持，也不证明输出符合任意 schema；原生拒绝/max_tokens 仍按已有 outcome 区分。schema 全量透传可能由原生 API 拒绝，不能把透传误标成全约束验收。

新增 4 项结构化转换回归，覆盖经典/Lite、effort/thinking 合并、source/schema 精确保留、能力门控、不支持语义、坏输入、数值精度及最终 body 字节上限；既有真实 HTTP 两轮 fixture 同时核验 format 和 effort。完整 workspace/Clippy/fmt/diff 通过；[CI 37583615309](https://github.com/bboytang/CAIdex/actions/runs/37583615309)（源码 9796682）三平台全部 success，新增 4 项逐平台通过。

依据：[OpenAI Structured outputs](https://developers.openai.com/api/docs/guides/structured-outputs)、[Anthropic Structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs)。未调用商业 API。

## Summary / reasoning context 执行端契约

`SummaryMapping` 显式声明 OpenAI summary auto/concise/detailed 到原生 summarized 的映射；不宣称各档位等价，不猜默认，不自动启用 thinking。摘要请求不能映射 omitted（否则隐藏所请求摘要），也不能给 disabled/between_tools 添加 display。只有已配置 adaptive/enabled 的 effort 映射可承接摘要，已配置 display 被本次显式 summary 设置覆盖。

`ThinkingContext` 声明实际原生模型保留策略 CurrentTurn/AllTurns；只有请求 context 与配置完全一致才通过。不会靠模型名字推断、编造原生字段、裁掉旧签名或把 all_turns 降为 current_turn。声明和离线 fixture 不是模型实际保留推理的证明；原生 API 的模型/账户/前缀绑定仍需后续验收。

新增 3 项推理测试（累计 8 项），覆盖经典/Lite、配置缺失/不匹配、display 覆盖、禁用思考/无 effort 不被隐式启用、坏 summary/context、重复配置及当前轮策略不删历史；实际 HTTP 两轮 fixture 包含 summary/context 和既有结构化输出、签名工具历史。修正 between_tools 禁止 display。完整 workspace/Clippy/fmt/diff 已通过；[CI 37610039312](https://github.com/bboytang/CAIdex/actions/runs/37610039312)（源码 024b211）三平台全部 success，新增 3 项和扩展 HTTP 两轮逐平台通过。

依据：[OpenAI reasoning summaries](https://developers.openai.com/api/docs/guides/reasoning)、[Anthropic thinking](https://platform.claude.com/docs/en/build-with-claude/thinking)。较新原生模型还检查签名的模型、账户和 system/tools/messages 前缀；现有载体不证明这些匹配，完整 Adapter 需请求前缀契约与 mode/tools/trim/resume 回归，不能自动 drop_block 或 retry 掩盖失败。参见 [Preserved thinking](https://platform.claude.com/docs/en/build-with-claude/preserved-thinking)。

## 固定 Runtime 请求字段

`include` 仅接受 reasoning.encrypted_content；原生回复经现有投影产生 CAIdex 版本化载体，不代表 OpenAI 密文，也不将 include 发给原生 API。未知输出扩展和重复项拒绝。

`retain_runtime_metadata` 是执行端显式契约：client_metadata 字符串映射、非空字符串 prompt_cache_key 原样保留于 source，仅用于 CAIdex 本地归属/历史；不提升为 Anthropic metadata.user_id，不发送任意认证头，不据此创建 cache_control。这个契约不承诺 OpenAI 缓存隔离、计费或路由等价，原生缓存策略仍待独立实现和验收。未知元数据字符串字段保留，坏形状拒绝，Debug 不暴露 wire。

ServiceTierMapping 由执行端固定：source default/auto 到 native standard_only/auto；不按同名推断 SLA/计费等价。无映射、重复配置、priority/flex 都拒绝，避免伪装能力。stream_options 缺省/null/空对象为无额外投递要求，sequential_cutoff 等非空值尚未等价实现，明确拒绝；access_programs 非空亦拒绝。

新增 3 项回归，经典/Lite 实际 HTTP 两轮扩展检查原生档位及本地字段未外发、原始 source 保留。完整 workspace/Clippy/fmt/diff 通过；[CI 37611149491](https://github.com/bboytang/CAIdex/actions/runs/37611149491)（源码 1afee18）三平台全部 success，新增 3 项与扩展 HTTP 两轮逐平台通过。依据：[OpenAI prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching)、[Anthropic Messages](https://platform.claude.com/docs/en/api/messages/create)、固定上游 d27764b 的 codex-api/src/common.rs 与 core/src/client.rs。完整 Adapter / Runtime 工具执行 / 商业 API 仍未验收。

## 原生 SSE 到 Responses 增量投影

ResponsesProjection 接收已验证的 NativeStreamEvent。原生块结束事件携带内部完成块快照，避免重建第二套原生解析器；最终 NativeMessage 必须与每个已停止块、准确模型和响应 ID 相符。文本、摘要、function 参数实时发送，function 保留原始 JSON 空白及数值精度；custom 的 JSON 包装在块结束后恢复完整原始字符串（目前不承诺逐字符 custom 输入展示）。signature/redaction/server 工具/未知块不变成普通文本或客户端执行工具，完整 native wire 留在 v2 载体中。

原生 message_stop 之后，先发送 reasoning 载体 done，再按输出顺序发送其他 done 和终态；不会把块结束、EOF、HTTP 200 当成功。MessageOutcome 中 max_tokens/context window/pause/未知停止原因仍为 incomplete，refusal 保留原生分类。事件序号覆盖所有输出，汇总结果重新校验与载体的一致性；最大事件/原生累计字节受显式上限限制，错误只有静态代码。文本 phase 在最终状态已知后定案。

ProjectedStreamingResponse 直接拉取原生单槽 worker，无第二次模型请求或额外传输 worker；有限 pending 事件保留背压，取消/超时错误与 Drop 会释放原生 socket/并发 slot。一次 poll 对无投影 metadata 事件最多处理 32 个，避免阻塞执行器；native ping 映射 heartbeat。

新增 4 项投影测试：字节切分、文本/摘要/工具增量及签名回放、SSE 序号与终态、停止类型、模型/最终块不匹配、预算及失败后不得完成。新增 2 项真实 HTTP fixture：无 native stop 时先收到文本、取消/Drop 关 socket 释放 slot、完整 signed function 回放且无需 HTTP EOF。源码 6a3986a 的 [CI 37613084679](https://github.com/bboytang/CAIdex/actions/runs/37613084679) 三平台全部 success，新增 6 项逐平台日志核对通过；本地 workspace/Clippy/fmt/diff 通过。Gateway、生产请求前缀/账户绑定及实际 Runtime 工具执行仍待接入，商业 API 未调用。

依据：[OpenAI Streaming responses](https://developers.openai.com/api/docs/guides/streaming-responses)、[Anthropic Streaming messages](https://platform.claude.com/docs/en/build-with-claude/streaming)。

## ModelProvider 六方法

AnthropicProvider 接收已初始化的 AnthropicClient、AnthropicModel 列表及原生清单数量上限，复用同一认证/TLS/并发/超时/字节预算。AnthropicModel 固定别名、准确 native model、max_tokens/max_tools 和现有 RequestOptions 的执行端映射；默认不猜模型能力。空/重复身份、非法 metadata、零预算和超过已声明 output_limit 的预算拒绝；使用现有编译器检查映射重复，不增加另一套请求校验器。

list_models 读取完整原生清单并与配置的 native model 取交集，返回 ProviderCatalog 可用性证据，不给未配置模型自动路由或 Full 标签。metadata/capabilities 不读取 Key；credential_requirements 使用共享 ApiKey variant，只公开 Broker reference。create_response/stream_response 分别校验模式、别名、dialect 与 streaming 门控；经编译器及原生传输后投影。非流式亦校验准确回复模型和投影预算，不能把错误模型回复纳入原生历史；流式复用 ProjectedStreamingResponse。

新增 6 项回归覆盖模型清单过滤/认证元数据、坏 profile 和虚假 Full 报告、经典/Lite 多轮 signed custom history 与完整配置联动、错误回复模型/完整投影预算、发送前拒绝（Key 未读）、经典/Lite signed function 流式回复。HTTP 累计 22 项；源码 6ae83b9 的 [CI 37614432384](https://github.com/bboytang/CAIdex/actions/runs/37614432384) 三平台 success，新增 6 项逐平台日志核对通过，workspace/fmt/Clippy/native keyring/schema/doctor 及既有 Runtime Linux 25、Windows/macOS 24 项通过。本地 workspace/Clippy/fmt/diff 通过；末次只增强测试后 HTTP/Clippy 复验通过。所有模型和秘密为合成 fixture，未调用商业 API。

当前 context headers 契约见下节；request-id 已接入，不编造原生 turn-state 等价。client_metadata/prompt_cache_key 在编译 source 保留且不外发；Provider 本身不持久化原始请求，生产 Host 仍须保存调用端 canonical 请求与本地归属。v2 回复载体不证明原请求前缀/账户匹配，配置/模型变化后的 signed history 和真实 Runtime 工具执行需后续验收。

## 本地 Runtime 上下文与响应关联

AnthropicConfig.with_local_runtime_context 由执行端显式启用；仅接受 session_id、x-client-request-id、x-codex-turn-metadata，在原 RequestContext 及 I/O worker 生命周期内保留，不发送到原生 HTTP 或 metadata.user_id。不会承诺 OpenAI 缓存亲和性、粘性路由或账户归属等价，也不代替 Host 的持久化。默认原生客户端继续拒绝这些头；错误方向头和 x-codex-turn-state 始终拒绝，后者是上游服务器发出的路由 token，而本 Adapter 不返回该头。

成功原生 JSON/SSE 响应的 request-id 经共享 ContextHeaders 校验后映射到 x-request-id；create_message_with_headers 返回消息及关联头，原 create_message 保留原接口。NativeStreamingResponse.headers 在建流时提供关联头，AnthropicProvider 传至共享响应契约；不复制原生 x-request-id、turn-state、账户/认证或其他任意头。缺少 request-id 保持缺省，重复、空值、非 ASCII 或超过共享 8192 字节上限报静态 502。SSE 校验在 worker 启动前，失败立即关闭连接并释放 permit；错误状态沿用既有静态分类，不导出原生错误 body/header。

新增 4 项 HTTP 回归，扩展既有经典/Lite 流式 fixture，覆盖 opt-in、三项本地字段未外发、响应关联、缺省/重复/空/坏编码/超长头、坏 SSE 头关闭 socket/释放 slot、发送前拒绝且 Key 未读。HTTP 累计 26 项。本轮还修正 profile 初始化的 Lite-only/Lite-first input 形状错误，使用合法消息数组并扩展原配置回归。源码 f35d2a7（完整 SHA f35d2a7ba6f53794d20b9c8cf9d07b918231a98d）的 [CI 37616230221](https://github.com/bboytang/CAIdex/actions/runs/37616230221) 三平台 completed/success，新增 4 项及扩展 2 项逐平台日志核对通过；workspace/fmt/Clippy/native keyring/schema/doctor 及既有真实 Runtime Linux 25、Windows/macOS 24 项通过。本地完整 workspace/Clippy/fmt/diff 通过。请求前缀/账户历史及完整 Anthropic Gateway/实际 Runtime 工具执行未完成。

依据：[Anthropic response headers](https://platform.claude.com/docs/en/api/overview)、[Anthropic request ID](https://platform.claude.com/docs/en/api/errors)、[固定 Codex client.rs](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/client.rs)。所有 HTTP 为合成 fixture，未调用商业 API。

## Input transformations 与历史交付门控

NativeMessage.input_transformations 返回原生报告的借用视图；JSON 和 SSE 验证可选数组及已知报告 type/path/reason 字符串形状。缺省/null 与空数组区分保留，未知 type/reason 和扩展字段保持原始数据及数值精度，不猜其语义。原生 parse/HTTP 接口把报告作为生成数据返回，不把 HTTP 200 或生成结束当作输入推理已保留的证明。

MessageStream 保留 message_start.message 中的报告。fallback 后的最终 message_delta 在事件顶层携带新数组，而不是 delta 内；非 null 数组替换初始报告，缺省/null 不覆盖（与官方 SDK 相同），不能累加两个不同 serving model 的报告。

Responses 投影及历史恢复共用检查：已知 thinking_dropped 的 prefix/model/organization/end_user_binding_mismatch 返回静态 502 anthropic_input_thinking_dropped；已知 thinking_mismatch_allowed/prefix_binding_mismatch 返回静态 502 anthropic_input_binding_mismatch。回放旧或改动载体中的这些报告报 400 invalid_anthropic_replay。流式在首帧或末帧见到已知问题立即终止，不交付 output_item.done、终态或新历史载体；末帧错误之前已发送的展示/参数增量不被冒充成完成。ProjectedStreamingResponse 沿用 Drop 关闭原生 worker/socket 并释放许可，不增加传输 worker，不 drop_block、删历史或重试；原始诊断/path/扩展内容不加入错误。未知报告按原生契约保留，不构成已验证兼容性的证据。

新增 5 项回归覆盖报告形状、逐字节 SSE、缺省/null/空与最终数组替换、未知字段/大整数、两种已知 type 与四种 drop reason、旧载体防绕过、经典/Lite 实际 JSON/SSE、首帧/末帧失败后的无工具 done/完成/socket 关闭/slot 释放与单次 Key 读取。源码 b4d05fd（完整 SHA b4d05fdfc623343992fdb5592456b03718dd247d）的 [CI 37618365350](https://github.com/bboytang/CAIdex/actions/runs/37618365350) 三平台 completed/success，新增 5 项逐平台日志核对通过，协议 13/投影 8/增量投影 5/HTTP 28 及 workspace/fmt/Clippy/native keyring/schema/doctor、既有真实 Runtime Linux 25/Windows/macOS 24 项通过。本地完整 workspace/Clippy/fmt/diff 通过。

尚未接入 thinking-binding-controls-2026-08-01 执行端 beta opt-in、前缀快照和真实组织作用域；缺省报告无法发现静默 drop，不宣称完整历史绑定或商业模型验收。fallback 身份/echo 修正见下节，仍不能据此宣称已验证跨模型兼容。上述工作完成后再接 Gateway/实际 Runtime。

依据：[Preserved thinking](https://platform.claude.com/docs/en/build-with-claude/preserved-thinking)、[Beta Messages API](https://platform.claude.com/docs/en/api/typescript/beta/messages/create)、[官方 SDK message_delta 定义](https://github.com/anthropics/anthropic-sdk-python/blob/main/src/anthropic/types/beta/beta_raw_message_delta_event.py)、[官方 SDK 累积逻辑](https://github.com/anthropics/anthropic-sdk-python/blob/main/src/anthropic/lib/streaming/_beta_messages.py)。未调用商业 API。

## 原生 fallback 身份、usage 与 echo

原生交接块验证 from/to.model 非空、可选 trigger 形状、连续交接链及最终 top-level model 与末次 to.model 一致；保留未知扩展，不按模型名猜许可。SSE 在 content_block_start 更新实际 serving model，交接前块必须停止，fallback 不接受 delta。首帧已是接替模型（输出前交接）及没有边界的 sticky 路由仍使用提供商报告的身份，不能把没有 fallback 块当成没有接替模型。

换模型后，已知四项 token 计数不再属于同一 attempt：清除旧计数，最终提供的计数替换，缺失保持未知，不继承另一模型的数字；同模型累计值仍拒绝下降。unknown usage 和 iterations 原样保存，不能把多模型尝试加成一个模型的计费值。JSON 原始身份/usage 不被改成请求模型；NativeMessage 保存完整原生回复，SSE 完成消息保存末次 serving model。

replay_message 仅生成原生 Messages echo，不改持久化 wire/载体。依据最后一个 fallback 边界，echo 排除边界前的 thinking/redacted_thinking/connector_text/client tool_use，以及没有匹配 *_tool_result 的 server_tool_use；保留文本、交接块相对顺序、已完成服务端工具/结果、未知块及最后交接后的全部内容。原生规则要求这项过滤，不能将其用于掩盖前缀/账户不匹配，不能触发 drop_block 或 retry。投影与 replay_group_len 共用可投影块计算，已退出模型的客户端工具不生成可执行调用；调用端也不能提交这些旧调用的结果来绕过 pending 校验。

固定 ModelProvider 的准确模型配置边界不被悄悄改写：非流式沿用 model mismatch 检查，流式遇实际 to.model 不符时在交接处返回静态 502 anthropic_response_model_mismatch，不交付工具 done、完成或新载体；沿用既有 worker 的 Drop/socket/slot 释放。原生 API 本身仍返回真实模型数据。显式 server-side fallback 请求/beta、获验证的跨模型组合及模型切换策略尚未接入，不自动增加重试或推理请求。

新增 6 项回归：输出前/中途/多跳/sticky 身份、逐字节 SSE、错误 shape/chain/最终身份/非法 delta/开放工具、不同 attempt 计数下降/缺失/同模型仍单调、完整 raw 历史与合法 echo、已退出调用不可执行、经典/Lite 请求编译及 JSON/SSE socket/permit/no retry。源码 1f5e022（完整 SHA 1f5e022942f1eab8153d4410dfb18ba6fb580533）的 [CI 37621255255](https://github.com/bboytang/CAIdex/actions/runs/37621255255) 三平台 completed/success，新增 6 项逐平台日志核对通过（协议 15/投影 9/增量投影 6/HTTP 30）。本地完整 workspace/Clippy/fmt/diff 通过；CI workspace/fmt/Clippy/native keyring/schema/doctor 与既有真实 Runtime Linux 25、Windows/macOS 24 项通过。所有 Key/数据为合成 fixture，未调用商业 API。

依据：[Refusals and fallback](https://platform.claude.com/docs/en/build-with-claude/refusals-and-fallback)、[官方 SDK fallback block](https://github.com/anthropics/anthropic-sdk-python/blob/main/src/anthropic/types/beta/beta_fallback_block.py)、[官方 SDK 累积逻辑](https://github.com/anthropics/anthropic-sdk-python/blob/main/src/anthropic/lib/streaming/_beta_messages.py)。
