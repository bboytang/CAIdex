# Anthropic 原生 Provider：实现与恢复点

## 当前范围

`model/providers/anthropic` 已实现原生 Messages/Models 数据结构、SSE 重建，以及独立原生 HTTP/SSE client。它是完整 Adapter 的基础，已有原生回复→Responses 投影和版本化原生回放，固定 Runtime 经典/Lite 两轮载体测试本机通过；尚未实现 Responses 请求→Messages、ModelProvider 六方法、Gateway 注入和完整 Runtime 互操作。

`AnthropicConfig` 固定执行端 API Key 引用（provider=anthropic、kind=ApiKey）、基址和可选 workspace。默认 HTTPS；显式代理/本地 fixture 复用既有 endpoint 安全策略，仅 literal loopback 允许 HTTP，不接受 URL 用户密码/query/fragment。HTTP client 保持 TLS 验证、禁用代理自动发现、重定向和自动重试；支持显式额外信任根。

Broker 只在发送时解析指定引用，不自动读取环境密钥。发送 `x-api-key`、`anthropic-version: 2023-06-01`，workspace 由执行端配置，均不来自请求 JSON。认证值标记 sensitive；错误只返回静态分类和安全 Retry-After 元数据，不传播 URL、原始 HTTP 错误或第三方错误 body。原生消息和事件是调用者数据，不是已脱敏日志；其 Debug 隐去 wire。

`discover_models(max_models, context)` 访问配置基址下的 Models 路径，使用 `limit=1000` 和 URL 编码的 `after_id`。完整分页共享单个并发许可、总 deadline、累计响应字节预算；重复 ID、坏游标、不完整结果和数量上限报错，不返回部分模型清单。能力仅由 Models 已声明的 image_input/thinking/structured_outputs 支持标记及正数 token 上限映射；缺失/null/零上限保持未知。名称不推导能力或 Codex 兼容性。

`create_message(model, native_json, context)` 发送非流式原生 Messages JSON：明确 max_tokens，固定 model 参数覆盖 body model，stream=false；保留原生消息内容及未知字段，限制请求/响应大小。它不是 Responses 请求转换，未全面验证提供商的所有输入 schema，其他参数是否合法仍由原生服务判定。携带尚未映射的 context headers 明确拒绝。并发、认证/header timeout、响应 idle timeout、总 deadline、取消和 drop 不触发重试；已开始的同步 SecretStore 读取不能强制中止，取消后不发送请求。

原生回复按 content 原顺序保留，包括 thinking/signature、redacted_thinking、工具及未来块；回放直接使用原内容。签名不做密码学验证，缺少完整签名的 thinking 当前拒绝回放。SSE 增量重建文本、签名、工具 JSON 和 citations，严格校验 lifecycle/index/usage；usage 累计值替换而非相加。未知事件/块保留；无法重建的未知 delta 在 block stop 报错，避免伪造无损 history。`Completed` 只表示收到完整 message_stop，具体 end_turn/tool_use/max_tokens/pause/refusal 等由 MessageOutcome 区分。

`stream_message` 与非流式共用认证、HTTP 状态和 media-type 检查，使用 stream=true 和 text/event-stream。返回原生事件流，只有解析器完成 message_stop 后才提供独立 Completed(NativeMessage) 数据项；保留原生签名、工具内容和累计 usage，不执行工具。原生 error 事件被替换为静态 ProviderError，不输出第三方诊断文本、URL 或原始 frame。

事件队列容量为 1，读取和发送均受取消/总 deadline 约束。I/O worker 持有并发许可；流无人读取时也会因超时/取消关闭 socket、释放许可，并独立保存错误，待已有队列项排空后交付。Drop 终止 worker。正常完成主动关闭上游连接，不等待 HTTP EOF；截断、非法 SSE、超限和错误不生成完整回复。

`NativeMessage::to_responses` 保留完整原生回复作为带 provider/version/model 的回放数据，并投影文本与原生客户端 function tool_use。载体使用 Runtime 已保留的 reasoning.encrypted_content 字段，带 CAIdex 专用前缀；其中 JSON **不是加密密文**，也不是可提交到 OpenAI 的 reasoning。它只用于 CAIdex Anthropic 边界，不宣称签名密码学校验、访问控制或历史 E2EE。调用者须按敏感原生历史存储，跨提供商切换不得原样转发该载体。

