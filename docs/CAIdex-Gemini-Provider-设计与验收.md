# CAIdex Gemini Provider：设计与验收

阶段 F/G，执行基准为 V3。当前实现原生 Models/catalog 和 HTTP 基础，代码在原方案的 `model/providers/google`，包名 `caidex-provider-google`。生成、Responses 转换、ModelProvider 六方法、Gateway/实际 Runtime 接线尚未实现；本阶段不授予 Gemini Codex Full。

## 原生协议与配置

- `GeminiConfig` 固定默认 `https://generativelanguage.googleapis.com/v1beta/`；高级地址沿用现有 endpoint 验证：HTTPS 或 literal loopback HTTP，拒绝 userinfo/query/fragment，保留代理路径前缀。
- 执行端 `CredentialRef` 必须为 `google`/`ApiKey`，经 owner 固定的 Broker 解析。认证只用 sensitive `x-goog-api-key`，不放 URL，不探测用户环境，不向手机返回保存的 Host Key。[官方认证](https://ai.google.dev/gemini-api/docs/api-key)
- `GeminiClient::discover_models(max_models, RequestContext)` 请求原生 `models`，稳定 `pageSize=1000`，仅将原样 `nextPageToken` 编码为后续 `pageToken`。页 token 不选择 endpoint；失败不返回部分列表。[Models reference](https://ai.google.dev/api/models)
- 当前拒绝所有非空 Runtime context headers，在凭据读取前返回 `unsupported_native_context_header`；后续生成接线时按已验证映射开放，不把 OpenAI 专有头直接送给 Google。
- 复用 Custom 的 Limits、ClientOptions、endpoint 验证和 Retry-After 解析；原生 HTTP 用本模块私有实现，沿 Anthropic 模式，不为不同认证方式增加通用传输抽象。Cargo.lock 只新增 workspace 包，第三方版本未变。

## 模型目录与证据边界

- `ModelsPage` 校验安全 `models/<id>`、必需非空 `baseModelId`/`version` 与已知字段类型；完整 native model JSON 保留，含未知字段与任意精度数值。Debug 不打印 wire。
- 默认空 repeated 字段可省略；空/null models 视为空页，缺省/null/空 nextPageToken 结束。支持空中间页继续分页，拒绝同页/跨页重复名称和 cursor 循环；失败 append 不改变已有 accumulator。[ProtoJSON](https://protobuf.dev/programming-guides/json/)
- `ModelCatalog` 有模型数量上限，只在最后一页后提供排序的完整列表。网络 discovery 另共享总字节预算和绝对 deadline，约束空页链。
- metadata 标记 ProviderCatalog，只采用显式 thinking、正 input/output token limits；字段缺失为 Unknown。supportedGenerationMethods 仅表示目录中的方法可用性，不证明工具、流式、图像或 Codex 兼容性；不按模型名称推断，不生成 CompatibilityReport。

## HTTP 生命周期与凭据边界

- 一次完整 discovery 共用一个 semaphore slot、总 deadline 和累计 response_bytes；编码后的完整 URL 受 request_bytes 限制，超限页在读取 Key 前拒绝。
- Broker 同步读取进入 blocking pool，header timeout 包含读取与发送；caller deadline/total/header/idle 和 CancellationToken 均有效。已开始的同步存储读取无法强制停止，但取消后不发送迟到 GET；HTTP 被取消时实际关闭未完成 socket，并释放 slot。
- 禁止 redirect、请求重试和默认环境代理；保留原生 TLS trust/hostname/expiry 校验，只允许执行端显式增加公共 CA，无跳过验证选项。
- 检查成功 JSON content-type/有界正文。认证、限流、拒绝、redirect、服务端错误使用安全分类，不回显错误正文/URL/第三方 error；429 的 Retry-After 只返回解析后的秒数。

## 验证与恢复点

- 本地 catalog4 + HTTP9 已通过，目录和 HTTP 正例各有实现前 RED → GREEN；随后完整 workspace/Clippy 日志通过，fmt/diff 检查通过。日志 `/tmp/caidex-google-{catalog-red,catalog-green,http-red,http-green,native-foundation}.log`、`/tmp/caidex-google-foundation-{workspace,clippy}.log`。
- 覆盖 raw/大整数保留、能力 Unknown、完整有界分页/重复/cursor 循环、分页 token 编码、原生 header 认证、错误凭据 kind/provider/owner、无效 header Key、未配置/预取消/过期/context 拒绝时无 HTTP、累计预算、无重试、slot 与真实 socket 关闭、阻塞凭据读取取消。
- TLS fixture 在内存生成 CA/leaf/私钥：可信且 hostname/expiry 有效时 GET 成功；未信任 CA、hostname 不符、过期均失败且服务器未收到认证 HTTP。所有 Key/模型/响应为合成 fixture，不调用商业 API。
- 独立只读审查无 Critical/Important；源码提交/push 与本阶段三平台 CI 尚待完成；旧 Anthropic CI 不能作为 Google 新源码证据。当前普通 workspace 不执行 ignored Runtime/原生服务，Foundation CI 会显式执行既有验证。

## 下一步

1. 完成本阶段独立审查及三平台验收，更新 HANDOFF 与本文件的精确 source SHA/CI。
2. 原生 generateContent/streamGenerateContent 的请求、回复、SSE 与终态；保留完整 Part/thoughtSignature/functionCall/functionResponse wire 和 usage，不混用 Interactions 的 signature/事件结构。[GenerateContent reference](https://ai.google.dev/api/generate-content)
3. 建立版本化原生历史、请求/工具/图片/推理/结构化输出转换，再接现有六方法、Registry、Gateway 与固定真实 Runtime；经典/Lite、审批/工具/取消/恢复分别验收。不执行工具，不创建第二套 Agent，不隐式降级或标商业 Full。

## 独立审查记录

本阶段唯一 fresh-context 只读审查包括新 Google 文件、workspace 清单和文档。无 Critical/Important；三项 Minor 暂缓，非已修复：

1. `catalog.rs` 的 token limits 只接受 canonical JSON 非负整数；ProtoJSON 允许的字符串/整值浮点或指数表示会拒绝整个目录。Google 当前规范输出普通整数；未来有实际兼容端点证据再扩展精确解析，原 raw 不变。
2. 目录/HTTP 的大整数相等比较使用同一 serde_json 配置，不能独立检测双方同时舍入；当前 core arbitrary_precision 已启用。后续补原始数字字面值断言。
3. caller deadline 用例在读凭据/建连前开始 100ms 计时，繁忙 runner 若未到 socket 阶段已超时，其断连断言可能失败；本机通过，三平台待验。若实测发生先复现并修同步，不跳过用例或放松生产 timeout。

审查排除项及裁定（已确认阶段边界，非架构重规划）：

- 生成/SSE/历史/六方法/Gateway/Runtime 接线仍按下一步实施；此目录基础不提供这些能力。若误判，会漏接 Provider，因此保持明确未完成。
- 商业行为/Full 无合成 fixture 之外证据，不授予兼容等级；错误成本是错误能力承诺，须另获真实 API 授权并验收。
- 已开始同步 SecretStore 读无法强停，沿 Broker 现有边界；只保证取消后无迟到 GET。成本是后台读取仍占资源，不宣称强制中止。
- 本模块不是通用 ProtoJSON codec；未来字段原样保留，替代字段拼写等完整 codec 语义未承诺。成本是非 canonical 兼容服务可能被拒绝，具体整数边界见 Minor1。
- Windows/macOS 不以旧 Anthropic CI 代验；本阶段新 CI 完成前保持待验。成本是未发现平台问题，依赖后续精确源码三平台检查。
