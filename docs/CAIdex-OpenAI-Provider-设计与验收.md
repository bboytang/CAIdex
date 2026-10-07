# CAIdex 原生 OpenAI Provider：设计与验收

阶段 F/G 的 OpenAI 离线适配。仅使用合成凭据和 loopback 协议服务；实现 API 合约不等于真实模型、账户或 Codex Full 兼容性通过。执行方案仍以 V3 为准，其他 Provider、生产 Host、Chat 与 UI 按后续阶段推进。

## 原生发现与模型清单

`OpenAiConfig` 默认 base 为 `https://api.openai.com/v1/`。`discover_models(context)` 对固定 profile 的 `models` 路径发 GET，解析 `object=list`、`data` 内的 id/object/created/owned_by/可选 shutdown_date，按 ID 排序并保留未知原生字段和大整数；重复或格式错误的清单失败，空清单有效。请求继承取消/整体 deadline，采用现有认证/TLS/并发/大小/超时边界，无隐式缓存、重试或降级。

官方 Models API 提供可见模型的基础信息，不提供 context window、输出上限或 Responses/Codex 兼容性。不能从模型名字猜能力，也不能把音频/embedding 模型自动配置为 Responses 路由。[Models API](https://developers.openai.com/api/reference/resources/models/methods/list)

因此完整原生清单与六方法接口 `list_models()` 分开：后者只返回显式配置的 Responses profile 与原生可见 ID 的交集，保留 CAIdex alias、配置的 dialect 与声明能力，source 为 `ProviderCatalog`。`metadata`/`capabilities`/`credential_requirements` 查询固定配置，不读 Key 或先联网；调用不会先重复发现清单。发现失败不返回旧配置伪装成成功。Registry 明确拒绝以 Configured/ProviderCatalog 作为兼容性报告证据，Full/Compatible 仍须实际 LiveRuntime 报告。

## 执行端认证与 routing scope

配置必须引用 `provider=openai`、kind 为 ApiKey 或 AccessToken 的 Broker reference；owner/provider/profile/kind 隔离不变。AccessToken 只是明确的 Bearer 引用，并未实现 OAuth 登录、刷新或自动导入 Codex 登录。默认无环境搜索/fallback，无手机读取 Host Key。

原生认证使用 Bearer；组织和项目可通过 OpenAI-Organization/OpenAI-Project 选择。[官方认证契约](https://developers.openai.com/api/reference/overview#authentication)

组织/项目仅来自执行端固定 `OpenAiConfig::with_scope`，不是模型 JSON、Gateway 请求头或同步历史。GET Models 与 POST Responses 共用相同 scope/reference。换 base 必须显式配置，URL 沿用 HTTPS/字面 loopback HTTP、无 userinfo/query/fragment 的规则；拼接 models/responses 保留代理路径前缀。Scope 非空、可见 ASCII、最多 1024 bytes，HeaderValue 标记 sensitive，Debug 不输出 profile。

## Responses 与历史边界

原生 Provider 实现 ModelProvider 六方法，Responses 创建/流式调用委托已有独立 client，不新增第二套 HTTP/SSE 或 Agent。Alias 仅替换顶层 model；请求图片、结构化输出、tools、reasoning、精确 usage、函数参数字符串及未来字段保留。工具始终为数据，由真实 Runtime 或普通 Chat 自身允许的执行器处理。

`stream=true` 使用类型化 SSE；经典/Lite 来自显式配置的 dialect，Lite 保留内部 header 与原始 input 编码，不能据此宣称公开 API/所有模型接受 Lite。[Responses 流式契约](https://developers.openai.com/api/docs/guides/streaming-responses)

请求缺少或 null 的 `store` 默认置 false，显式 true 为调用方存储 opt-in，显式 false 保留；非布尔值拒绝。无服务端存储时，调用方回放完整 output，保留 reasoning encrypted_content、message phase 等原生上下文；当前官方文档说明 stateless 回复默认含 encrypted_content，旧 include 参数仍接受，不额外强制添加它。[无存储的推理回放](https://developers.openai.com/api/docs/guides/reasoning#preserve-reasoning-without-stored-responses)

当前契约只支持前台推理，background=true 返回 `unsupported_background_generation`，不静默改写；background 非布尔拒绝。远端后台任务的单独取消 API 未实现。HTTP 200 的 failed/incomplete 保持失败/不完整状态，不变为 completed。

Adapter 直接两轮测试验证收到的 output 中 encrypted_content、encrypted_function_args、signature、phase 与未来字段进入下一轮。它不证明固定 Runtime 已保留未知签名或 non-OpenAI 分支删除的字段。固定 Runtime 的经典/Lite 两轮另行验证 encrypted_content 与稳定前缀；原生 opaque history 重建、跨模型切换/压缩与必要的 Runtime 最小补丁仍属 F/G 后续工作。

## Gateway 注入与边界

`start_with_provider(Arc<dyn ModelProvider>, broker.redactor(), limits)` 启动纯 loopback Gateway，不发现模型/读 Key/发推理。使用实际执行端 Broker 的 Redactor 注册随机独立 Gateway token，不能将其作为上游 Key。旧 `start(routes, broker, limits)` 入口保持有效。

Gateway 在调用 Adapter 前自行验证模型 metadata/ID、dialect 和明确不支持的 streaming；不靠 Adapter 自觉执行。整体取消/非流式 deadline、流式建连 header deadline 在 Gateway 再约束一次。Adapter 返回的 context header 只能是现有两项 RESPONSE_HEADERS，错误方向的 header 返回安全 502。调用方认证/cookie/组织/项目/API Key 不透传，组织/项目始终由固定 profile 添加。

## 验收与恢复点

- 新增 `model/providers/openai/tests/provider.rs` 11 项实际 socket 测试：原生 GET/清单交集/空或坏清单、原生未来字段、六方法、固定 scope 与认证、两轮回放/图片/工具/结构化输出、store opt-in/前台门控、经典/Lite SSE、取消/timeout/Drop/slot 释放、HTTP 分类/429/redirect/大小/类型与注入 Gateway。
- 新增 `model/gateway/tests/injected.rs` 4 项：模型/dialect/能力/metadata 门控、响应头方向、Adapter 忽略 context 时的两种 deadline、shutdown Drop；均经实际 Gateway HTTP。
- 复用现有核心 20、Custom 7、Gateway HTTP 18 项回归，包含 TLS 信任/hostname/过期、畸形或截断 SSE、诊断脱敏等边界；这些不是商业模型证据。
- 本机上述定向回归、完整 workspace、fmt/Clippy/diff 与固定 Runtime Linux 23 项已通过；三平台 CI 待验，结果在 HANDOFF 与此文档更新，不提前宣称已验。
- 新增真实 Runtime 测试分别使用原生 OpenAI Adapter 经典/Lite 两轮和两条路径 interrupt；回复合成、不收费、不配置真实模型 Key。Runtime 生产代码/固定版本及品牌资产未改。
- Cargo.lock 只新增 workspace crate 与 Runtime dev-dependency 引用，没有新增或升级第三方 package。

下一步：通过本机完整回归及 GitHub 三平台 CI 后，按 V3 接入 Anthropic，再 Gemini、兼容 API/Ollama；逐 Provider 验证原生历史/工具/usage/能力与兼容性报告，之后推进 H/I 客户端基础。真实 API/Key 授权、iOS 工程/真机/签名/UAT 尚未完成。