`from_responses_output` 仅恢复完整回复组：检查前缀/provider/version、大小、原生 schema、准确模型版本，以及展示文本/工具/推理与载体的一致性，拒绝丢失、修改或错误作用域的投影。允许 Runtime 展示 ID/status 变化和等价工具 JSON 格式；结构一致性校验不防止同时伪造载体和展示内容，提供商仍负责验证 thinking 签名。原生字段、内容顺序、signature/redacted data、citations/未来块/usage 保留在载体，不能从展示文本重建。

文本显示及基本 function 工具投影已实现；服务端工具/未知块保持原生数据，绝不投影为 Runtime 可执行客户端工具。命名空间/custom 工具转换、引用展示、工具结果/完整请求翻译、流式 Responses 事件转换仍待实现。stop reason 分开保留；max_tokens/context 上限/pause/未知原因转 incomplete。refusal/tool_use 的 completed 仅是生成结束，不代表任务成功。usage 输入归一化为未缓存+缓存读+缓存写，缺失保持未知，累计值不重复相加；原始 usage 单独完整保存。

## 验证

- 12 项协议测试：所有字节切分及逐字节 UTF-8/SSE、签名和 opaque 顺序、工具输入、累计 usage、各 stop reason、未知字段/大整数、截断/取消/错误/预算、Models 分页与能力证据。
- 7 项基础真实 loopback HTTP 测试：认证/版本/workspace、游标编码、两轮原生签名回放、429 安全分类且不重试、取消及 missing Key 不发送、实际 socket EOF/timeout/并发许可释放、跨页字节和数量上限、配置拒绝。
- 使用合成秘密与离线 fixture，未读取用户模型 Key、调用商业 API或执行工具。源码 `95df007` 的 [CI 37575612338](https://github.com/bboytang/CAIdex/actions/runs/37575612338) 在 Linux/Windows/macOS 全部成功：每个平台 12 项协议、7 项 HTTP 测试通过，既有固定 Runtime 回归 Linux 23、Windows/macOS 各 22 项通过。该 Runtime 回归尚不验证 Anthropic 互操作。

新增 6 项原生 SSE socket 测试：签名/工具/累计 usage、完成后主动关闭、静态错误分类、EOF/大小上限、Drop/取消、满队列 deadline/错误保存/许可释放、idle timeout/content-type。源码 `4b856b8` 的 [CI 37576472343](https://github.com/bboytang/CAIdex/actions/runs/37576472343) 三平台全部成功，逐平台确认协议 12/HTTP 13 项通过；既有 Runtime Linux 23、Windows/macOS 各 22 项回归通过。尚不验证 Anthropic Runtime 互操作。

## 后续顺序

1. 原生 HTTP SSE 三平台已验；本轮回复投影/回放新增 7 项测试、经典/Lite 真实 Runtime 载体各一项本机通过，完成本轮 workspace 与 CI 验证。
2. 实现 Responses→Messages、工具/图片/推理/结构化输出映射和执行端原生 history；经典与 Lite 分别验收，不将未知字段静默丢弃。
3. 完成 ModelProvider 六方法、原生认证需求元数据、Gateway 注入及固定 Runtime 多轮/工具/interrupt 离线验收；必要的 Runtime 修改保持最小范围。
4. 再进入 Gemini。真实提供商兼容性与付费调用须另行明确授权，离线成功不授予 Full 标签。

## 官方契约依据

- [Messages API](https://platform.claude.com/docs/en/api/messages/create)：原生请求、认证版本及不修改签名内容的回放要求。
- [Models API](https://platform.claude.com/docs/en/api/models/list)：after_id 分页、模型元数据和声明的能力。
- [Streaming](https://platform.claude.com/docs/en/build-with-claude/streaming)：事件生命周期、工具 JSON/签名增量、累计 usage。
- [Stop reasons](https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons)：暂停、拒绝、上限和正常结束应区分。

- [OpenAI reasoning](https://developers.openai.com/api/docs/guides/reasoning#preserve-reasoning-without-stored-responses)：Responses 的 opaque 回放字段；CAIdex 原生载体仅复用固定 Runtime 的传输槽位，不是 OpenAI 密文。
- [Anthropic prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching#tracking-cache-performance)：总输入计数为三项相加，不能只取非缓存 input_tokens。
