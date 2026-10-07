# Anthropic 原生 Provider：实现与恢复点

## 当前范围

`model/providers/anthropic` 已实现原生 Messages/Models 数据结构、SSE 重建，以及独立原生 HTTP client。它是完整 Adapter 的基础，尚未实现 ModelProvider 六方法、Responses 经典/Lite 转换、Gateway 注入和固定 Runtime 互操作。

`AnthropicConfig` 固定执行端 API Key 引用（provider=anthropic、kind=ApiKey）、基址和可选 workspace。默认 HTTPS；显式代理/本地 fixture 复用既有 endpoint 安全策略，仅 literal loopback 允许 HTTP，不接受 URL 用户密码/query/fragment。HTTP client 保持 TLS 验证、禁用代理自动发现、重定向和自动重试；支持显式额外信任根。

Broker 只在发送时解析指定引用，不自动读取环境密钥。发送 `x-api-key`、`anthropic-version: 2023-06-01`，workspace 由执行端配置，均不来自请求 JSON。认证值标记 sensitive；错误只返回静态分类和安全 Retry-After 元数据，不传播 URL、原始 HTTP 错误或第三方错误 body。原生消息和事件是调用者数据，不是已脱敏日志；其 Debug 隐去 wire。

`discover_models(max_models, context)` 访问配置基址下的 Models 路径，使用 `limit=1000` 和 URL 编码的 `after_id`。完整分页共享单个并发许可、总 deadline、累计响应字节预算；重复 ID、坏游标、不完整结果和数量上限报错，不返回部分模型清单。能力仅由 Models 已声明的 image_input/thinking/structured_outputs 支持标记及正数 token 上限映射；缺失/null/零上限保持未知。名称不推导能力或 Codex 兼容性。

`create_message(model, native_json, context)` 发送非流式原生 Messages JSON：明确 max_tokens，固定 model 参数覆盖 body model，stream=false；保留原生消息内容及未知字段，限制请求/响应大小。它不是 Responses 请求转换，未全面验证提供商的所有输入 schema，其他参数是否合法仍由原生服务判定。携带尚未映射的 context headers 明确拒绝。并发、认证/header timeout、响应 idle timeout、总 deadline、取消和 drop 不触发重试；已开始的同步 SecretStore 读取不能强制中止，取消后不发送请求。

原生回复按 content 原顺序保留，包括 thinking/signature、redacted_thinking、工具及未来块；回放直接使用原内容。签名不做密码学验证，缺少完整签名的 thinking 当前拒绝回放。SSE 增量重建文本、签名、工具 JSON 和 citations，严格校验 lifecycle/index/usage；usage 累计值替换而非相加。未知事件/块保留；无法重建的未知 delta 在 block stop 报错，避免伪造无损 history。`Completed` 只表示收到完整 message_stop，具体 end_turn/tool_use/max_tokens/pause/refusal 等由 MessageOutcome 区分。

## 验证

- 12 项协议测试：所有字节切分及逐字节 UTF-8/SSE、签名和 opaque 顺序、工具输入、累计 usage、各 stop reason、未知字段/大整数、截断/取消/错误/预算、Models 分页与能力证据。
- 7 项真实 loopback HTTP 测试：认证/版本/workspace、游标编码、两轮原生签名回放、429 安全分类且不重试、取消及 missing Key 不发送、实际 socket EOF/timeout/并发许可释放、跨页字节和数量上限、配置拒绝。
- 使用合成秘密与离线 fixture，未读取用户模型 Key、调用商业 API或执行工具。源码 `95df007` 的 [CI 37575612338](https://github.com/bboytang/CAIdex/actions/runs/37575612338) 在 Linux/Windows/macOS 全部成功：每个平台 12 项协议、7 项 HTTP 测试通过，既有固定 Runtime 回归 Linux 23、Windows/macOS 各 22 项通过。该 Runtime 回归尚不验证 Anthropic 互操作。

## 后续顺序

1. 接入原生 HTTP SSE：单槽背压、取消/Drop/未消费队列 deadline、错误事件脱敏、完整结束校验及真实 socket 测试。
2. 实现 Responses→Messages、工具/图片/推理/结构化输出映射和执行端原生 history；经典与 Lite 分别验收，不将未知字段静默丢弃。
3. 完成 ModelProvider 六方法、原生认证需求元数据、Gateway 注入及固定 Runtime 多轮/工具/interrupt 离线验收；必要的 Runtime 修改保持最小范围。
4. 再进入 Gemini。真实提供商兼容性与付费调用须另行明确授权，离线成功不授予 Full 标签。

## 官方契约依据

- [Messages API](https://platform.claude.com/docs/en/api/messages/create)：原生请求、认证版本及不修改签名内容的回放要求。
- [Models API](https://platform.claude.com/docs/en/api/models/list)：after_id 分页、模型元数据和声明的能力。
- [Streaming](https://platform.claude.com/docs/en/build-with-claude/streaming)：事件生命周期、工具 JSON/签名增量、累计 usage。
- [Stop reasons](https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons)：暂停、拒绝、上限和正常结束应区分。
