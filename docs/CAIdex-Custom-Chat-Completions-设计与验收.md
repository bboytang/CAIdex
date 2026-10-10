# CAIdex Custom Chat Completions：设计与验收

2026-10-10 用户正式补充 V3 F/G：Custom Responses-compatible 与 Custom OpenAI-compatible 两类同时保留。本 Adapter 为独立 `caidex-provider-chat-completions`，标准 `/chat/completions` 上游；现有 DeepSeek/Qwen/OpenRouter 仍为各自 Responses Adapter。固定 Codex 0.160.1/d27764b 的执行与审批继续是真源，不新增执行器、审批引擎、模型 HTTP 栈或 fallback。

## 协议与复用

依据 [Chat Completions 请求/回复](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create)和[原生流事件](https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events)。这是公共协议依据，不是任意商业端点支持证据。

六方法 ModelProvider、CanonicalRequest/Response、Credential Broker、ModelRouter、Responses Gateway 原样复用。Custom 共享传输新增 `post_sse`：原认证、TLS、并发、请求/帧/响应预算、取消、header/idle/overall deadline 和静态错误分类；SseDecoder 只做 framing，不把原始 Chat 帧送入 ResponsesStream。单槽后台交付及 Drop abort 保证未消费/背压时取消也能关闭上游。原 Responses 解码与已有 Provider 实现不改。

端点及 CredentialRef 是执行端配置，必须精确以 `/chat/completions` 结尾；HTTPS 或 literal-loopback HTTP，无用户信息/query/fragment，不跟随 redirect、不读取隐式环境 Key。配置 Debug 不输出端点。目录为配置列表，能力/模型限额默认 Unknown/None，兼容报告默认 None；不将可配置 URL、名字或合成推理当商业能力证明。所有请求和工具编译先于凭据解析；Listener Token 与执行端 Key 分离。

## 转换契约与能力边界

| 项目 | 实现与限制 |
| --- | --- |
| 输入 | instructions→system；user/developer/system/assistant 文本消息→messages；Classic tools 与 Lite 首项 additional_tools 共用编译。Runtime message ID 仅在显式 context 配置接受并本地消费；身份不授权上游路由。 |
| 工具 | function 及 namespace 通过稳定 SHA-256 alias 编译成 native function；声明/choice/call 都绑定同一映射。custom 文本工具用严格单 input:string wrapper；回到原 name/namespace/type。参数原字符串保留，工具不执行。 |
| 结果/历史 | call_id 去重、pending 组及类型配对；不得跨未完成调用插入普通消息或新调用组。原函数参数/完整结果文本往返；文本结果数组按原序拼接为 native string，媒体拒绝。完整已完成可见历史可重放；reasoning/签名/compaction/未知 item 均拒绝，不创建伪 opaque carrier。 |
| SSE | 单 choice/index=0，id/model/created 一致；role、内容、function name/arguments 分片及 optional null 合并。必须有效 finish_reason 和 `[DONE]`；EOF、漂移、孤立 usage、非法 alias/参数均失败。 |
| 交付 | 原生 SSE 在终态前缓冲，完成后生成合法 Responses lifecycle；工具半流、length 截断、未知工具及 parallel=false 多调用不会交付执行调用。不宣称逐 token 下游延迟；长生成仍受实际 Runtime idle 限制，不以 heartbeat 伪造模型进展。 |
| 终态 | stop/tool_calls→completed；length/content_filter→incomplete 且原因保留。弃用 function_call、工具调用与终态不一致、error 均明确失败，不把 HTTP200/EOF 当成功。 |
| usage | prompt/completion/total、cached/reasoning 计数只映射实际提供值；缺值保持未知，负数/非整数/溢出或不一致拒绝；完整原生usage（含未知扩展）保存在usage.native_chat_usage，不解释为新的计费能力。reasoning_tokens 是计费计数，非推理内容支持证明。 |
| reasoning | 通用协议没有可移植签名历史，默认拒绝非 none effort、summary/include/context 及 native reasoning 内容字段。显式 with_no_reasoning_runtime 仅允许固定 Runtime 无推理配置及其无条件可选 encrypted include 空结果；不生成 encrypted 内容、summary 或签名。该配置不保证任意上游关闭内部推理或相关计费；没有通用的控制/证明能力。 |
| grammar | 默认拒绝 grammar。显式 with_grammar_prompt_mapping 将已验证格式的 lark/regex 定义完整放入 function description，仅为提示映射，**没有原生 grammar 强制约束**，不授完整 grammar 能力。未知 syntax/format 拒绝。 |
| 其他 | temperature/top_p 有效范围、max_output_tokens→max_completion_tokens、store=false、n=1、stream_options.include_usage=true。stateful previous_response_id、服务等级、结构输出、vision/audio、hosted/discovery/deferred tools 和未知控制均明确拒绝。 |
| 边界 | Native model 必须与显式 native_model 精确一致；商业别名返回其他版本会拒绝，不能自动猜 alias。JSON 目前不透传 response context headers；SSE 只透传共享白名单。原生未知顶层字段不解释，语义未知 message/delta 字段拒绝。 |

