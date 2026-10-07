# CAIdex Gemini Provider：设计与验收

阶段F/G，执行基准为V3，代码在原方案的model/providers/google，包名caidex-provider-google。Models/catalog、generateContent JSON、原生SSE解析及streamGenerateContent HTTP、v1原生历史/输出投影、工具身份映射与显式v2声明绑定历史已实现并三平台验收；最新源码`d2a363d64c80b63049b7bffa0325224955657983`，[CI37675979615](https://github.com/bboytang/CAIdex/actions/runs/37675979615)三平台completed/success，Google44项逐平台通过。Responses请求编译/工具结果配对/图片/参数转换、ModelProvider六方法、Registry/Gateway/实际Gemini Runtime尚未接；本阶段不授予Gemini Codex Full。

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
- 独立只读审查无 Critical/Important；源码 `d5a1132b449cae28923e2da7f2349e72b12696eb` 已提交/push，[CI 37657922506](https://github.com/bboytang/CAIdex/actions/runs/37657922506) 三平台 completed/success。逐平台核对新增 catalog4+HTTP9 全部13个测试名各通过一次；workspace/fmt/Clippy/native credentials/schema/doctor 均通过，既有真实 Runtime Linux30/WindowsmacOS29通过。普通 workspace 不执行 ignored Runtime/原生服务，CI已显式验证；macOS stdout/stderr标记有交错，按具体测试名和终态核对，不以 marker 顺序代替结果。日志 `/tmp/caidex-ci-37657922506{,-linux,-macos,-windows}.log`，跨机器以 CI 为准。

## 下一步

1. 本阶段独立审查及精确源码三平台验收已完成；沿以下顺序继续，不重复目录基础。
2. generateContent JSON、原生SSE解析和streamGenerateContent?alt=sse真实HTTP/单槽背压/取消、Drop、deadline与socket/slot生命周期已三平台验收，继续第3步；保留完整 Part/thoughtSignature/functionCall/functionResponse wire 和 usage，不混用 Interactions 的 signature/事件结构。[GenerateContent reference](https://ai.google.dev/api/generate-content)
3. 版本化原生历史与Responses输出投影已三平台验收；继续Responses请求/工具/图片/推理/结构化输出转换，再接现有六方法、Registry、Gateway 与固定真实 Runtime；经典/Lite、审批/工具/取消/恢复分别验收。不执行工具，不创建第二套 Agent，不隐式降级或标商业 Full。

## 独立审查记录

本阶段唯一 fresh-context 只读审查包括新 Google 文件、workspace 清单和文档。无 Critical/Important；三项 Minor 暂缓，非已修复：

1. `catalog.rs` 的 token limits 只接受 canonical JSON 非负整数；ProtoJSON 允许的字符串/整值浮点或指数表示会拒绝整个目录。Google 当前规范输出普通整数；未来有实际兼容端点证据再扩展精确解析，原 raw 不变。
2. 目录/HTTP 的大整数相等比较使用同一 serde_json 配置，不能独立检测双方同时舍入；当前 core arbitrary_precision 已启用。后续补原始数字字面值断言。
3. caller deadline 用例在读凭据/建连前开始 100ms 计时，繁忙 runner 若未到 socket 阶段已超时，其断连断言可能失败；本机及本次三平台均通过，调度风险仍暂缓。若后续实测发生先复现并修同步，不跳过用例或放松生产 timeout。

审查排除项及裁定（已确认阶段边界，非架构重规划）：

- 生成/SSE/历史/六方法/Gateway/Runtime 接线仍按下一步实施；此目录基础不提供这些能力。若误判，会漏接 Provider，因此保持明确未完成。
- 商业行为/Full 无合成 fixture 之外证据，不授予兼容等级；错误成本是错误能力承诺，须另获真实 API 授权并验收。
- 已开始同步 SecretStore 读无法强停，沿 Broker 现有边界；只保证取消后无迟到 GET。成本是后台读取仍占资源，不宣称强制中止。
- 本模块不是通用 ProtoJSON codec；未来字段原样保留，替代字段拼写等完整 codec 语义未承诺。成本是非 canonical 兼容服务可能被拒绝，具体整数边界见 Minor1。
- Windows/macOS 不以旧 Anthropic CI 代验；本阶段新 CI 已核对精确源码并三平台通过。若误用旧证据，成本是未发现新增平台问题，后续阶段继续各自验收。

## 原生 generateContent JSON（三平台已验收）

`GeminiClient::generate_content(model, native_json, context)` 使用安全 `models/<id>:generateContent` 路径，保留代理前缀；输入不能包含另一个 body model、改变 endpoint 或认证。非空 contents/parts及已知 role/Part/function call/result 形状校验；原生其余字段原样发送，实际模型对媒体/schema/参数的支持由原生服务判定，不冒称完整请求 schema 校验。[原生请求与 Part reference](https://ai.google.dev/api/generate-content)

POST 与已验 Models GET 共用本模块 JSON/auth/status/media/预算/取消实现，不复制 HTTP 栈。请求 JSON与URL分别受request_bytes限制，回复受response_bytes；Key/header/deadline/slot/TLS/无重试政策沿用。完整 JSON正文才解析为 NativeResponse，不提供提前可执行调用。

NativeResponse 保留所有 candidates/content/Part/thoughtSignature/usage/未来字段和数值精度，Debug省略wire；已知字段坏形状、重复index、缺finishReason/未停止、error envelope、prompt阻断与非空候选冲突报静态502。未知finishReason保留并标Unknown；STOP/客户端ToolCall、MaxTokens、Filtered、InvalidToolCall分别展示，生成结束不代表任务成功。缺省候选index按Proto默认0，缺省/null repeated候选在明确promptFeedback阻断时允许。服务端tool/code/媒体只为数据，不执行、不抓取、不删除。签名仅保留类型/原值，不做真实性校验，不宣称生产历史访问控制完成。

原始usage保留，不相加cached/thought计数、不把未知补0；模型实际modelVersion/responseId缺失保持原义，不由请求名称伪造。后续Responses投影与版本化历史需要单独建立归属、展示一致性/完整回放及Runtime证据，当前仅native内容直接回传和下一请求原文回放。

本地回复3与新增HTTP4（Google合计20）通过：原文/签名/function/tool/未来Part/大数字字面值、终态/阻断/坏结构；真实POST精确body、两轮内容/工具结果原文、输入/route/预算前置零Key/网络、静态错误/429/redirect/不完整body无重试，以及取消POST真实socket关闭/共享Models slot释放。RED→GREEN日志 `/tmp/caidex-google-content-{red,green}.log`、`/tmp/caidex-google-generate-{red,green}.log`；商业调用、SSE、六方法/Gateway/Runtime未验收。本轮完整workspace242passed/0failed、Clippy -D warnings、fmt/diff通过；唯一fresh-context只读审查无Critical/Important，精确源码 `5c41ae2aaea2632b5c98689f7c14b26ba826d0ef` 的 [CI37661724323](https://github.com/bboytang/CAIdex/actions/runs/37661724323) 三平台 completed/success。逐平台核对Google catalog4/回复3/HTTP13共20个测试名各通过一次；workspace Linux242/Windows237/macOS241（0失败），真实Runtime Linux30/WindowsmacOS29，fmt/Clippy/native credentials/schema/doctor通过。恢复时本地Google20项再次通过，日志 `/tmp/caidex-google-json-resume.log`；CI日志 `/tmp/caidex-ci-37661724323-status.json` 与{,-linux,-macos,-windows}.log，跨机器以GitHub为准。/tmp/caidex-google-generate-{workspace,clippy}.log。

本轮JSON审查Minor2暂缓：

- 可选 functionCall.id/functionResponse.id 显式空字符串目前按非法身份拒绝；默认字段序列化兼容端点可能因此拒绝整个有效回复/回放输入。当前 canonical 服务通常省略缺省ID，不冒称支持完整ProtoJSON默认表示。
- distinct多候选正例/第二候选未停止的专门测试缺失；当前代码逐候选校验，已有重复index负例，不冒称覆盖上述两项。

本轮审查排除项裁定：SSE/投影/历史/六方法/Registry/Gateway/Runtime仍需后续实现，误判会漏功能，保持未完成；商业/签名真实性/Full无真实证据，误判会错误授权能力，不授予；生产历史访问控制留H/I，误判会泄漏敏感历史，raw不作为公共日志；完整schema/媒体/参数由原生服务判定，误判成本是原生请求拒绝，不把结构检查当完整能力验收；非通用ProtoJSON（替代字段/数值/枚举）保持canonical范围，成本是兼容端点被拒绝；同步存储读不可强停，成本是后台资源占用，只保证取消后无迟到POST；Windows/macOS已核对本轮精确源码新CI，后续SSE阶段仍须独立新CI，旧JSON/目录CI不能代验，误用成本是漏新增平台问题。以上均沿现有阶段边界，未改架构。


## 原生 SSE 解析（三平台已验收）

`ContentStream` 复用 core `SseDecoder`，显式 frame/stream 字节上限和本次请求的 expected_candidates（HTTP 接线时取 generationConfig.candidateCount，缺省1）。共享既有 NativeResponse 的已知字段/Part校验，增量候选允许尚无 finishReason；JSON完整回复规则保持原样。默认/null index 为0，同帧重复、范围外index、responseId/modelVersion改变、停止后新Part或不同停止原因拒绝。prompt阻断原因独立锁定，后续空/null feedback不解除，其他明确原因或候选拒绝，派生回复仍保留最初阻断原因。

没有伪造 Responses/Interactions 终态。调用方只有在原生HTTP正常EOF后调用 finish：所有请求候选均有明确finishReason（或明确prompt阻断）、无未完成尾帧才产生 NativeStreamResponse；候选已停但流未结束时仍允许末尾usage/metadata。EOF缺候选/未停/残data、取消、坏UTF8/JSON/event、[DONE]、超限或原生error都不能产生完成历史。core只新增has_pending_frame查询，不改变其他Provider的既有分帧行为。[原生API与候选停止契约](https://ai.google.dev/api/generate-content)

完成结果分别提供完整原生chunks与派生NativeResponse：原始chunks保留每次未知字段/空值/数值精度及签名位置；派生候选按index排序，Part按候选内到达顺序原样追加，不合并不同text/thought/signature Part。派生普通metadata采用末次非null值，usage按已提供字段覆盖而非相加；重复/替换的完整metadata仍留raw chunks，不能把派生视图当无损原生历史。此阶段不证明重组后的签名回放真实性，版本化历史/模型归属/Runtime回放仍需后续验证；服务端工具与预测调用只是数据，解析器不执行或抓取。

4项解析回归先RED后GREEN，涵盖所有字节切分/逐字节、BOM/CRLF、原始chunks/签名/未来Part/独立大数字面值、末尾usage、多候选不同停止原因/缺候选或第二候选未停、prompt阻断、身份与生命周期冲突、静态原生错误、残帧/取消及预算。日志 `/tmp/caidex-google-stream-{red,green}.log`；修复后完整workspace246passed/0failed、Clippy -D warnings、fmt/diff通过；独立审查结果及修复见下，源码 `4c9571a4e4cc90fda907d2e6a2c79136166de0e6` 已提交/push，[CI37664189813](https://github.com/bboytang/CAIdex/actions/runs/37664189813)三平台completed/success，精确head已核对。逐平台Google catalog4/回复3/HTTP13/解析4共24个测试名各通过一次；workspace Linux246/Windows241/macOS245（0失败），既有实际Runtime Linux30/WindowsmacOS29，fmt/Clippy/native credentials/schema/doctor通过。日志 `/tmp/caidex-ci-37664189813-status.json` 与{,-linux,-macos,-windows}.log，跨机器以GitHub为准。该解析阶段未包含真实流式HTTP/socket/背压；当前传输阶段的新CI见下，不能用JSON/解析旧CI代验。


本轮唯一独立只读审查：无Critical，1项Important（空feedback覆盖阻断状态后可接受候选）已复现RED，并在同一修复pass独立锁定阻断原因后GREEN。已有两项回归增强覆盖空/null feedback后候选、显式恢复UNSPECIFIED，以及阻断后只追加metadata仍保留原因/完整chunks；未增加其他审查或重做旧阶段。日志 `/tmp/caidex-google-stream-block-{red,green}.log`。

审查Minor1暂缓：重复/部分usage更新的专项覆盖未补（当前只检查末尾usage）；现实现按提供字段覆盖，不相加，原始每个chunk均保留，不冒称此分支已专项验收。

本轮排除项裁定：原生HTTP/cleanEOF检测/背压/socket/slot是下一接线阶段（误用解析证据会漏传输问题）；Responses投影/版本历史/六方法/Gateway/实际Gemini Runtime未接（误判漏功能）；签名真实性/重组Part的提供商回放未验（误判签名请求被拒）；商业/Full无live证据（误判错误能力承诺）；全ProtoJSON/完整媒体函数schema沿已知形状canonical边界（误判兼容端点拒绝）；字节/JSON空白不承诺，只保留native JSON语义（误判失去逐字节一致性）；停止后无新Part的metadata与未知停止原因保留，不授予任务成功（误判未知语义被误当成功）；comment/control-only尾不作为生成数据，残data/event不完成（误判会把截断当完成，当前严格尾查询亦拒绝残未结束comment行）；生产历史访问控制留H/I（误判历史泄漏，raw不作为公共日志）。以上均保留既定阶段边界，后续接线继续逐项验证。


## 原生流式 HTTP（三平台已验收）

`GeminiClient::stream_content(model, native_json, context)` POST `models/<id>:streamGenerateContent?alt=sse`；请求原文/安全路径与JSON生成共用generation_request，Broker/header/status/media与Models/JSON共用execute；认证仍只在sensitive x-goog-api-key。不新增通用传输层，不改变JSON生成接受的原生参数范围。stream独立校验generationConfig对象及canonical正整数candidateCount，缺省/null为1；真实模型支持的候选上限由原生服务判定，不按名称猜测。[原生流式方法](https://ai.google.dev/api/generate-content)

新增NativeStreamingResponse提供原生Event和唯一Completed；复用已验ContentStream。容量1的交付队列、16KiB解析批次、frame/response原字节预算控制缓存，保留所有native chunks和派生视图。候选停止后继续接收usage，只有正常HTTP正文EOF才finish并取走完整记录，不为传输错误/取消/超时调用finish，不用STOP代替EOF。停止但HTTP未结束会idle超时；HTTP截断/残SSE尾/缺候选或未停都不能完成。

worker拥有socket与Models/JSON共用owned permit；读取与send均受绝对caller/total deadline及CancellationToken约束，读取另受idle限制。Drop abort worker；满槽而无人消费时timeout仍关闭socket、释放slot。安全error另存于队列外，已排队Event之后交付一次，不因满槽等待塞入error；debug不打印wire。无redirect/retry/环境proxy/TLS弱化，签名/工具均只为数据。沿既有Broker边界，同步Key读取开始后无法强停，但取消后不POST。

新增HTTP6有效RED→GREEN，Google全30项本地通过：精确原生请求/代理路径/只header认证、候选数及前置零Key/网络、multi-candidate签名Part/大数字字面值/末尾usage、static错误/429/redirect/media/超限/原生坏帧/真实不完整Content-Length、Drop/取消真实socket关闭及共享slot释放、未消费流caller与total deadline满槽错误保存、header/STOP后idle timeout。日志 /tmp/caidex-google-stream-http-{red,green}.log。完整workspace252passed/0failed/32ignored，Clippy -D warnings、fmt/diff通过；独立审查无Critical/Important；源码`27d227aeac01e19216d44a920896ac6618b89e28`的[CI37666947624](https://github.com/bboytang/CAIdex/actions/runs/37666947624)三平台completed/success，精确head及所有步骤已核对。每个平台Google catalog4/content3/HTTP19/解析4共30个测试名各通过一次；workspace Linux252/Windows247/macOS251（0失败，ignored32/30/30），真实Runtime Linux30/WindowsmacOS29，fmt/Clippy/native credentials/schema/doctor通过。日志 /tmp/caidex-ci-37666947624-status.json 与{,-linux,-windows,-macos}.log；跨机器以GitHub为准。 不能用旧解析CI代验。日志 /tmp/caidex-google-stream-http-{workspace,clippy}.log。

阶段边界仍为native HTTP：版本化敏感历史/Responses投影/六方法/Registry/Gateway/实际Gemini Runtime未接，商业签名回放和Full无live证据；生产历史访问控制留H/I，raw不作为公共日志。既有重复/部分usage专项覆盖Minor仍暂缓，不能用本轮单次末尾usage测试宣称已补齐。


本轮唯一fresh-context只读审查无Critical/Important；Minor1暂缓：满槽时取消/Drop的直接专项覆盖未补，当前覆盖读取期取消/Drop和满槽caller/total deadline。生产发送共用cancel/deadline guard，Drop abort同一worker，未发现实现缺陷；覆盖补充后才能宣称此组合已专项验收。

排除项裁定：投影/历史/六方法/Registry/Gateway/真实Gemini Runtime留下一步，误用native证据会漏功能；商业/签名真实性/重组后实际回放/Full无live证据，误判会错误授予能力或提交被拒；生产Host/历史权限留H/I，误判会泄漏敏感数据，raw不作日志；全ProtoJSON/完整schema/媒体/模型参数沿canonical已知形状边界，误判会原生拒绝，不把形状检查当完整能力；旧parser/Minors不重复独立审查、不冒称已修复，成本为既有覆盖边界仍在；同步SecretStore开始后不能强停，只保证取消后无迟到POST，成本为后台读取资源；Windows/macOS必须本轮精确源码新CI，旧parserCI不能代验，误用会漏平台新增问题。均沿既定架构和阶段边界。


## v1原生历史与Responses输出投影（三平台已验收）

NativeHistory沿既有opaque载体模式使用caidex.google.native-history.v1:，provider=google/version=1，保存执行端选定的安全models资源、原始native request、独立CAIdex generation_id、显式candidate_index和完整response；流式另存所有原生JSON chunks。responseId/modelVersion缺失仍为未知，不拿请求路由或本地ID伪造服务端身份。请求校验提取自既有client供HTTP/历史复用，JSON HTTP原有参数接受范围保持；history按stream相同canonical candidateCount（缺省1）核对完整候选及范围。多候选必须选一条，完整替代候选原文仍保存；prompt阻断选择None。

历史通过reasoning.encrypted_content进入经典/Lite canonical wire，该字段在CAIdex边界只是敏感JSON载体，不是加密、密码学认证或OpenAI密文。恢复要求完整连续carrier/display group、同执行端指定model、同原始native request；重放校验思考summary、可见text/role/存在的phase、函数name/call_id/namespace/JSON arguments及顺序/数量。Runtime可省略展示ID/phase或补status，等价JSON空白可变化；原始native值不由这些展示字段重建。provider/version/错模型/错请求/不一致/局部组/超限报静态错误。服务端签名真实性与同时伪造完整载体/投影的认证不在结构校验能力内，保护历史的H/I仍待实现。

流式历史恢复时通过已验ContentStream重建派生response并与载体中response精确比较；坏/缺终态chunk或停止后新Part拒绝，完整raw chunks含未知字段/空值/重复usage保持。capsule序列化总大小有界；重框定增加的SSE分隔字节不冒充原始HTTP预算，网络原预算仍由HTTP worker执行。replay_content只给出所选候选存在且非空的原始Content，Part/thoughtSignature的位置与值不合并、不搬移；保留native角色默认表示，下一请求编译阶段负责其多轮角色归属。当前回放调用者显式提供原始请求校验，尚非自动canonical请求编译器。[Content/Part/FunctionCall契约](https://ai.google.dev/api/generate-content)

输出投影仅产生所选候选的thought文本summary、可见text和STOP且非thought的客户端functionCall；server tool/code/media/未来Part留raw数据，不执行/抓取。native函数ID提供时原样使用，缺省时以本次generation_id/候选/Part位置生成稳定关联ID，重复最终call_id拒绝；缺省args表示为空对象，原始缺省仍留载体。STOP为生成completed；MAX_TOKENS/过滤/阻断/未知reason为incomplete，坏工具调用failed；这些非STOP不交付可执行function_call，文本保持commentary。完整工具声明/namespace/custom/动态发现映射及result配对留请求编译阶段，当前native函数名称投影不是这些能力的完成证据。

usageMetadata.promptTokenCount已含cache，totalTokenCount含prompt+thoughts+candidates；规范化input直接取prompt，output取已知candidates+thoughts或明确total-prompt，total取原值/精确和。不会加cache或toolUsePrompt两次、不补缺省0；不足推导完整三计数时usage=null而raw保持。参与当前归一化计算的计数矛盾或u64相加溢出拒绝，cache/reasoning细节只在明确提供时设置；部分计数的已知下界矛盾覆盖边界见下列Minor。多候选计数为整次generation，不按所选候选猜分摊。[原生UsageMetadata](https://ai.google.dev/api/generate-content#UsageMetadata)

新增history6与真实HTTP1已有效RED→GREEN，Google全37项通过：JSON/SSE完整wire/chunks/签名/未知Part/大数字面值与classic/Lite往返、选择/终态/安全拒绝/展示一致性、重复部分usage按字段覆盖、独立模型/request归属、native可选ID；实际JSON/SSE→canonical序列化→restore→下一原生POST精确保留Content/签名/functionResponse。reply/Key全部合成、没有执行工具或商业API；此HTTP正例使用客户端明确组装下一native body，不冒称六方法/Gateway/实际Gemini Runtime已接。日志 /tmp/caidex-google-history-{red,green,http-red,http-green}.log；完整workspace259passed/0failed/32ignored、Clippy -D warnings、fmt/diff通过；独立审查无Critical/Important，2项代码Minor见下。源码`421ead55b8c516021c81be277808bda37a2f886a`已提交/push，[CI37672020384](https://github.com/bboytang/CAIdex/actions/runs/37672020384)三平台completed/success，精确head及全部步骤已核对。每个平台catalog4/content3/history6/HTTP20/stream4共37个Google测试名各通过一次；workspace Linux259/Windows254/macOS258，0失败，ignored32/30/30；真实Runtime Linux30/WindowsmacOS29，0失败/0ignored。fmt/Clippy/native credentials/schema/doctor通过；日志 /tmp/caidex-ci-37672020384-status.json 与{,-linux,-windows,-macos}.log，跨机器以GitHub为准。日志 /tmp/caidex-google-history-{workspace,clippy}.log。

下一步：接Responses请求编译和原生工具映射（经典/Lite、namespace/custom/结果配对、图片、推理/结构输出/Runtime参数），再六方法/Registry/Gateway和固定Runtime。商业/签名实际验证/Full、生产访问控制仍保留边界，不混用Interactions的signature/事件字段。旧SSE重复/部分usage覆盖Minor由本轮history回归补齐（候选计数1→7、thought及末尾total补齐与raw值全保留）；旧满槽取消/Drop直接专项覆盖等其余Minor保持原边界。


本轮唯一fresh-context只读审查无Critical/Important，2项代码Minor暂缓，1项交接旧checkpoint文本在常规阶段更新中删除：

- 缺promptTokenCount的部分usage：total=1/candidates=7/thoughts=3或total=1/cache=2目前接受，规范化usage=null且完整原值保留，未造虚假计数；没有检查已知下界与total的矛盾。审查临时独立程序已复现（/tmp/caidex-google-history-review.rs）；以后补RED→GREEN下界校验才能宣称全量矛盾拒绝，不把现有覆盖夸大。
- STOP里只有thought=true的functionCall加最终普通text时，不投影该调用，但native outcome仍为ToolCall，text phase为commentary。复现同上；目前为边缘展示一致性问题，无商业该组合证据；以后基于实际非thought客户端调用判phase并补回归，当前不标已修复。

本轮排除项裁定：自动请求编译/工具映射/六方法/Gateway/真实Runtime是下一里程碑，误用本轮输出证据会漏功能；缺省role的model归属及失败/过滤/MAX_TOKENS中未执行call的配对/拒绝策略须在编译器落实，误用replay_content会提交不合适历史，本轮它只是原始Content借用视图；取消/HTTP截断流不能构造NativeStreamResponse，当前不承诺中断历史恢复，误判会丢失中断恢复上下文，留Host/Chat历史阶段单独记录而不伪装生成完成；展示ID/status可变但原生内容及语义字段仍真源，误判会用展示状态篡改回放；同时伪造载体/投影/source及签名真实性不是结构校验认证，误判会认领未认证数据，生产H/I访问控制与live验证仍待验；完整ProtoJSON/schema与旧parser/HTTP Minor保持既定canonical范围，误判会兼容请求拒绝，不重做已验阶段；商业Full无live证据，误判会错误能力承诺，不授予；本轮Windows/macOS需精确源码新CI，误用旧证据会漏平台问题。本轮只读取完成的native响应，JSON/SSE原始生成终态与Responses状态保持各自语义，不改变既定架构。


## 工具身份映射与v2原生历史（三平台已验收）

这是Responses请求编译器的前置依赖，沿既有core ToolCall/ToolKind/ToolInput边界和Anthropic工具映射模式实现，不更改V3架构。ToolMap把function/custom和一层namespace声明编译为原生FunctionDeclaration；alias由namespace/name/kind的SHA256生成，重排或修订schema不换身份，同名不同namespace不冲突。原始声明含未来字段完整保存，namespace说明与工具description进入原生说明；函数schema用parametersJsonSchema原样保留，不降为Google Schema子集，不冒称服务端接受任意JSON Schema。[原生FunctionDeclaration契约](https://ai.google.dev/api/generate-content.md#FunctionDeclaration)

custom工具用单一input字符串对象承载，空白/转义/Unicode保持；原grammar/text format完整保留并写入指导，grammar只是提示，不是硬约束。strict=true、defer_loading=true/client tool_search、web_search分别明确unsupported，不默默去掉约束、提前加载延迟工具或把OpenAI缓存网页等同Google grounding；未来获得等价协议与模型证据才扩展。当前只有编解码器，不授予这些能力/商业Full。

native_call把已有canonical call编成FunctionCall数据，responses_call只从固定声明认领原生alias；native id存在时保持，缺省由调用方提供稳定generation/候选/Part关联ID。native args缺省为空对象，函数参数为JSON对象且任意精度保留；custom只接受一个input字符串，不让未知name/namespace/kind选择Runtime工具。服务端预测调用仍只留raw，不执行、不抓取。原生FunctionResponse配对/图片等结果转换由下一请求编译器落实，本轮HTTP正例明确手工组装结果。

NativeHistory.with_tools为显式v2载体caidex.google.native-history.v2:，保存本次原始canonical tools，并用重建ToolMap逐值核对实际native request.tools；不是拿下一请求声明重新解释历史。完整JSON/SSE raw response/request/chunks及签名位置仍为真源；所选客户端调用按原映射投影function/custom/namespace，恢复验证call_id/name/namespace/arguments或custom原文及连续完整组。prefix与version须一致，改变schema或工具身份无法借旧request恢复。v1构造/投影不自动升级；携带tools的伪v1拒绝。载体仍是敏感JSON，无加密/签名认证承诺。

6项工具/历史测试初始实际501 RED→GREEN；先工具4通过/历史2以缺映射501失败，再v2历史通过。namespace说明丢失也独立断言RED→GREEN。新HTTP1真实JSON/SSE POST→classic/Lite canonical序列化→恢复→下一POST，原生Content/签名/自定义原文完整；缺省原生call/result ID仍省略，未插入Runtime本地ID。Google共44项本地通过，全部为合成回复/Key；新增HTTP是扩大已测试编解码行为的集成验收，未声称HTTP测试有单独实现前RED。日志 /tmp/caidex-google-tools-{red,map-green-history-red,green,namespace-red,http-green}.log；完整workspace266passed/0failed/32ignored、Clippy -D warnings、fmt/diff通过；唯一独立审查无Critical/Important，1项覆盖Minor暂缓：未对完整有效v1/v2组做prefix-only交叉替换，现有prefix/version校验正确但删除该校验可能逃过测试；测试注释中夸大prefix覆盖的文字已纠正。源码`d2a363d64c80b63049b7bffa0325224955657983`已提交/push，[CI37675979615](https://github.com/bboytang/CAIdex/actions/runs/37675979615)三平台completed/success，精确head及全部步骤已核对。逐平台Google catalog4/content3/history6/HTTP21/stream4/tools6共44个测试名各通过一次；workspace Linux266/Windows261/macOS265，0失败，ignored32/30/30；真实Runtime Linux30/WindowsmacOS29，0失败/0ignored；fmt/Clippy/native credentials/schema/doctor通过。日志 /tmp/caidex-ci-37675979615-status.json 与{,-linux,-windows,-macos}.log，跨机器以GitHub为准。日志 /tmp/caidex-google-tools-{workspace,clippy}.log。

下一步仍是Responses请求编译器：system/messages/前缀归属、原始完整历史组恢复、function/custom结果配对、图片、推理/结构输出/Runtime参数与能力门控；随后六方法/Registry/Gateway/固定Runtime。不能把本轮工具codec当作完整Provider或Runtime接线证明。现有partial usage/text phase、满槽取消Drop等Minor保持原边界。

本轮审查排除项裁定：自动请求编译/结果关联/图片/Runtime仍是下一里程碑，误用手工HTTP证据会漏功能；native_call只编译新的独立canonical调用，签名回放必须用原始Content，误用会丢签名或写入本地ID；失败/MAX_TOKENS的自动回放策略由下一编译器落实，误用会发送未执行call，当前投影不交付可执行调用；商业schema接受/grammar硬约束/签名真实性无合成fixture之外证据，误判会提交被拒或授予错误能力，不宣称支持；整组伪造及生产授权留H/I，误判会认领未认证敏感历史；旧parser/HTTP/usage/text phase Minor未改，误判会夸大覆盖；Windows/macOS须本轮精确源码新CI，旧history CI不能代验，误用会漏平台新增问题。以上保持原架构和scope；Minor不进入修复轮次，未标已修复。

## Responses请求编译基础（本地验证，本轮CI待做）

GenerateContentRequest复用CanonicalRequest/ToolMap/NativeHistory及原生HTTP。经典instructions和初始system/developer文本成为systemInstruction；Lite仅首项developer additional_tools提供声明。普通user/assistant文本成为user/model Content，后置system/developer明确unsupported，避免重排签名前缀。source完整保留，Debug不输出wire；路由模型、maxOutputTokens、字节/工具预算由执行端给定，不从请求另选endpoint。

恢复时在当前位置编译实际前缀，再核对载体model和完整原native request，并校验完整carrier/display组；不把载体请求当它自己的期望值。签名Part/未知Content原样回放；role缺失/null/空只在新请求Content补model。STOP里的function/custom按固定ToolMap还原身份并登记，nonSTOP或thought functionCall明确拒绝自动回放。结果按call_id或唯一legacy kind/name/namespace配对，拒绝孤儿/重复/歧义/身份错误/未完成结果；原生缺省ID保持省略，本地关联ID不插入签名Part或functionResponse。结果字符串（包括JSON文本）和已支持文本数组保留原值/顺序，空数组合法，包装在原生对象response.output中。

Ruling: 维持现有v1/v2完整native request绑定，暂含generationConfig/toolConfig，不自动放宽成仅system/tools/contents；因此改变maxOutputTokens或tool_choice会拒绝旧历史，后续有明确原生兼容依据再版本化调整，误判成本是拒绝原本可安全更改的参数。

Ruling: 本阶段先交付编译/回放/结果基础，图片和高级参数保持明确unsupported并沿既定下一步接入；当前仅auto/required/none映射AUTO/ANY/NONE，include仅本地reasoning.encrypted_content。parallel_tool_calls=false且工具启用/choice非none时明确unsupported，FunctionCallingConfig没有等价单调用限制；固定Lite默认false仍是Runtime接线前的兼容边界，不静默丢弃、不用提示词冒充硬约束。[原生FunctionCallingConfig](https://ai.google.dev/api/caching#FunctionCallingConfig)；错误成本是请求拒绝/Runtime不能接通，需要后续等价契约或继续明确不支持。

本地编译器8项有效RED→GREEN（含空结果和畸形message.type两次独立RED），HTTP1在真实JSON/SSE/三POST上扩大集成验证（无独立实现前RED）；Google53/workspace274/0failed/32ignored、Clippy/fmt/diff通过。新源码唯一审查的1项Important已RED→GREEN修复，无Critical/新增Minor；提交/push及本轮精确三平台CI待做。日志 /tmp/caidex-google-request-{red,empty-red,type-red,green,http,workspace,clippy}.log。没有真实Gemini调用或六方法/Gateway/Runtime证据，不授予Full；旧usage/text phase等Minor未改。

唯一独立审查发现1项Important：相同alias的并行native call都缺ID时，canonical逆序结果会在native wire丢失关联。新idless_parallel_results_keep_original_native_call_order用例先RED（SECOND先于FIRST），再以完整结果组按原native调用顺序输出修复；包括混合ID、签名原值、不补ID及部分结果不准接新调用。原source结果到达顺序、单个结果内数组顺序不变。完整workspace/Clippy/fmt/diff复验通过；不派第二次审查。日志 /tmp/caidex-google-request-order-{red,green}.log。

Ruling: 结果组统一等全部配对后按原native call顺序发出，含本可用ID逆序的组，减少不必要的两套顺序策略；这是请求编译，不执行或等待工具运行，source仍保存到达顺序。[原生并行调用关联](https://ai.google.dev/gemini-api/docs/generate-content/function-calling?hl=en#parallel_function_calling)。若误判，成本是未支持非阻塞/分批native结果时序；本阶段没有承诺NON_BLOCKING/willContinue。

本轮审查排除项裁定：商业服务接受/签名真实性/schema/grammar硬约束无live证据，不授予Full（误判成本为错误能力承诺或提交被拒）；六方法/Registry/Gateway/固定Gemini Runtime仍下一阶段，Lite false并行约束门控未解决（误判成本为漏接功能）；媒体/推理/结构输出/高级参数及放宽完整request历史绑定沿已记录后续范围（误判成本为请求拒绝或错用签名）；Windows/macOS须本轮精确源码CI，旧证据不能代验（误判成本为漏平台问题，待本轮实测）；旧usage/text phase/HTTP/codec Minor未改，生产历史授权留H/I（误判成本为夸大覆盖/未授权历史访问）。
