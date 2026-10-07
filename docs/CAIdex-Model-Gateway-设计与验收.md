# CAIdex 模型核心与 Gateway：设计和验收

阶段 F/G，依 V3 顺序推进。F 第一至第三步当前范围已有三平台验收，第三步包括 ModelProvider、CanonicalResponse、模型 Registry 和独立 Custom Responses client。其他原生 Provider 和实际模型兼容性尚未实现，不能将本轮当作 F/G 全部验收。

原生 OpenAI Models/Responses 与 Gateway 注入入口已在源码 e7253c8 实现，[CI 37573720266](https://github.com/bboytang/CAIdex/actions/runs/37573720266) 三平台全部通过；新增契约和限制见 [OpenAI Provider 设计与验收](CAIdex-OpenAI-Provider-设计与验收.md)。Anthropic/Gemini/兼容 API/Ollama 与真实模型兼容性仍待后续。

## 固定协议依据

源码固定 `d27764b82f7118f674371e6d6e76271d9d606edb` / Codex 0.160.1，不从当前 main 或未知模型 fallback 推断行为。

| 路径 | 固定上游实际行为 | 本轮验证 |
| --- | --- | --- |
| 经典 Responses | `/responses`，instructions/tools 在顶层；使用 bundled gpt-5.5 元数据路径 | 真实 Runtime 两轮、本地服务捕获请求和 opaque reasoning 回放 |
| Responses Lite | 同一路径，header `x-openai-internal-codex-responses-lite: true`；instructions/tools 不在顶层；input 内 additional_tools + developer message；稳定 at_/msg_ ID，reasoning.context=all_turns，parallel_tool_calls=false | bundled gpt-6.1-sol/code_mode_only 元数据选择、两轮稳定前缀/推理回放；没有执行 Code Mode 工具 |

Lite 的 tools 编码还依赖 Provider 的 namespace_tools 能力，不保证每个提供商都接受内部 Lite header。模型名称在上述测试中用于选择固定 Runtime 元数据，回复是本地合成事件，不代表商业模型推理或真实 API 兼容性已通过。

来源：[固定 client.rs](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/client.rs)、[Lite 上游回归](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/tests/suite/responses_lite.rs)、[请求定义](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/codex-api/src/common.rs)、[bundled metadata](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/models-manager/models.json)。

## 当前核心契约

- CanonicalRequest 保留完整 wire JSON，dialect 作为传输信息单独保存；不把 Lite 改写成经典，不通过重建已知字段丢掉扩展。
- ResponseItem/ResponseEvent 保留 JSON 对象，提供类型、工具、文本增量、usage/终态视图；未知 item/event 类型保留。已知事件缺少必要字段或终态状态矛盾被拒绝。
- Function/Custom tool call 统一提供 kind/call_id/name/namespace/input；arguments 保留原字符串（含空格/数字格式），freeform input 不转换为 JSON。核心不执行工具，不解析模型参数后“修正”它。
- Tool result 保留字符串或结构化 content array，含图片；经典 legacy function output 的 name/namespace 且无 call_id 形式保留。自定义 tool output 要求 call_id。
- Usage 将 input/output/total 计数提为可选非负整数，同时保留原 cache/reasoning/cost/未知字段。缺失不伪造为 0。
- encrypted_content、encrypted_function_args、签名、compaction、模型扩展和嵌套未知字段保持 JSON 语义；serde_json 启用 arbitrary_precision，未知大整数/高精度小数不降为 f64；字符串原值不变，不承诺整个 JSON 的字段排序/空白或数字词法形式逐字节一致。
- wire 包装类型的 Debug 不输出正文；错误只用安全枚举，不附原始 JSON/第三方 error。真实传输/诊断接入时仍必须使用凭据 Redactor，不靠 Debug 就宣称所有日志安全。

## SSE 与轮次终态

- SseDecoder 支持 UTF-8 跨分片、首 BOM、LF/CRLF/CR、注释、多行 data、id 保持/重置、retry 数值；只在完整空行分隔后派发，EOF 不补发残帧。
- 帧上限由调用方显式提供，覆盖同一帧的多行；CRLF 视为一个换行终结符。非法 UTF-8 直接失败，避免替换字节损坏签名/推理数据；这是针对模型协议的严格处理，不宣称完全等同浏览器 EventSource。
- ResponsesStream 区分 Open/Completed/Interrupted/Incomplete/Failed/Cancelled/Truncated/Invalid。只有 response.completed 可作为成功；response.incomplete 的 interrupted 单独标识，其他原因保留不完整。response.failed/error 为失败。
- 已提供的 sequence_number 要递增，允许缺省/间隔；同一流不能混合 response.id。显式 SSE event 名与 JSON type 矛盾、坏编码/坏 JSON/超限、终态后新的事件均失败关闭。
- 终态后的配对 CRLF 尾字节、注释/空行可以解码；finish 后状态保持，不允许重开。core 的取消只关闭解析；HTTP Gateway 另行负责取消 task/socket，见下。
- 不把 [DONE] 或 EOF 当作 Responses 成功，不做模型 POST 自动重试。Chat Completions 的 [DONE] 转换将由对应 Adapter 处理。

SSE framing 依据 [WHATWG 标准](https://html.spec.whatwg.org/multipage/server-sent-events.html#event-stream-interpretation)。终态处理也核对了 [官方完成事件指导](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference#3-wait-for-completed-inference)，历史/工具 correlation 与 encrypted reasoning 核对 [Responses 迁移清单](https://developers.openai.com/api/docs/guides/migrate-to-responses#incremental-rollout-checklist)。

## 本地 HTTP/SSE Gateway

- `model/gateway` 是供执行进程嵌入的 Rust 库，入口 `start(routes, Arc<Broker>, Limits)` 返回 `RunningGateway`；目前没有生产 Host/CLI 启动或配置管理入口。只监听 `127.0.0.1` 随机端口，提供 `POST /v1/responses` 的 stream=true SSE 和非流式 JSON。
- 每次启动生成 256-bit 本机 bearer token，常数时间比较；无凭据、重复 Authorization、Origin、query、非 JSON/压缩输入拒绝。token 加入 Broker 的诊断 Redactor，只给隔离 Runtime 的子进程环境，不写入 config/历史。它不是提供商 API Key，也不替代后续 Host 身份授权。
- ModelRoute 明确公开 model → upstream model/endpoint/profile 和允许的经典/Lite dialect；未知模型、重复路由或不支持 dialect 拒绝，无默认提供商/环境 Key 回退。只改 model 名，其余未知请求字段、input 前缀、图片及工具字符串保留。
- CustomResponses 使用准确 Responses endpoint，支持显式无认证或 Broker 的 bearer credential；Broker 在执行端解析 reference、校验 owner，阻塞 native backend 放在 blocking pool。已发起的同步存储读取不能强制中止，超时/取消后不会继续向提供商发请求。
- URL 拒绝 userinfo/query/fragment；明文 HTTP 只允许 literal loopback，其他地址需 HTTPS。reqwest 的 rustls 默认验证配置保持启用，关闭环境 proxy、redirect、POST 重试和空闲连接池；已验证本地 TLS 正/负证书 fixture，尚未做真实提供商证书链/推理兼容性验证。
- 不转发客户端 Authorization、cookies/任意头；提供商 Key 仅构造上游 Authorization 并标记 sensitive，Lite header 由已验证 dialect 生成。不向 Runtime 暴露保存的 Key。明确允许的 context headers 见下；其他提供商头需对应 Adapter 验证后接入。
- SSE 使用核心逐帧验证，派发完整事件；保留原 data 字符串和 id/retry 语义，不承诺原始注释/换行逐字节一致。上游字节持续活动而没有模型事件时，最多每秒发送规范化注释 heartbeat；不把部分 JSON 或 heartbeat 当作成功事件。合法终态后立即结束此次上游传输。
- 单槽有界发送队列、分片解析和 in-flight permit 提供背压；不积攒整个流。客户端丢弃 HTTP Body 会 abort producer、释放 permit 并关闭上游 socket；响应头返回前断开也取消请求。显式 shutdown/handle Drop 终止监听及活动生成。
- connect/header/idle/absolute deadline 与 request/frame/nonstream response 大小有明确可配置上限。absolute deadline 在下游不读时仍生效；坏 UTF-8/JSON/序号/response ID、超限、EOF 缺终态关闭流，能投递时提供安全 error 事件；背压超过 deadline 时直接关闭，不无限等待错误通知。
- HTTP 401/403/429/5xx/redirect 分类为静态错误，丢弃第三方 body/cookie/Location。429 接受数字或 HTTP-date Retry-After，转为不超过一天的秒数提示，日期向上取整、过去日期为 0；坏值/超限丢弃，不自动等待/重发。SSE error（含顶层 message）/response.failed 与非流式 response.error 只在诊断部分使用 Redactor；opaque item/history 不按日志脱敏。
- 默认上限：请求 8 MiB、SSE 帧 2 MiB、非流式回复 16 MiB、同时 16 个请求；connect 10s、header 30s、idle 90s、total 600s。这些是传输边界，不冒充具体模型 context/output 能力。

固定 Runtime 接入：base_url=`http://127.0.0.1:<port>/v1`，wire_api=`responses`，env_key 指向仅该子进程注入的 Gateway token，requires_openai_auth=false。必须设置 request_max_retries=0 和 stream_max_retries=0；Gateway 禁重试不能替调用方关闭 Runtime 自身重试。生产 Host/CLI 接入待 H/P，当前真实验证在隔离测试 harness 内完成。

HTTP 库依据：[reqwest 0.13.5 禁重试](https://docs.rs/reqwest/0.13.5/reqwest/retry/fn.never.html)、[ClientBuilder](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html)、[axum 0.8.9 HTTP body](https://docs.rs/axum/0.8.9/axum/body/struct.Body.html)。依赖锁文件新增传输/TLS 包，未升级已有包版本；TLS 编译需要 C 工具链，三平台实际构建由 CI 验证。

## 共用 ModelProvider 与模型 Registry

- `model/core::ModelProvider` 是 object-safe 的六方法接口：list_models、create_response、stream_response、capabilities、metadata、credential_requirements。异步方法返回 Send boxed future/stream；RequestContext 提供取消 token、调用方 absolute deadline 和明确的非认证 context headers。
- `model/providers/custom` 实现全部六方法，不依赖 axum/Gateway/工具执行器。Gateway 调用同一 ModelProvider；普通 Chat 可独立调用 create_response/stream_response。当前 list_models 是经验证配置的确定性模型清单，source=Configured，不伪装成向未知 Custom endpoint 自动发现模型。
- CanonicalResponse 验证 id、终态、output item/工具字段和 usage，保留未知字段/opaque/数字精度；提供文本、输出、usage、Completed/Incomplete/Interrupted/Failed 视图。HTTP 200 的 failed/incomplete 仍保持原状态。
- Registry 区分 Unknown/Supported/Unsupported 的声明能力，包含 V2 所列 text/vision/reasoning/tools/parallel/structured output/streaming/web search/image generation/context/output limit/prompt profile。未知能力/上限不编造；显式 Unsupported streaming 在读取凭据或网络前拒绝。其他能力的原生转换与约束随 Adapter 实现验收，当前不自行改写工具或降级模型。
- 模型 metadata 与路由绑定，不允许通过 metadata 更改 model ID、native model 或 dialect。CredentialRequirement 仅公开 owner/provider/profile/kind reference，不返回 Secret；配置清单和 metadata 查询不读取凭据。
- Codex compatibility 没有默认 Full；缺报告为未验证。版本化 CompatibilityReport 有 level、source、测试版本、证据引用和 limitations；Configured 不是报告，协议 fixture 不能标 Full/Compatible，这两级至少需要 LiveRuntime（实际模型 + 固定 Runtime）来源。Registry 验证报告结构/来源门槛，不代替执行端审核报告真实性，也不把真实 Runtime 配合合成 fixture 当作 LiveRuntime 证据。
- 提供商流只有一个有界事件槽，Drop abort producer 并关闭 socket；取消或 absolute deadline 在消费者不读取/队列已满时仍关闭 I/O。安全失败另行保存，消费者恢复读取后先排出排队事件、再得到一次明确错误，不无限等待错误入队。Gateway 再编码 SSE，EOF 没有模型终态不能成功。
- 已取消/已过期请求在凭据访问前拒绝；同步 backend 已开始的读取不能强行中止，但取消后不会 POST。调用方整体 deadline 同时约束认证读取、HTTP headers 和流。

## Context headers 与 TLS 验收

| 方向 | 明确允许的 headers |
| --- | --- |
| 调用方 → provider | session_id、x-client-request-id、x-codex-turn-state、x-codex-turn-metadata |
| provider → 调用方 | x-request-id、x-codex-turn-state |

这些是固定 [client.rs](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/client.rs) 和 [Responses SSE](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/codex-api/src/sse/responses.rs) 中核对的当前契约，未宣称覆盖所有上游 header。重复、非可见 ASCII 或超过 8192 bytes 的 context 值拒绝；opaque turn-state 的 Debug 省略值，不实现 Serialize/Display。其他头不透传；上游非法 context 返回安全 502，客户端重复 context 为 400。认证头始终由 Broker 构造。

ClientOptions 允许为该独立 client 显式添加公共信任根，不提供跳过 TLS 验证选项。fixture 在内存生成 CA/叶证书/私钥，明确不同的 issuer/subject、用途/AKI/有效期以支持原生证书链验证：可信有效证书成功，未信任 CA、主机名不符、过期证书均失败；负例服务器未收到 HTTP 请求，证明 Bearer 不穿过失败握手。私钥不落盘/入日志。额外公共根接入最终设置及真实服务链兼容性仍待相应阶段。

实现依据：[add_root_certificate](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html#method.add_root_certificate)、[HTTP-date parser](https://docs.rs/httpdate/1.0.3/httpdate/fn.parse_http_date.html)、[rcgen 0.14.10](https://docs.rs/rcgen/0.14.10/rcgen/)。

## 不透明数据的边界限制

HTTP Gateway 通过以下边界保留收到的 wire；不会恢复上游已经删除的数据。

- 本轮 core 保留“收到的 wire”。固定上游在 non-OpenAI 请求构造时清除 internal metadata/encrypted_function_args；Runtime 反序列化也可能丢掉未知字段。Gateway 无法恢复早已被上游删除的数据。
- 真实用例证明 encrypted_content 进入下一轮；额外 provider_signature 只证明 core 解析/序列化保留，未宣称 Runtime 会把未知签名回传。
- F/G 各 Adapter 必须建立 provider-owned opaque context 与正确的原生历史重建，必要时评估固定上游最小补丁；在提出兼容性通过前验证完整多轮 round-trip。跨提供商不直接复用他方签名，按 V3 使用关联分支/新线程。

## 验证与恢复点

- 本地 fmt、Clippy -D warnings、workspace tests 通过；新增核心回归 15 项，包含每个字节分割点/逐字节、多种换行、未知数据/工具/用量、取消/EOF/乱序/身份混合/终态冲突。
- [三平台 CI 37550000148](https://github.com/bboytang/CAIdex/actions/runs/37550000148) 全部通过，源码 `ea7d902`：核心 15 项和 Gateway 16 项在 Linux/Windows/macOS 通过；真实 Runtime Linux 20 项、Windows/macOS 各 19 项通过，含新增 Gateway 经典/Lite 两轮/Broker 认证和两条路径 interrupt 后实际 EOF/reset、无重放。既有协议/凭据/schema/doctor 继续通过，HTTP/TLS 依赖三平台构建通过。
- 本机 workspace/fmt/Clippy、Gateway 16 项、真实 Runtime Linux 20 项通过；最终大整数/高精度小数 wire 和 EOF/reset 判据分别定向复验通过。全部使用本地合成推理数据，不代表真实商业模型兼容性。
- F 第三步 [三平台 CI 37552752561](https://github.com/bboytang/CAIdex/actions/runs/37552752561) 全部 success，源码 `42fdaa3`（接口源码 e834549 + 证书 fixture 修正）：核心 20、独立 provider 7、Gateway 18 项各平台通过，真实 Runtime Linux 20、Windows/macOS 各 19 项通过。新增验证包含独立 client 六方法/经典与 Lite、配置能力与未验证标记、完整同步 response、header 双向筛选、HTTP-date、单并发 slot 释放、未消费流取消/超时、TLS 正/负证书；fmt/Clippy/workspace/schema/doctor 及既有凭据回归保持通过。
- 本机 workspace/fmt/Clippy/真实 Runtime 20 项通过，单并发 slot/TLS 定向复验通过。首轮 Windows TLS 正例被拒绝，以明确 CA/leaf identity、用途/AKI/有效期的 fixture 修正后全部通过，未放宽生产 TLS。日志 `/tmp/caidex-ci-37552752561.log` 供本机复查，跨机器以 CI 链接为准。
- 未实现/未验证：其他原生 Provider、原生 opaque history 重建、真实 Provider TLS/商业推理、Lite Code Mode 工具执行、远端 opaque compaction、其他 provider-specific headers。E 原生 keyring 回归继续由 CI 保持。

## 下一步顺序

1. OpenAI 当前离线适配范围已通过三平台验收，按 V3/原 V2 第 14–22 节继续 Anthropic→Gemini→兼容 API/Ollama，使用已验六方法接口与 Registry，先以本地合成 fixture 验证原生 Adapter/模型清单；OpenAI 真实模型兼容性与原生历史限制继续保留验收项。
2. 补各 Adapter 原生请求/响应/工具/usage/reasoning/images/结构化输出/context/capabilities/prompt compatibility；不要把 Responses pass-through 当作最终跨提供商 Gateway。
3. 原生 opaque history、经典/Code Mode、多轮签名/模型切换分别验收并生成实际证据报告。实际用户凭据复用/创建及付费调用前明确授权；已授权离线协议工作继续。


## 专用 tool_search wire 契约（2026-10-07）

核心新增 `ToolSearchCall` / `ToolSearchOutput` 借用视图，独立于 Function/Custom，保留 execution、可选 call_id/status、原 JSON arguments 和完整 tools 数组；不把搜索参数改成函数参数字符串，不编造 name/namespace。客户端 execution=client 必须有非空字符串 call_id；服务端和未来 execution 原值保留，不授予 Runtime 客户端执行权。arguments 按固定上游 Value 契约保留，声明 schema、已发现工具权限与调用/结果配对由 Adapter 校验，不能靠核心读取视图获得执行授权。

非流式 CanonicalResponse、SSE output_item.done 和 completed/incomplete/failed/error 所有终态内已提供的 search items 校验已知结构；坏字段拒绝，流进入 Invalid，不交付合法完成事件。output_item.added 可以仍不完整。未知字段、namespace/defer_loading 声明、高精度数字和原始 wire 保留；核心不联网搜索、不执行工具、不改 Runtime 或已有 ToolMap。

两项新增回归覆盖缺失客户端关联及已知字段、同步/流式完成边界、逐字节 SSE、服务端/未来 execution、原生 JSON 值和声明保留；缺少 call_id 被错误接受已先 RED 复现，再 GREEN。当前只完成共享协议契约，Anthropic 动态加载、原生 inline/deferred tools、历史前缀绑定及 provider web_search 仍待实现，不能据此标经典 Gateway 或 F/G 完成。尤其 external_web_access=false 不等于原生实时搜索，不能静默改写。

依据：[OpenAI Docs 客户端工具发现](https://developers.openai.com/api/docs/guides/tools-tool-search#client-executed-tool-search)、[固定上游 ToolSearchCall/Output](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/protocol/src/models.rs)。本轮测试与未同步状态见 HANDOFF.md；真实 API/商业模型及三平台新增契约尚未验收。