固定 Runtime 的无推理 profile 来源是[固定源码 build_responses_request](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/client.rs)：optional encrypted include 总被请求，Lite 携带完整 tools/developer 前缀。生产默认不自动启用这些显式映射；Runtime fixture catalog 的 none effort/summary、关闭 web/search、Classic/Lite 和 grammar prompt opt-in 单独定义，不是商用全功能配置。

## 离线验收矩阵

| 要求 | 测试/当前证据 |
| --- | --- |
| JSON、六方法、usage/缺值、非成功终态 | [协议测试](../model/providers/chat-completions/tests/protocol.rs)：标准文本/中文、计数详情与 incomplete；原生非法计数、漂移、choice/错误及配置负例。 |
| 工具/namespace/custom/Lite 与结果 | 同文件：JSON 双轮原参数/完整文本结果；SSE name/arguments 分片、有效 DONE 与半流/length 对照；多调用在单调用配置下拒绝。 |
| reasoning/grammar/媒体/历史边界 | 同文件：默认 Key 前拒绝；显式 no-reasoning/grammar prompt 正例及不可支持的 effort/summary 拒绝；重复/orphan/错类型/跨组/opaque/未知字段拒绝。 |
| 错误/限流/取消/超时/预算 | 同文件：401/403/429/302/500、安全静态错误、Retry-After、单次 POST；cancel/idle/caller deadline、Drop、实际 socket 关闭和许可复用；header/overall guards复用共享传输，由[Gateway既有超时测试](../model/gateway/tests/http.rs)验证；请求/响应预算与未知 route Key 前拒绝。 |
| 共享接口具体必要性 | [Custom 测试](../model/providers/custom/tests/provider.rs)：原始 Chat SSE 缺接口编译 RED→GREEN；未消费双帧 cancel 真实 socket 不关闭 RED→后台交付 GREEN，原 Responses 既有七项保持。 |
| Router/Gateway/固定 Runtime | [真实 Runtime 离线测试](../runtime/bridge/tests/chat_completions/mod.rs)：Classic/Lite 文本双轮与 disk resume；ModelRouter→Gateway→新 Adapter→合成 native Chat。真实原生批准后隔离执行、完整参数/结果与磁盘一致；重启不重跑；Cancel/等待审批 interrupt/迟到批准无副作用。 |
| 全体回归/三平台 CI | 最终本地workspace640/0/83（本次含原生凭据子进程项）、固定Runtime81/0/0及Clippy/fmt/diff通过；新Adapter15项、Custom9项、新Runtime3项全部覆盖。源码586199fb9655ccd8bff1830968b209aee3d6bf23/[CI38090495112](https://github.com/bboytang/CAIdex/actions/runs/38090495112)整体completed/success，三平台完整日志逐名已验：workspace640/635/639、固定Runtime81/80/80、0失败，不沿旧CI认领。首次CI fixture证据竞争已修复并完整本地复验。 |
| 商业/生产 | 未执行；用户全项目完成后自验商业模型。没有真实商业版本、生产 Host、API 特有 reasoning/签名/grammar 或性能证据，不授 LiveRuntime/Compatible/Full。 |

新增 Adapter 不自动完成 F/G；整体状态与其余门槛继续按[要求级核对](CAIdex-FG-离线验收核对-V1.md)逐项核实。准确源码、CI 和下一步见 [HANDOFF](../HANDOFF.md)。

源码`586199fb9655ccd8bff1830968b209aee3d6bf23`/[CI38090495112](https://github.com/bboytang/CAIdex/actions/runs/38090495112)整体completed/success；Linux/Windows/macOS三个job114325748624/114325748645/114325748432，各17steps成功或预期跳过，完整日志逐名精确旧CI38086078152集合+15Chat+2Custom+3Runtime，无遗漏/重复。workspace640/635/639（忽略83/81/81），固定Runtime81/80/80（无忽略），全部0失败；Linux另单独原生凭据项1通过，Secret compile-fail doctest保持。raw2358/2044/2055行、函数通过名721/714/718。Core29/Gateway24/Provider550各一次：Custom9、Chat15、OpenAI11、Anthropic123、Google94、Ollama69、DeepSeek57、Qwen72、OpenRouter100。watch与三个完整日志下载exit0。
