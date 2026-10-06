# CAIdex 模型核心与 Gateway：设计和验收

阶段 F/G，依 V3 顺序推进。本轮完成 F 第一步 `model/core` 的协议核心及真实 Runtime 经典/Lite wire 验证。HTTP Gateway、Provider Adapter、模型 Registry 和实际模型兼容性尚未实现；不能将本轮当作 F/G 全部验收。

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
- 终态后的配对 CRLF 尾字节、注释/空行可以解码；finish 后状态保持，不允许重开。取消只关闭本模块解析；实际取消 socket/task、超时和背压待 HTTP 实现，不能冒称网络已取消。
- 不把 [DONE] 或 EOF 当作 Responses 成功，不做模型 POST 自动重试。Chat Completions 的 [DONE] 转换将由对应 Adapter 处理。

SSE framing 依据 [WHATWG 标准](https://html.spec.whatwg.org/multipage/server-sent-events.html#event-stream-interpretation)。终态处理也核对了 [官方完成事件指导](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference#3-wait-for-completed-inference)，历史/工具 correlation 与 encrypted reasoning 核对 [Responses 迁移清单](https://developers.openai.com/api/docs/guides/migrate-to-responses#incremental-rollout-checklist)。

## 不透明数据的边界限制

- 本轮 core 保留“收到的 wire”。固定上游在 non-OpenAI 请求构造时清除 internal metadata/encrypted_function_args；Runtime 反序列化也可能丢掉未知字段。Gateway 无法恢复早已被上游删除的数据。
- 真实用例证明 encrypted_content 进入下一轮；额外 provider_signature 只证明 core 解析/序列化保留，未宣称 Runtime 会把未知签名回传。
- F/G 各 Adapter 必须建立 provider-owned opaque context 与正确的原生历史重建，必要时评估固定上游最小补丁；在提出兼容性通过前验证完整多轮 round-trip。跨提供商不直接复用他方签名，按 V3 使用关联分支/新线程。

## 验证与恢复点

- 本地 fmt、Clippy -D warnings、workspace tests 通过；新增核心回归 15 项，包含每个字节分割点/逐字节、多种换行、未知数据/工具/用量、取消/EOF/乱序/身份混合/终态冲突。
- 真实 Runtime Linux 17 项通过（既有 15 + 新经典/Lite 2）；新两项均完成两轮请求，并使用本地合成推理数据。当前尚待本轮 GitHub 三平台验证（预期 Linux 17/Windows、macOS 16）。
- 未实现/未验证：HTTP Gateway/流背压/socket 取消，HTTP 错误/限流与安全诊断，凭据 Adapter 接入、模型能力 Registry，真实 Provider/商业推理、Lite Code Mode 工具执行、远端 opaque compaction。E 原生 keyring 回归继续由 CI 保持。

## 下一步顺序

1. 提交本轮协议核心、fixture/回归和文档，完成三平台 CI，记录结果。
2. F 第二步：实现受控的本地 Responses HTTP/SSE Gateway 与 Custom Responses Adapter；模型路由和目标端点来自显式配置，认证经执行端 Broker；用合成认证/本地服务验收端到端转发、真实 Runtime、断开/取消、背压、超时、HTTP 429/错误，不自动重放 POST。
3. 完整 Provider 接口/模型 Registry 随 Adapter 实现落地，依次适配 OpenAI、Anthropic、Gemini、兼容 API/Ollama，补请求/响应/工具/usage/reasoning/images/结构化输出/context/capabilities/prompt compatibility。不要把 Responses pass-through 当作最终跨提供商 Gateway。
4. 按每个模型的真实能力验收经典与 Code Mode、多轮历史/签名/切换。实际用户凭据复用/创建及付费调用前明确授权；已授权离线协议工作继续。
