# CAIdex Gemini Provider：设计与验收

阶段F/G，执行基准V3，代码在model/providers/google。原生Models/JSON/SSE/历史/工具/图片/推理/结构输出/Runtime参数与六方法/Profile、Registry/Gateway已三平台验收；上阶段源码530ead5535b6bc6cd8896317b7360c7872a98571/[CI37696809902](https://github.com/bboytang/CAIdex/actions/runs/37696809902)，Google88逐平台通过。当前实际Gemini Runtime离线接线已在源码56f9789/CI37717424972三平台验收，唯一审查已完成；下一步兼容API/Ollama，见末节与HANDOFF。本阶段不授予Gemini Codex Full。

## 原生协议与配置

- `GeminiConfig` 固定默认 `https://generativelanguage.googleapis.com/v1beta/`；高级地址沿用现有 endpoint 验证：HTTPS 或 literal loopback HTTP，拒绝 userinfo/query/fragment，保留代理路径前缀。
- 执行端 `CredentialRef` 必须为 `google`/`ApiKey`，经 owner 固定的 Broker 解析。认证只用 sensitive `x-goog-api-key`，不放 URL，不探测用户环境，不向手机返回保存的 Host Key。[官方认证](https://ai.google.dev/gemini-api/docs/api-key)
- `GeminiClient::discover_models(max_models, RequestContext)` 请求原生 `models`，稳定 `pageSize=1000`，仅将原样 `nextPageToken` 编码为后续 `pageToken`。页 token 不选择 endpoint；失败不返回部分列表。[Models reference](https://ai.google.dev/api/models)
- 默认拒绝所有非空 Runtime context headers，Key前报unsupported_native_context_header；本轮显式本地归属opt-in只允许3头且不外发，详后节。
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

## 目录基础阶段的下一步（历史记录，当前恢复点见末节与HANDOFF）

1. 本阶段独立审查及精确源码三平台验收已完成；沿以下顺序继续，不重复目录基础。
2. generateContent JSON、原生SSE解析和streamGenerateContent?alt=sse真实HTTP/单槽背压/取消、Drop、deadline与socket/slot生命周期已三平台验收，继续第3步；保留完整 Part/thoughtSignature/functionCall/functionResponse wire 和 usage，不混用 Interactions 的 signature/事件结构。[GenerateContent reference](https://ai.google.dev/api/generate-content)
3. 版本化原生历史与Responses输出投影已三平台验收；请求/工具/图片编译范围已验，继续其余Runtime参数转换，再接现有六方法、Registry、Gateway 与固定真实 Runtime；经典/Lite、审批/工具/取消/恢复分别验收。不执行工具，不创建第二套 Agent，不隐式降级或标商业 Full。

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

## Responses请求编译基础（三平台已验收）

GenerateContentRequest复用CanonicalRequest/ToolMap/NativeHistory及原生HTTP。经典instructions和初始system/developer文本成为systemInstruction；Lite仅首项developer additional_tools提供声明。普通user/assistant文本成为user/model Content，后置system/developer明确unsupported，避免重排签名前缀。source完整保留，Debug不输出wire；路由模型、maxOutputTokens、字节/工具预算由执行端给定，不从请求另选endpoint。

恢复时在当前位置编译实际前缀，再核对载体model和完整原native request，并校验完整carrier/display组；不把载体请求当它自己的期望值。签名Part/未知Content原样回放；role缺失/null/空只在新请求Content补model。STOP里的function/custom按固定ToolMap还原身份并登记，nonSTOP或thought functionCall明确拒绝自动回放。结果按call_id或唯一legacy kind/name/namespace配对，拒绝孤儿/重复/歧义/身份错误/未完成结果；原生缺省ID保持省略，本地关联ID不插入签名Part或functionResponse。结果字符串（包括JSON文本）和已支持文本数组保留原值/顺序，空数组合法，包装在原生对象response.output中。

Ruling: 维持现有v1/v2完整native request绑定，暂含generationConfig/toolConfig，不自动放宽成仅system/tools/contents；因此改变maxOutputTokens或tool_choice会拒绝旧历史，后续有明确原生兼容依据再版本化调整，误判成本是拒绝原本可安全更改的参数。

Ruling: 本阶段先交付编译/回放/结果基础，图片和高级参数保持明确unsupported并沿既定下一步接入；当前仅auto/required/none映射AUTO/ANY/NONE，include仅本地reasoning.encrypted_content。parallel_tool_calls=false且工具启用/choice非none时明确unsupported，FunctionCallingConfig没有等价单调用限制；固定Lite默认false仍是Runtime接线前的兼容边界，不静默丢弃、不用提示词冒充硬约束。[原生FunctionCallingConfig](https://ai.google.dev/api/caching#FunctionCallingConfig)；错误成本是请求拒绝/Runtime不能接通，需要后续等价契约或继续明确不支持。

本地编译器8项有效RED→GREEN（含空结果和畸形message.type两次独立RED），HTTP1在真实JSON/SSE/三POST上扩大集成验证（无独立实现前RED）；Google53/workspace274/0failed/32ignored、Clippy/fmt/diff通过。新源码唯一审查的1项Important已RED→GREEN修复，无Critical/新增Minor；源码08402e6已提交/push且本轮精确三平台CI37679653990通过。日志 /tmp/caidex-google-request-{red,empty-red,type-red,green,http,workspace,clippy}.log。没有真实Gemini调用或六方法/Gateway/Runtime证据，不授予Full；旧usage/text phase等Minor未改。

唯一独立审查发现1项Important：相同alias的并行native call都缺ID时，canonical逆序结果会在native wire丢失关联。新idless_parallel_results_keep_original_native_call_order用例先RED（SECOND先于FIRST），再以完整结果组按原native调用顺序输出修复；包括混合ID、签名原值、不补ID及部分结果不准接新调用。原source结果到达顺序、单个结果内数组顺序不变。完整workspace/Clippy/fmt/diff复验通过；不派第二次审查。日志 /tmp/caidex-google-request-order-{red,green}.log。

Ruling: 结果组统一等全部配对后按原native call顺序发出，含本可用ID逆序的组，减少不必要的两套顺序策略；这是请求编译，不执行或等待工具运行，source仍保存到达顺序。[原生并行调用关联](https://ai.google.dev/gemini-api/docs/generate-content/function-calling?hl=en#parallel_function_calling)。若误判，成本是未支持非阻塞/分批native结果时序；本阶段没有承诺NON_BLOCKING/willContinue。

本轮审查排除项裁定：商业服务接受/签名真实性/schema/grammar硬约束无live证据，不授予Full（误判成本为错误能力承诺或提交被拒）；六方法/Registry/Gateway/固定Gemini Runtime仍下一阶段，Lite false并行约束门控未解决（误判成本为漏接功能）；媒体/推理/结构输出/高级参数及放宽完整request历史绑定沿已记录后续范围（误判成本为请求拒绝或错用签名）；Windows/macOS使用本轮精确源码CI，旧证据不能代验（误判成本为漏平台问题；本轮已实测通过）；旧usage/text phase/HTTP/codec Minor未改，生产历史授权留H/I（误判成本为夸大覆盖/未授权历史访问）。

源码08402e6caccba2b0ff7cc98dc09f60ea4ea16078的[CI37679653990](https://github.com/bboytang/CAIdex/actions/runs/37679653990)三平台completed/success，全部步骤核对；catalog4/content3/history6/HTTP22/request8/stream4/tools6共Google53个名字各平台各通过一次。workspace日志汇总Linux275/Windows270/macOS274，0失败，ignored32/30/30；真实Runtime Linux30/WindowsmacOS29，0失败/0ignored；fmt/Clippy/native credentials/schema/doctor通过。日志 /tmp/caidex-ci-37679653990-status.json与{-watch,-linux,-windows,-macos}.log；本地凭据子进程stdout交错使汇总少计1条，不能把摘要差异当遗漏用例，CI该用例已明确通过。

恢复点：继续图片/工具结果媒体（inlineData、FunctionResponse.parts及引用/能力边界），再推理/结构输出/Runtime参数，随后六方法/Registry/Gateway/固定Runtime。原生单调用约束、工具发现/网页、后置system等unsupported边界仍明确，不以编译器离线证据标商业Full或完整F/G完成。

## 图片与工具结果媒体（三平台已验收）

沿GenerateContentRequest增加显式执行端RequestOptions，默认空能力仍拒绝图片；不从模型名或目录推断支持。用户data URL仅接受配置允许的PNG/JPEG/WEBP/HEIC/HEIF，生成inlineData并保持原base64、文本/图片顺序；工具结果独立能力只接受PNG/JPEG/WEBP，媒体放FunctionResponse.parts.inlineData，response.output按原数组位置使用单键$ref对象，displayName由全局调用序和块序生成。function/custom、并行逆序到达及缺省native ID仍沿原配对/输出策略，source保存完整canonical原文，签名Part不改。[用户图片格式](https://ai.google.dev/gemini-api/docs/generate-content/image-understanding#supported_image_formats)、[原生多模态工具结果](https://ai.google.dev/gemini-api/docs/generate-content/function-calling?hl=en#multimodal_function_responses)。

ImageDetailMapping由执行端明确把low/high/original意图映射为native per-Part mediaResolution.level；映射存在才声明相应能力，不承诺跨Provider画质/token等价。auto/缺省/null不设置分辨率；未知detail拒绝，工具结果非auto拒绝，FunctionResponsePart没有该分辨率字段。不改全局generationConfig，也不把Google实验/部分模型能力猜成全模型支持。[分辨率契约](https://ai.google.dev/gemini-api/docs/generate-content/media-resolution)。

Ruling: 只支持有界base64 data URL，不抓取远程URL/读取本地路径/认领file_id；检查编码及非空字节，不解码像素或检测文件魔数，内容与模型视觉能力仍由服务验证。成本是URL、file_id或未列格式需以后独立附件通道，不新增隐式IO。原请求和展开后的native body均受字节预算；仅复用已锁定base64 0.22.1，无新第三方版本。

Ruling: 含媒体的工具结果中，结构化单键$ref对象属于原生媒体引用语义，必须只引用本结果生成的displayName且每个名字仅出现一次；未知/重复引用在发送前拒绝。其他metadata保留，字符串内的JSON字面文本不解释；纯文本结果原有$ref数据行为不改。成本是与保留语法冲突的结构化元数据需编码为文本或以后建立等价协议，不能默默重定向图片。固定上游工具结果content schema无input_file，PDF/音频/其他附件本轮保持unsupported，不借原生文档支持虚构canonical类型。

6项媒体回归初始5项RED→GREEN，引用冲突第6项独立RED→GREEN；既有编译器8项通过。新增实际HTTP1验证经典/Lite三POST（JSON→SSE→持久化往返）图片/签名/媒体引用与结果关联，编辑旧图片和外部URL在新增Key读取之前拒绝；这是扩大已TDD编解码行为的集成覆盖，无独立HTTP实现前RED。全部回复/Key/编码字节合成，不证明真实图片识别、签名真实性、商业接受或Full。日志/tmp/caidex-google-media-{red,green,ref-red,ref-green,http}.log；完整workspace282passed/0failed/32ignored、Clippy -D warnings/fmt/diff通过；唯一fresh-context只读审查无Critical/Important/新增Minor；独立复跑媒体6+编译器8通过。源码cf8c2c942c593858b3a2b775b3ea94c5510a9877的[CI37684084386](https://github.com/bboytang/CAIdex/actions/runs/37684084386)三平台completed/success，精确head及全部步骤已核对。Google catalog4/content3/history6/HTTP23/request8/media6/stream4/tools6共60个名字逐平台各通过一次；workspace Linux282/Windows277/macOS281，0失败，ignored32/30/30；真实Runtime Linux30/WindowsmacOS29，0失败/0ignored；fmt/Clippy/native credentials/schema/doctor通过。日志/tmp/caidex-ci-37684084386-status.json及{-watch,-linux,-windows,-macos}.log，跨机器以GitHub为准。后续仍按HANDOFF继续推理/结构输出/Runtime参数，再六方法/Registry/Gateway/固定Runtime，不把本轮canonical Lite fixture当实际Runtime工具执行。

本轮审查排除项裁定：实际图片识别/商业接受/签名真实性未调用live，不授予Full，误判成本是错误能力承诺；displayName采用官方多模态指南及REST示例，通用API生成参考暂缺该字段，真实端点接受仍待live证据，误判成本是服务拒绝。编码校验而非像素/MIME魔数、URL/file_id/本地读取/PDF/音频、分辨率等价及工具detail、保留$ref语法均依上述Ruling，误判成本为不支持的附件或元数据被拒，不能冒称无损任意内容。六方法/Registry/Gateway/实际Gemini Runtime及parallelfalse、推理/结构输出/参数/后置system/放宽历史绑定仍下一阶段，误判成本是漏功能或错用历史；非阻塞/部分native结果仍只接受完整组，误判成本是请求拒绝/时序不兼容。旧parser/history/HTTP Minor及生产历史权限/整组伪造认证未改，误判成本是夸大覆盖或认领未认证历史；Windows/macOS须本次精确源码CI，误用旧CI会漏新增平台问题。唯一审查报告/tmp/caidex-google-media-review.md，不派第二次审查。

## 推理设置与执行端契约（三平台已验收）

沿RequestOptions增加ReasoningMapping/SummaryMapping及ThinkingContext，默认无映射时effort/summary/context仍明确unsupported。effort对应一个执行端显式native thinkingConfig：thinkingBudget或thinkingLevel二选一，可另带布尔includeThoughts。预算为原生int32中的-1（动态）、0（关闭）或正整数，level采用API参考的MINIMAL/LOW/MEDIUM/HIGH；具体模型的预算范围、可用level与关闭能力由执行端profile负责，不从model名称/目录猜测。不把native参数偷偷当canonical调用方字段。[ThinkingConfig](https://ai.google.dev/api/generate-content.md#ThinkingConfig)、[原生思考控制](https://ai.google.dev/gemini-api/docs/generate-content/thinking#controlling_thinking)。

Ruling: none明确要求关闭，必须显式映射thinkingBudget=0，不能用MINIMAL或动态模式冒充；其他effort保留固定上游非空字符串的开放命名，由profile声明意图转换，不承诺跨Provider档位等价。budget与maxOutputTokens分别保留，不复制Anthropic budget小于max_tokens规则；Google输出上限包含思考token，native预算指导可能上/下溢，MAX_TOKENS仍由原终态路径标不完整。成本是错误或未验证的模型profile可能被服务拒绝/截断，需要LiveRuntime报告而不是合成Full。[原生预算/上限](https://ai.google.dev/gemini-api/docs/generate-content/thinking#thinking_budgets)。

Ruling: summary auto/concise/detailed只在显式映射下设置includeThoughts=true，none映射false；不声称Google能控制这些档位的相同长度。summary-only只改显示开关，不设置预算/level或激活模型默认思考；明确预算0时要求非none摘要会拒绝。该开关只返回可用摘要，不保证模型一定产生摘要；原生thought summary/signature仍沿完整history保存，不为显示设置删除签名。成本是模型默认关闭或缺摘要时无摘要，不能用伪摘要补齐。[Thought summaries](https://ai.google.dev/gemini-api/docs/generate-content/thinking#thought_summaries)。

Ruling: context current_turn/all_turns是执行端声明的原生保留策略，只有与请求精确匹配才接受；本地保留source，不发虚构Google context字段，不因该声明裁掉旧Part。Google无公开同名context设置，模型实际利用哪些历史仍需独立live证据；声明不授予AllTurns/Full。误判成本是模型没有请求期望的推理承接能力，不能以JSON结构验证代替原生真实性。[完整签名回传](https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures)。

推理参数在处理任何载体前进入actual compiled native prefix；现有v1/v2完整request绑定继续生效，改预算/level或摘要开关会在联网/读取Key前拒绝旧历史，不自动松绑、换模型或删签名。重复配置、未知约束、坏类型/形状/预算/level及禁用摘要错误映射均安全拒绝；空/null reasoning和已知可选null视为未指定，完整canonical source保留。

5项reasoning测试有效501/缺转换RED→GREEN，原request8/media6通过；扩大既有HTTP1至经典/Lite两种thinking模式各三POST，JSON及SSE摘要、图片、tool media、原生签名、结果关联和落盘往返保持，改native映射在新增Key读取前拒绝。HTTP是扩大既有TDD编译器行为的集成范围，没有独立实现前RED；全部回复/Key/编码字节合成，不验证商业模型推理。完整workspace287passed/0failed/32ignored、Google65个名字各通过一次，Clippy -D warnings/fmt/diff通过；唯一独立审查及当前精确源码三平台CI已完成（见本节最终证据）；日志/tmp/caidex-google-reasoning-{red,green,http,workspace}.log。下一步结构化输出/其余Runtime参数，再六方法/Registry/Gateway/固定Runtime，不重做已验媒体。

本轮唯一fresh-context只读审查无Critical/Important/新增Minor，独立reasoning5/request8通过；原Minor未变。排除项逐项裁定：商业接受/质量/签名真实性及Full无live证据，模型profile的范围/档位/关闭能力由执行端显式承担（误判成本是服务拒绝、截断或错误能力承诺）；summary长度/可用性、实际context承接不能由显示开关/本地声明证明（成本是意图不等价）。预算与输出上限独立、开放effort及canonical原生JSON形状按上述契约，替代ProtoJSON拼写未承诺（成本是非canonical配置被拒）；预算0且native includeThoughts=true在没有显式非none summary时只请求可用显示，不启用思考。完整原request绑定不放松（成本是改变有效配置后旧历史拒绝）。六方法/Registry/Gateway/实际Gemini Runtime及Lite parallelfalse、结构输出/高级参数/发现/网页/后置system/非阻塞部分结果仍后续范围（成本是功能缺失或请求拒绝）；媒体服务接受/displayName/像素识别等旧契约没有新增live证据（成本是服务拒绝）；旧parser/history/HTTP/codec Minor、生产Host权限/整组载体伪造认证未修复（成本是夸大覆盖或认领未认证历史）。Windows/macOS/Linux须本轮精确源码CI，旧CI不能代验（成本是漏平台问题）。报告/tmp/caidex-google-reasoning-review.md，不派第二次审查。

推理阶段留下的检查点（结构输出已按下节落实）：[现行结构输出指南](https://ai.google.dev/gemini-api/docs/generate-content/structured-output#limitations)明确模型会忽略不支持的JSON Schema属性；REST示例使用generationConfig.responseFormat.text.schema。接线前核对API参考的旧responseJsonSchema与新responseFormat支持范围，建立显式能力/子集拒绝边界，不能套用Anthropic原样传schema后等待服务拒绝的假设。

推理源码f3284fbef7c046b733dbcdeab20c5bc7741e4f0d的[CI37687665151](https://github.com/bboytang/CAIdex/actions/runs/37687665151)三平台completed/success，精确head及全部步骤核对。Google catalog4/content3/history6/HTTP23/request8/media6/reasoning5/stream4/tools6共65个名字逐平台各通过一次；workspace Linux287/Windows282/macOS286，0失败，ignored32/30/30；真实Runtime Linux30/WindowsmacOS29，0失败/0ignored；fmt/Clippy/native credentials/schema/doctor通过。日志/tmp/caidex-ci-37687665151-status.json与{-watch,-linux,-windows,-macos}.log。本轮未新增实际Gemini Runtime接线或商业验证，macOS Rust检查不是iOS应用构建；下一步沿HANDOFF继续结构输出及其余Runtime参数。

## 结构输出转换（三平台已验收）

现有RequestOptions新增supports_structured_outputs及独立supports_structured_outputs_with_tools声明；默认仍拒绝，不能从model名称/目录猜测。text.format=json_schema要求strict:true，name验证后只作canonical标签，wrapper description/non-strict/未知字段明确拒绝；schema内title/description原样保留。json_object使用native对象Schema保证对象意图，text/缺省/null不设额外控制；verbosity仍属后续参数转换。现行generationConfig.responseFormat.text使用API参考canonical MIME枚举APPLICATION_JSON及原schema，不发送已弃用responseMimeType/responseSchema/_responseJsonSchema，不借typed OpenAPI Schema重写JSON Schema。[现行格式](https://ai.google.dev/api/generate-content.md#TextResponseFormat)。

Schema检查是native支持子集门控，不是结果求值器：type含nullable数组、title/description、日期format、字符串/数字enum、minimum/maximum、array items/prefixItems/minItems/maxItems、object properties/required/additionalProperties、anyOf、$defs及无环本地JSON Pointer $ref保持原文与数值精度。属性名不被当关键词；未知/被忽略的关键字（pattern/minLength等）、oneOf（native按anyOf解释）、布尔schema（additionalProperties除外）及未知format拒绝，不静默删除约束。只解析本地JSON Pointer，无外部IO；$id/$anchor/anchor引用及循环引用未建立严格等价，明确unsupported，不按服务有限展开假装完整递归语义。[JSON Schema原生支持](https://ai.google.dev/api/generate-content.md#GenerationConfig)、[忽略属性的限制](https://ai.google.dev/gemini-api/docs/generate-content/structured-output#limitations)。

Ruling: 递归检查active路径最多64级并按节点去重，避免引用DAG指数展开；不展开/改写schema，不新增validator依赖。超限/未支持schema须明确拒绝，未来有实际需求与原生等价证据再扩展，不降低strict意图。原生服务仍验证自身Schema复杂度/模型接受；此编译器不认证生成值/业务正确性，不能把合成JSON回复标商业结构输出或Full。Schema与任何thinking设置一起在history处理前进入完整native request绑定，改Schema/移除格式拒绝旧v1/v2历史，不删签名或放松原绑定。

6项有效RED→GREEN，既有request8/reasoning5通过；扩大既有HTTP媒体/推理用例，budget/level及经典/Lite各三POST加入输出Schema与原文JSON回复，保留摘要/签名/媒体/工具关联/落盘往返；改Schema及不支持pattern在新增Key读取之前编译拒绝。HTTP为既有编译器行为集成扩展，无独立实现前RED。日志/tmp/caidex-google-structured-{red,green,http}.log；审查前完整workspace293passed/0failed/32ignored、Google71个名字各通过一次、Clippy -D warnings/fmt/diff已通过；唯一审查及精确源码三平台CI已完成（见本节最终证据）。

唯一fresh-context审查发现1项Important（无Critical/新增Minor）：本地$ref直接用JSON Pointer查找而未先解码URI fragment，%61/a同名定义能令循环引用漏检；非法百分号和~转义也是同一根因。已按RFC6901在共享路径先解码一次UTF-8片段、校验%HH与~0/~1语法，再由Value::pointer查找；原始$ref/source/native schema保持不变，无新依赖/IO。新增第7项回归对编码循环、坏编码/UTF-8/tilde、合法空格/Unicode/编码分隔符/一次解码语义完成有效RED→GREEN，结构输出7/request8/reasoning5通过。此为审查后的唯一修复，修复后最终完整workspace294passed/0failed/32ignored、Google72项逐名通过、Clippy -D warnings/fmt/diff通过；新CI已完成（见下），不派第二次审查。[RFC6901](https://www.rfc-editor.org/info/rfc6901/)。报告/tmp/caidex-google-structured-review.md，独立复现/tmp/caidex-google-ref-review-repro.log；修复日志/tmp/caidex-google-structured-ref-{red,green}.log。

审查18项排除裁定（不是隐含支持）：1/3/5商业wire/模型profile/组合能力及Full无live证据，显式开关不授予LiveRuntime（成本是服务拒绝或错误能力承诺）；2结果schema/业务值不由编译器求值，应用验证后续落实（成本是不合预期输出不能被认领为已校验）；4签名真实性/整组载体认证与16生产Host权限/原生凭据/客户端iOS/真机签名保持后续边界（成本是认领未认证历史或未验证客户端）。6六方法/Registry/Gateway/实际Gemini Runtime、7Lite parallelfalse/发现/网页/strict工具/grammar/后置system、8verbosity/高级参数仍下一步（成本是请求明确拒绝或漏功能）；9外部/anchor/ID/循环/布尔schema及广泛关键词按本节门控拒绝，本地编码引用缺陷已修，不能把它排除（成本是未支持的schema拒绝）。10矛盾/不可满足schema、enum/type一致性、服务数值/复杂度及替代整数表示不承诺完整metaschema验证，已接受约束原文保留（成本是服务拒绝/无可满足结果）；11wrapper name仅本地标签，其他wrapper语义明确拒绝（成本是请求者期望的额外意图未支持）；12既有完整native请求绑定拒绝schema/注释改变或移除，不自动迁移（成本是需要新分支/线程或后续承接策略）。13真实推理/摘要/context及14图像/MIME/displayName/组合服务接受无新增live证据（成本是错误模型能力承诺）；15旧Gemini/Anthropic Minor未修未复审（成本是夸大覆盖）；17新精确源码三平台CI仍必需，旧CI不能代验（成本是漏平台问题，macOS Rust≠iOS）；18扩大既有HTTP没有独立集成RED，如实记录编译器RED→GREEN及集成验收（成本是夸大TDD证据）。

结构输出源码a097b16322f9e680119c4d3d4a656ecaebee42ab的[CI37690749429](https://github.com/bboytang/CAIdex/actions/runs/37690749429)三平台completed/success，精确head及全部步骤核对；Google catalog4/content3/history6/HTTP23/request8/media6/reasoning5/stream4/structured7/tools6共72个名字逐平台各通过一次。workspace Linux294/Windows289/macOS293，0失败，ignored32/30/30；真实Runtime Linux30/WindowsmacOS29，0失败/0ignored；fmt/Clippy/native credentials/schema/doctor通过。日志/tmp/caidex-ci-37690749429-status.json与{-watch,-linux,-windows,-macos}.log，跨机器以GitHub为准。容量注释不影响macOS成功，Rust CI不是iOS构建；本轮不认证生成值/商业Full或实际Gemini Runtime接线。继续HANDOFF记录的其余Runtime参数阶段，不重做结构输出。


## Runtime参数转换（三平台已验收）

基线793af21；固定Runtime build_responses_request确实发送include、prompt_cache_key、client_metadata、可选text/service_tier/stream_options/access_programs。经典/Lite均需保留本地归属；Lite启用工具时parallelfalse门控仍独立未解决。参考Anthropic现有模式，不复制HTTP栈，不改架构。

验收顺序：参数/格式配置与错误门控实际RED→GREEN；持久化v1/v2完整native请求前缀验证；扩大现有三轮JSON/SSE HTTP；完整workspace/fmt/Clippy；一次独立审查及必要一次修复；精确源码三平台CI。

裁定：client_metadata字符串map、prompt_cache_key非空字符串仅在执行端显式retain_runtime_metadata下原样保存在source；不生成labels/safety_identifier/cachedContent/cache promise（成本是缓存提示只保留本地，归属消费者后续接线）。include只接受唯一reasoning.encrypted_content，optional null视缺省；空stream_options可接受，非空delivery及非null access_programs拒绝（成本是这些OpenAI专有行为没有支持）。

裁定：verbosity low/medium/high需执行端显式提示映射，保留原文，提示位于原instructions及初始system/developer之后，比较历史前缀前即写入systemInstruction；未知/重复/空配置拒绝，不冒充原生长度保证（成本是输出风格仍取决模型）。保持完整签名Part，提示改动/移除及native tier改变拒绝旧v1/v2，不自动迁移；本地metadata变化不会改变native前缀（成本是配置改变需新分支/线程，局部metadata不是签名认证）。

裁定：[GenerateContent/ServiceTier参考](https://ai.google.dev/api/generate-content.md#ServiceTier)已有顶层serviceTier的standard/flex/priority；执行端显式source auto/default→standard、flex→flex、priority→priority，无自动推断、无跨服务SLA/价格等价承诺；错配/重复配置拒绝，未映射请求拒绝（成本是需要按目标模型/账号验证可用性）。未配tier时不附加字段；源store:false不关闭CAIdex本地会话/原生历史持久化，也不构成Google日志/保留保证；本编译器不擅自改项目logging设置。


本地结果：参数3先有效RED0/3→GREEN3/3；新增映射/历史3在仅API声明/构造壳时实际RED3/6→GREEN6/6，初始化合法但native字段/提示未实现、错误配置被接受。Lite fixture纠正为固定Runtime的developer前缀而非禁止的top-level instructions，core未改。参数6/request8/structured7通过；扩大既有HTTP1的四组经典/Lite×budget/level三POST通过（HTTP没有独立实现前RED）；完整workspace300passed/0failed/32ignored、Google78个名字各通过一次；Clippy -D warnings/fmt/diff通过。唯一审查及精确源码三平台验收已完成，见以下收尾证据。日志/tmp/caidex-google-parameters-{red,green,mappings-red,mappings-green,http,workspace,clippy}.log。


唯一独立审查：无代码Critical/Important；报告/tmp/caidex-google-parameters-review.md。审查标为Minor的store:false文档被执行者重定级为Important：错误的本地不保留承诺涉及隐私预期，已一次更正文档，代码未改、无复审。证据为既有持久化native载体测试及完整回归；人类文档不新增镜像测试。无新增暂缓Minor，旧Minor不变。

审查20项排除裁定及成本（非隐含支持）：1商业tier可用性/准入/价格/fallback/SLA无live证据，仅选原生类（成本是服务拒绝/错误承诺）；2prompt_cache_key无原生cache收益（成本是仅有本地hint）；3归属消费者后续接线（成本是保留source尚非运行遥测）；4verbosity模型质量/硬长度无证明（成本是提示可能未遵循）；5执行端矛盾提示不做语义求值（成本是profile需负责有效指令）；6非空delivery/access明确拒绝（成本是相应功能缺失）；7其他source controls/native类/include仍门控拒绝（成本是未映射请求拒绝）；8Google账号logging/生产保留保证无证据，文档本地保留歧义已修（成本是须单独配置和验收）。9六方法/Registry/Gateway/固定Gemini Runtime尚待实现（成本是尚不可用户选择/Full）；10固定Lite启用工具默认parallelfalse仍独立unsupported，fixture true不证明Runtime成功（成本是Lite接线前须门控）；11发现/网页/deferred/strict工具/grammar硬约束/后置system保持既有拒绝（成本是功能缺失）；12结构绑定不认证签名/伪造整组/权限/Key owner，H/I落实生产边界（成本是不能认领未认证历史）；13不自动松绑或迁移native前缀，tier/style改变需新分支/线程，等价source alias且native相同可回放（成本是切换受限）；14商业媒体/推理/摘要/context/工具/schema/responseFormat组合无live验收（成本是profile能力仅声明）；15不做完整schema/生成业务值求值/外部解析（成本是不合预期结果须应用验收）；16旧Gemini/Anthropic Minor未改未复审（成本是已知边界仍在）；17扩大HTTP没有独立实现前RED（成本是不得夸大TDD）；18新源码三平台未验前不得引用旧CI替代（成本是漏平台问题，现已按以下收尾证据完成）；19H–R Host/UI/Windows桌面UAC/iOS真机签名/同步Relay/权限仍后续（成本是最终项目未交付）；20未做穷举fuzz/病理profile压力或形式化证明，当前source/final byte bounds与焦点测试支持已测路径（成本是不能承诺任意压力界限）。


源码05f6f9182c101c16fd67565b9c4fa7136c4c294e已公开提交/push；[CI37693355853](https://github.com/bboytang/CAIdex/actions/runs/37693355853)三平台completed/success，head、三job及全部步骤结论已核对。Google原72+runtime_parameters6共78个名字逐平台各通过一次；workspace Linux300/Windows295/macOS299，0失败、ignored32/30/30；既有实际Runtime Linux30/WindowsmacOS29，0失败/0ignored；fmt/Clippy/native credentials/schema/doctor通过。watch退出0，最终status及三job日志/tmp/caidex-ci-37693355853-{status.json,watch.log,linux.log,windows.log,macos.log}。

下一步：六方法/Profile/JSON与SSE投影→Registry/Gateway→固定实际Runtime；沿Anthropic已有边界和本次已验映射，不重复参数阶段。非空context headers、Lite parallelfalse工具、发现/网页等尚未有新等价支持；既有Runtime绿结果不证明Gemini已接线，Rust macOS也不证明iOS交付。


## 六方法与增量投影（三平台已验收）

基线dffc2d0，沿既有ModelProvider/Anthropic模式，不重规划架构。执行端GeminiModel/Profile绑定native route、预算、显式映射/能力；Provider复用同一客户端认证/TLS/slot/取消/no retry，六方法及JSON/SSE再接Registry/Gateway。

投影先送文本/摘要增量，不把签名写进delta；原生没有message_stop，只有正常EOF完成完整NativeHistory校验后才能发output_item.done/terminal和工具调用。functionCall的STOP/非STOP决定它是否可执行，第一次非thought调用后出现的文本暂缓至EOF，确保最终output_index与既有native投影顺序一致；这仅延迟调用后的混合文本，不缓冲全部正常文本。完整chunk/Part索引及v2原声明保持，metadata后缀/非STOP/blocked/error仍如实收尾。

裁定：固定编译器只请求一条candidate，投影选择index0；blocked选None；不把native modelVersion当执行路由名字，原始版本作为native metadata保留。生成ID需在首帧之前唯一固定，不能依赖可能晚到/缺省/复用的Google responseId；复用已安装getrandom（同Gateway版本）而非时钟/计数器。成本是本地response ID与原生ID不同，后者单独保留，不承诺真实版本/商业Full。

裁定：不支持的parallelfalse/Lite工具/发现/网页等仍明确拒绝；metadata为空的原生有效帧可以作为本地进度heartbeat，纯SSE comment当前native parser不生成事件，不能夸大跨Gateway keepalive支持；实际Registry/Gateway阶段需验证该边界。当前Gateway transfer仅用绝对deadline，native读取每个网络chunk重设idle计时；成本是unsupported请求及comment没有向下游转发，固定Runtime下游idle行为仍需下一阶段验证。

验证顺序：投影与Provider有意义RED→GREEN；真实loopback JSON/SSE/三轮持久恢复、取消/Drop/错误/边界；完整workspace/fmt/Clippy；唯一审查及必要一次修复；精确源码新三平台CI。既有Runtime测试不是新增Gemini接线证据。

本地阶段证据（尚未全阶段验收）：投影4、Provider六方法HTTP3均有效RED→GREEN；新增真实Gateway经典/Lite、token隔离、JSON/SSE/原生历史验证及wrapper取消/Drop/截断2项集成通过，没有额外独立RED。响应头负例1项发现空x-request-id被接受：有效RED→GREEN修复共享native headers路径，JSON/SSE均覆盖空/重复/过长/非ASCII，仍不转发x-codex-turn-state。Provider/HTTP新6共通过；源码未提交，尚待完整回归/唯一审查/新精确CI。首次边界编译磁盘满不是有效RED，清理本项目可再生成incremental缓存后重跑。

唯一独立审查：无Critical/Important，报告/tmp/caidex-google-provider-review.md；投影4由审查者独立复验。新增1覆盖Minor暂缓：wrapper取消用例在text delta后才取消，pending投影队列已空；已证明原生连接/slot释放及无terminal/history，未专项证明队列非空取消/Drop。成本是不得声称立即丢弃所有已缓冲进度，后续真实Runtime取消可补该路径；旧Minor不变，不派复审。

审查10项排除裁定及成本（非隐含支持）：1固定Gemini Runtime执行/审批/持久恢复/下游idle仍下一阶段，现有Gateway不代验（成本是尚非用户可用完整Codex）；2商业账号/模型/签名有效性/tier/cache/质量及Full无live证据，配置和fixture仅声明/协议（成本是实际服务拒绝或错误承诺）；3Lite工具parallelfalse明确拒绝，true fixture不证明固定Runtime路径（成本是Lite接入范围受限）；4发现/网页/strict/deferred/后置system/grammar硬约束仍unsupported或仅指导（成本是功能缺失）；5metadata/cache hints仅保留source，下游消费者未实现（成本是无遥测/缓存承诺）；6JSON载体结构绑定不是认证，H/I生产权限及签名真实性后续验收（成本是不能认领伪造整组/未授权历史）；7既有Google/Anthropic Minor未重做（成本是旧边界仍在）；8未穷举fuzz/极端压力/形式证明，焦点测试和byte guard只支持已测路径（成本是不能承诺任意压力行为）；9新精确源码三平台CI仍必须，旧CI不代验（成本是漏平台问题）；10H–R Host/UI/SSH/iOS/Relay/设备签名仍后续（成本是整个产品未交付）。本轮Gateway idle文档按实码纠正：只有绝对deadline，native收到comment chunk会重设idle，但comment不向下游转发，固定Runtime下游仍待验。

最终本地：完整workspace310passed/0failed/32ignored，Google88个名字各通过一次；Clippy -D warnings、fmt、diff通过。Clippy初次发现投影测试不必要clone，改为slice::from_ref后Clippy及投影4复验通过，生产行为未改。全部Key/回复为合成，源码待以下提交/CI记录后才认领三平台。

源码530ead5535b6bc6cd8896317b7360c7872a98571已提交/push；[CI37696809902](https://github.com/bboytang/CAIdex/actions/runs/37696809902)三平台completed/success，精确head、三job及所有步骤结论已核对，watch终态0、三job完整日志下载0。Google原78+投影4+HTTP/Provider6共88个名字逐平台各通过一次（嵌套http/provider.rs使用provider::前缀，核验脚本已递归）；workspace Linux310/Windows305/macOS309，0失败、ignored32/30/30；既有实际Runtime Linux30/WindowsmacOS29，0失败/0ignored；fmt/Clippy/native credentials/schema/doctor通过。日志/tmp/caidex-ci-37696809902-{status.json,watch.log,linux.log,windows.log,macos.log}。无新增商业或实际Gemini Runtime证据，Rust macOS不等于iOS应用构建。

本阶段六方法/投影/Registry/Gateway离线范围已验收；下一步固定Runtime的实际请求、显式本地context归属、已知unsupported前Key拒绝、支持范围内审批/工具/取消/落盘重启。保持Lite parallelfalse、发现/网页/strict/deferred/后置system门控和完整native request绑定，不改架构，不重做本阶段。


## 固定Runtime接线（三平台离线已验收）

- 固定Codex公开model_catalog_json配置复用gpt-5.5/gpt-6.1-sol模板，仅fixture别名/展示描述和supports_search_tool=false；默认高级工具拒绝仍单独实际验证，非Full。显式web_search=disabled范围避免尚无native等价的网页，不在Gateway过滤声明。
- GeminiConfig::with_local_runtime_context仅允许3个固定Runtime header留本地，不映射native路由/cache；默认非空与turn-state拒绝保持。GET/JSON/SSE实际HTTP正例与负例1项有效RED→GREEN。
- 执行端GeminiModel.enforce_single_tool_call默认false；opt-in才允许parallel_tool_calls=false。成功交付前在JSON或SSE正常EOF校验STOP非thought调用数量，超限整轮google_tool_call_limit_exceeded，工具/完整载体/terminal均不发布，无自动重试。tool_choice=none/无tools上限0。裁定：这是本地交付保证，不是Google生成约束；错误成本为原生生成已消耗但整轮报错，不截断/删调用/伪改签名/提示词冒充硬保证。前缀绑定不松动，native wire与历史仍原样。
- context1、单调用1（JSON/SSE×单调用、双调用、none、MAX_TOKENS）有效RED→GREEN；实际Runtime默认拒绝4模式、经典/Lite三轮签名/落盘重启、Lite执行/MCP/取消与完整套件均已本地通过，唯一审查已完成，新三平台CI通过（见本节收尾证据）。准确恢复点与日志见HANDOFF.md。

- 增补本地实际证据：经典/Lite三轮含重启后磁盘恢复；Lite Code Mode审批前无marker，批准后实际生成marker，精确canonical custom call/input/result与native functionResponse、签名content逐值匹配，移除marker后重启不重复执行。经典静态MCP明确禁用发现，echo实际调用/result逐字保留，重启后新MCP进程toolCalls=[]；不宣称Gemini动态发现。classic/Lite interrupt实际socket关闭。新Lite profile实际有效RED→GREEN。
- 实际双调用负例暴露共享Gateway通用SSE error只被固定Codex用于flex-unavailable，安全code被忽略变成generic断流。固定源码sse/responses.rs/responses_error.rs已核对；共享出口改response.failed/response.error，无成功终态，更新既有坏流/timeout测试；坏流有效RED→GREEN与实际双调用safe code、无审批/执行/载体且1Key/1POST已绿。裁定：共享修复适用于各Adapter，HTTP错误格式不变；新的SSE消费者读取response.error，成本为旧消费者只认error事件时需迁移。完整本地回归、唯一审查及精确源码三平台CI均已通过；仅认领本节离线支持范围。

本轮最终本地完整workspace312passed/0failed/39ignored；另完整实际Runtime37passed/0failed/0ignored，Google7各一次，新增Google离线测试合计90。Clippy -D warnings/fmt/diff通过；Python fixture语法解析通过。唯一独立审查与新精确源码三平台CI已完成；实际服务与凭据都是合成，固定Runtime证据不授予商业模型Full。


### 本轮唯一独立审查与裁定

唯一fresh-context审查/tmp/caidex-google-runtime-review.md，无Critical/Important；1项Minor暂缓：单调用新回归未专项覆盖thought调用豁免、无tools分支及未opt-in时none。现有共用guard/映射检查正确，无观察到执行缺陷；不可夸大覆盖。不派复审。

以下按审查所有排除项逐项裁定，并记录错误成本：

1. 商业Gemini/账户/签名真实性/计费/native tier-cache/Full：没有真实Key/API证据，保持未验。误判成本是商业能力与费用承诺错误。
2. Google原生单调用生成：仅交付前计数保证；多预测整轮失败，无裁剪/重试/签名重写。成本是已消耗生成与失败轮次。
3. discovery/web/strict/deferred/后置system/硬grammar：门控及显式opt-out保留；正例不是不限范围的默认Codex兼容。成本是对应请求明确拒绝，不能标Full。
4. 3个context原生routing/cache/session：仅本地归属，不转发；新增消费者需验证自己的策略。成本是无原生缓存/会话路由收益。
5. 整组history伪造/密码学认证：结构与原请求归属校验不认证载体/签名，生产访问控制留H/I。成本是权限被绕过时可整体替换有效结构历史。
6. 历史生成受新local policy追溯：限制本次新generation交付，不伪改旧signed wire或放松v1/v2绑定。成本是过去多调用仍是有效过去记录，不提供策略迁移保证。
7. 低层编译/投影消费者：getter明确消费方负责数量校验，ModelProvider两路径已执行；原生client不执行工具。成本是绕过该入口的自建消费者要自行正确门控。
8. comment-only实际Runtime idle：native网络chunk刷新idle、comment不转发，实际下游idle未单独验；interrupt不代证。成本是只有comment时可能触发下游idle。
9. 旧pending/full-slot/usage/ProtoJSON/thought-phase/prefix-swap/Anthropic Minor：本轮未改变相关覆盖，不重开已记录范围。成本是这些极端/兼容表示仍可能拒绝或展示不准，详前节。
10. SSE旧error消费者：本地错误现在response.failed/response.error，HTTP格式不变，不加双事件迁移层。成本是只识别旧error的调用方需迁移；固定Runtime/core已验新格式。
11. 每Provider每种实际timeout/坏流矩阵：共用出口HTTP/parser测试及实际双调用错误证明当前公共路径，不声称全部组合实际运行。成本是仍可能有未覆盖组合，出现具体失败再定位。
12. 生产Host/ACL/网络/外部exactly-once/UI/SSH/Relay/iOS/真机/UAC/签名/H–R：沿V3顺序未完成；这些重启fixture只证明已完成历史的具体恢复。成本是产品级恢复/平台功能仍不可发布认领。
13. 本源码Windows/macOS：提交后新精确源码三平台CI是前置，不能用旧CI/Linux代证；Rust macOS不等于iOS。成本是未验新增平台差异，收集新CI后再认领。
14. 穷举fuzz/极端负载/全坏输入/全部时序：有限回归与已存预算/slot/deadline是现有证据，不声称形式证明。成本是未测边界仍可能拒绝或失败，实际异常再补回归。

最终本地复验日志workspace-final/all-real-final分别312/0/39与37/0/0；Clippy-final exit0，fmt/diff通过。

源码56f9789e133d94b657b950405ddbf50214afd56b已提交/push；[CI37717424972](https://github.com/bboytang/CAIdex/actions/runs/37717424972)三平台completed/success，精确head、三job及全部step success/skipped已核对。Google90个名字及新增Google实际Runtime7个名字逐平台各一次。workspace Linux312/Windows307/macOS311，0失败、ignored39/37/37；实际Runtime Linux37/WindowsmacOS36，0失败/0ignored；Linux隔离native credentials1及fmt/Clippy/schema/doctor通过。watch exit0、三job日志下载均exit0，逐名脚本exit0。日志/tmp/caidex-ci-37717424972-{status.json,watch.log,linux.log,windows.log,macos.log}。商业模型Full、comment-only Runtime idle及H–R未验；Rust macOS不等于iOS应用构建。

本节实际Runtime离线阶段已验收；下一步兼容API/Ollama，沿V3与现有Provider/Gateway边界，不重做Gemini已完成阶段。


## F/G剩余单调用分支专项（2026-10-10，三平台离线已验）

按[离线门槛核对V1](CAIdex-FG-离线验收核对-V1.md)核实，固定Runtime阶段记录的thought豁免、无tools、未opt-in none三个专项此前未覆盖；重复/部分SSE usage已在history阶段覆盖，不重复开发。

在既有HTTP/Provider测试新增2项，均JSON/SSE×Classic/Lite：`single_call_policy_exempts_thought_calls_without_losing_native_history`验证thought调用不占可执行单调用额度，仅交付actual-call且参数准确，完整原生签名Parts经canonical序列化/恢复保持；`no_tools_and_none_reject_calls_without_single_call_opt_in`验证默认非opt-in时无tools和choice=none额度仍为0，原生返回调用时502且不交付工具/载体/成功终态，Key读取与POST各一次，无重试。纯新增测试，不改Provider生产校验、Runtime、fixture、依赖或workflow，也不开放未支持语义。

定向3/0/0（含旧单调用测试）、Google完整92/0/0、workspace621/0/73、Clippy workspace/all-targets-D warnings与fmt/diff通过；固定Runtime完整71/0/0及schema指纹通过；workspace通过名精确旧CI+2、Runtime通过名保持，无遗漏/重复。精确源码`d9b0d1a49d2aa78822d4110d07a5dc91a22e0800`/[CI38079737190](https://github.com/bboytang/CAIdex/actions/runs/38079737190)三平台完整验收通过。首次定向失败仅为沙箱禁止loopback监听，允许socket环境复跑通过；不是生产缺陷RED。完整Runtime首次启动因本会话未安装项目固定二进制而71项NotFound，按既有CI安装0.160.1、版本与schema校验后完整重跑71/0/0，不以全局0.162.1替代。唯一独立只读审查无Critical/Important；计划中多余“无调用正例”措辞已删，实际只认领上述范围。thought-only文本phase、usage下界、整组互换/满槽cancel及商业/Host等其他缺口仍保留核对表，不因本轮测试宣布F/G完成。

三平台收尾：[CI38079737190](https://github.com/bboytang/CAIdex/actions/runs/38079737190)整体completed/success，Linux114293993979/Windows114293994016/macOS114293993905各17steps成功或条件跳过，完整日志2298/1984/1995行。workspace621/616/620（0失败、忽略73/71/71），固定Runtime71/70/70（0失败/忽略）；Google92每名每平台一次，全通过名693/686/690精确旧CI38072098372+2，旧Runtime保持。watch、三日志下载及精确SHA/步骤/逐名checker通过；不因本轮2项测试升级商业能力或F/G整体状态。


## F/G 剩余离线边界补齐（2026-10-10）

缺prompt计数时仍验证usage已知下界：cached只作为prompt子集，candidate与thought各计算一次，已知下界超total报google_usage_inconsistent，checked加法溢出报google_usage_overflow。不完整计数继续返回usage=null，原始usage保留，不补零或重复加toolUsePrompt。既有usage表增加合法缺计数、单项/组合矛盾及溢出。修复前矛盾用例真实RED，修复后通过。

STOP分类仅将非thought的functionCall视为可执行工具调用。thought-only原生调用不生成客户端工具，最终可见文本为final_answer；有真实调用仍为commentary。JSON/SSE（含逐字节）完整native及signature恢复、done/终态phase对照已验；旧自动回放拒绝用例继续报unsupported_google_replay_call，不放宽历史门控。修复前phase专项真实RED，修复后通过。

新增完整载体组互换专项：同模型两条不同输入的JSON/SSE完整组均先验证自身合法并序列化恢复，双向移入对方历史；Classic/Lite、JSON/SSE目标入口均在Key/POST前400 google_history_request_mismatch。不修改组内内容，也不认为载体JSON是加密认证。原生/投影已有cancel/Drop用例扩展到不消费队列：多帧停流，cancel先观察socket关闭再排空缓存并验证终止错误，不交付工具done/载体/成功终态；Drop关闭socket、共享permit可复用，Key读取恰好2次。既有消费后取消/截断及满槽deadline用例保持。

本地Google94/0/0、workspace623/0/73、固定Runtime71/0/0、Clippy workspace/all-targets-D warnings、fmt/diff通过；最终双向组互换专项复验1/0/0。当前待提交及精确三平台CI。新增2测试、增强3既有用例；生产仅调整共享usage校验与outcome分类，无依赖/fixture/workflow变更。商业真实模型由用户在全项目完成后自行验证，当前未验，不升级LiveRuntime/Full。
