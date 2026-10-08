# CAIdex 项目交接

更新：2026-10-08 17:38 UTC（Ollama Lite实际Runtime测试及验收文档本地收尾，待提交/新CI）。每次先读本文件、AGENTS.md及Git状态；按V3续接，不重规划架构。详细历史证据留docs各阶段验收文档。

## 当前任务

F/G：Ollama Lite Adapter源码8ef915d已push；随后新增4项固定Runtime Lite测试已实现、本地通过，尚未提交。已恢复中断前状态并同步README、Ollama与Model-Gateway验收文档，正在提交既有测试。未提交测试覆盖Code Mode审批后实际临时marker执行、磁盘重启原文回放且不重复执行、多调用拒绝无审批/执行/载体、native socket中断及等待审批取消。当前工作区完整workspace381/0/45、固定Codex0.160.1 Runtime43/0/0、Clippy/fmt/Python语法/diff通过；暂未发现本次修改的功能错误。源码8ef915d的[CI37816328543](https://github.com/bboytang/CAIdex/actions/runs/37816328543)Linux/Windows成功且完整日志逐名核对，macOS queued零steps；该CI不包含本地新增4项，不能认领其跨平台通过。旧CI37809165149 macOS三次容量失败，不循环重试；整体F/G及H–R未完成。

## 已完成

- A–E当前范围：V3/UI/品牌/固定上游、Rust workspace/三平台CI、真实app-server facade、审批/Queue/Steer/Goal/MCP/PTY离线回归；执行端Broker、Windows keyring/Linux Secret Service/受保护文件/env、脱敏与CLI凭据入口。
- F/G：经典/Lite canonical wire、ModelProvider六方法/Registry证据门槛、独立Custom Responses、OpenAI原生；loopback Gateway独立token、取消/背压/TLS/安全错误已三平台验。
- Anthropic：原生Models/Messages、JSON/SSE/参数/工具/媒体/推理/结构输出、v3前缀与组织绑定、v4动态发现；经典显式禁用web后MCP发现/执行/重启、Lite Code Mode审批执行/取消已三平台离线验。默认cached web拒绝，非Full。
- Gemini：Models、native JSON/SSE/HTTP、完整Part/签名/未知字段/大数/v1v2载体、工具映射、请求/媒体/推理/结构/参数、六方法/Profile/Registry/Gateway已三平台验。本轮真实Runtime新增7测试：默认拒绝4路径、经典/Lite各3轮签名含磁盘重启、Lite批准后实际marker/custom结果原文回放且重启不重复、经典静态MCP结果/重启零重跑、Lite双调用拒绝无审批/执行/载体、两路径interrupt关闭native socket。
- 本轮context显式opt-in允许3头只留本地；Lite单调用策略显式opt-in在正常EOF交付前校验，超限整轮失败，不承诺Google生成控制。共享Gateway本地流错误改response.failed/response.error，固定Runtime显示安全code，HTTP错误格式不变，无重试。

- Ollama：六方法/原生目录与配置交集、独立无认证或正确ollama Bearer、经典文本/function历史、typed JSON/SSE/失败终态/Gateway隔离/取消/Drop/slot已有10项。新增6项show/thinking：Custom bounded POST、显式同origin端点、原生声明精度/Unknown/绑定snapshot、JSON/SSE exact等级与bool/default/Unsupported/边界，16项三平台离线通过。native history显式opt-in新增10项（共26）：v1 profile/model/compiled prefix/display group绑定、JSON3轮/SSE2轮、文本进度/延迟工具及坏终态/取消；三平台通过。图片/非严格格式显式opt-in新增6项（共32）：内联/带图结果/JSON&SSE原文、回放、语义拒绝、预算及默认，三平台通过。严格结构输出新增7项（共39）：offline Schema/精确数字/refs、JSON&SSE终态保护、history开关/工具延迟、拒绝/部分状态、取消/Drop/预算/slot复用，三平台通过。未支持控制仍在Key/POST前拒绝，复用Custom transport/OpenAI Models parser及既有base64版本。

## 下一步顺序

1. 收尾已实现的4项Ollama Lite固定Runtime测试：文档已同步、本地回归已过，复核diff后提交/push两份测试源码和文档，以新精确head验证三平台。当前本地完整回归已过，无修改则无需重复本地全套。CI37816328543 Linux/Windows日志已核对，只余macOS queued；新CI不得借旧head代验，更不能把8ef915d的CI当作新增4项测试通过。strict仍是本地交付校验，不承诺native生成控制或HTTP clean EOF；无需重写已完成的Adapter或实际Runtime测试。
2. 随后DeepSeek/Qwen/OpenRouter等兼容API；DeepSeek Responses developer按user、未知input忽略，其models不保证created，不能直接透传/照搬严格catalog。等价路径复用transport，差异明确编译/拒绝；不据URL/目录/fixture授予Lite/Full，不下载大模型/调用商业API。
3. 兼容API/Ollama完成当前范围后，V3 H/I → Windows → SSH/iOS → CLI → Relay → R。iOS simulator/无签名archive在GitHub macOS；真机/UAC/签名/逐模型商业报告独立验。

## 重要架构决定

- Windows11x64 Tauri2+React/TypeScript/Rust；Linuxx86_64 Host/CLI；iOS17+/Swift5 SwiftUI+UniFFI。GUI中文/英文，CLI英文；只用assets/brand原件，UI尽量1:1官方参考，模型/Key放设置。
- 固定真实Codex0.160.1/d27764b82f7118f674371e6d6e76271d9d606edb，一个Runtime，不造第二Agent/HTTP栈。Gateway只适配Responses，不执行工具；Runtime负责发现/审批/沙箱/执行。未知协议字段保留、实验opt-in、经典/Lite分别验收。
- Native wire是回放权威；Gemini v1完整model/request绑定，v2另绑定canonical工具及actual native tools，raw chunks重建核对。Anthropic v3绑定compiled前缀/认证组织，v4保存发现记录。签名/未知字段不改写、不自动升级/松绑；各Provider自建encrypted_content只是敏感JSON载体，Ollama原生此字段是明文thinking，仅显式native history允许绑定回放；v1保留、v2另绑定original tools/kind/发现子集/actual native tools及opt-in policy，均不宣称加密/密码学认证，生产访问控制留H/I。
- Gemini正例公共model_catalog_json复用精确固定gpt-5.5/gpt-6.1-sol模板，仅alias/描述/display_name/supports_search_tool=false；显式禁用web。高级默认与未opt-in Lite负例继续Key/POST前拒绝。工具搜索/web/strict/deferred/后置system未支持；grammar仅指导。local数量限制不追溯伪改旧signed历史。
- Unknown≠Supported，Configured/ProviderCatalog/ProtocolFixture不授予Full/Compatible，须LiveRuntime报告。
- Chat独立无Shell/Git/项目写权限；Remote用执行Host Key，手机不读已存Host Key，同步不含凭据。模型轮次边界切换，跨Provider关联分支/新线程。
- Host后台、SQLite journal先落盘再广播、快照补缺口/请求幂等/审批首次有效，不盲重跑未知结果、不承诺外部exactly-once。Remote先SSH后Noise/Snow Relay；活动线程不迁移Host。Chat自托管HTTPS+SQLite/outbox/cursor/冲突/tombstones，服务端为信任边界，无历史E2EE承诺；unsigned archive不是可安装IPA。

## 问题 / 暂缓项

- Ollama尚无真实daemon/模型/Live；固定Classic MCP及Lite Code Mode实际Runtime本地正例已通过，macOS及新增Lite测试三平台待验。Lite Adapter6项、实际Lite Runtime4项分别验证，不混用。show/thinking、native history、images/结构交付、runtime context/verbosity的既有三平台范围不重做；默认高级能力仍明确拒绝。snapshot仅绑定模型ID，非来源认证/version锁；native vision不自动开启媒体。README、docs/Ollama与Model-Gateway已同步实际Lite本地验收；三平台证据待包含新测试的提交CI。

- Gemini实际Runtime阶段唯一独立审查无Critical/Important，1覆盖Minor暂缓：thought-call豁免、无tools、未opt-in none缺直接专项。14排除项裁定/成本留Gemini文档；不派复审。
- 旧Minor：prefix-only v1v2完整组互换、非空projection pending取消/Drop、满槽取消/Drop、ProtoJSON替代整数/空ID表示、部分usage下界矛盾、thought-only phase；Anthropic重启第三轮/完整Lite custom结果覆盖等，详Provider文档，未认领修复。
- comment-only native chunk刷新Provider idle但不转发；实际Runtime下游idle单独未验。同步SecretStore读开始后不可强停，只保证取消后不POST。商业Key/签名真实性/Full、生产Host权限、UI/iOS/真机/签名均未验；Rust macOS CI不是iOS。
- 断开前额度不足造成的自动审批中断已恢复；依赖修改漏记已补齐，无当前审批阻塞。完整项目/文档公开及离线fixture已授权，不重复询问；不读用户模型Key/调用商业API。
- .git普通沙箱只读，提交/push require_escalated；gh bboytang，push用 git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main，不改全局/输出凭据。普通沙箱loopback受限；完整测试用授权环境+TMPDIR=/var/tmp，保留/tmp/.git保护。本机缺Windows/Xcode/gnome-keyring-daemon，对应GitHub验证。

## Git / 文件 / 验证

- 既有Ollama show/thinking、v1历史、图片/非严格格式、offline严格输出、runtime context/verbosity、namespace/search、custom→function/v2均已有精确三平台证据，包含在本轮69项回归，不重做。v1不自动升级/混用，grammar仅指导；历史源码/RED/GREEN/完整CI日志索引留docs/Ollama。

- branch main；HEAD及GitHub main均d743a9acbf882201a4742702f3c7be7684d7bb60，origin/main一致。未提交：runtime/bridge/tests/real_runtime.rs（显式Lite Harness及4项真实Runtime测试）、runtime/bridge/tests/fixtures/responses_server.py（真实Code Mode原生响应/多调用/停流fixture）、HANDOFF.md（恢复点）、README.md及docs/Ollama、Model-Gateway（当前验收状态）。两份测试文件391 insertions/33 deletions，无生产源码、依赖或workflow改动；已实现并本地验证，提交/push及新head CI未完成。现有远程源码8ef915d的验收与未提交测试明确分开。

- 中断核查：原/tmp/caidex-ollama-lite-runtime-first.log确认4/0/0，断开时运行已成功结束，非丢失或失败。当前工作区重新完整验证：/tmp/caidex-quota-audit-{workspace,runtime,clippy}.log；workspace381/0/45（新增4项被默认ignored，另有原41项），显式固定Runtime43/0/0，旧39与新4每名各一次；Clippy全targets-D warnings/fmt/Python compile/diff均通过。只使用合成凭据与隔离临时marker，无商业API/用户Key。文档同步已补齐，当前缺口为未提交测试的远程备份及新精确head三平台，不是已复现的功能错误。
- 基准：docs/CAIdex-实施计划-V3.md、CAIdex-UI-规范-V1.md，原V2只作需求背景。相关代码model/providers/ollama/src/{tools,mapped_tools,runtime,structured,request,history,history_stream,lib}、tests/{tools,runtime,structured,content,history}；其他模块model/providers/custom、model/core、model/gateway、runtime/bridge/tests/{real_runtime.rs,fixtures}、credentials/core、apps/cli、upstream/codex、.github/workflows/ci.yml、scripts。
- jsonschema固定0.58.6、只开arbitrary-precision、强制Offline retriever，无HTTP/file解析；新增41依赖，无既有包升级/删除，hashbrown启用依赖、bit-vec/r-efi更新多版本标识，base64保持0.22.1。标准Schema按库draft/约束校验，未知format拒绝，未知注释保留。同步求值有字节/Regex回溯限制，不承诺硬CPU抢占，留Host隔离；不重试或修补坏回答。
- 旧磁盘满已解除，只清本项目可再生target；本次df约4.8G可用，下轮先查df。完整测试TMPDIR=/var/tmp，保留/tmp/.git保护，不清源码/凭据/保护目录。本机缺Windows/Xcode/gnome-keyring-daemon，对应CI检查；iOS未建立，不以Rust macOS CI冒充iOS。

- 本轮先完成真实Codex0.160.1/direct loopback wire捕获（非Ollama Adapter正例）：/tmp/caidex-ollama-runtime-{classic,lite}-wire.json及probe.log（exit0）。经典含developer/cache/client_metadata/include/text.verbosity与custom apply_patch、tool_search/web_search；Lite含additional_tools、namespaced custom exec、parallel=false、reasoning.context=all_turns。接入前据实际字段逐项编译/拒绝，不能用伪造Core fixture冒充完整Runtime。

- 下步事实已只读核对：固定Ollama responses.go仅声明DeferLoading、不实施延迟可见性，custom/freeform不原生支持；转换后需同时绑定原始声明/kind与actual native tools，不能把只绑定native编译前缀的v1当作已绑定custom。保留v1，不自动升级/松绑；参考现有Provider codec、不引入另一Provider依赖或执行器。Custom ConfiguredModel::with_metadata要求dialects完全一致；Lite当前以公开原dialects/内部Classic路由处理，已验证；Core已有custom input delta。后置developer/unknown phase仍未支持。

- 前阶段deferred/Classic MCP：源码1a2668f已push，Ollama63、本地workspace375/0/41与真实Runtime39/0/0通过；JSON/SSE客户端可见性、null carrier精确codec、MCP发现/执行/落盘重启已验，重启不重跑。完整日志/tmp/caidex-ollama-deferred-{workspace-final,all-real-final,clippy-final}.log，详细契约/RED-GREEN在docs/Ollama。Linux/Windows完整raw已验；macOS无runner，不认领三平台。

- CI37809165149：首轮Linux/Windows完整raw 1918/1605行已核对，workspace375/370（0failed，ignored41/39）、Runtime39/38（0failed/ignored），Linux native credentials1。macOS attempt1/2/3均零steps、未分配runner；三次annotations均明确hosted runner容量不足，非源码错误。仅重跑macOS，REST确认attempt3同一head已cancelled、零steps，不据两平台认领阶段通过。

- 原macOS job113436013980 completed/cancelled，17:17:35 UTC、零steps，官方annotations为未获hosted runner与arm64 capacity；原CI已终态，不再重启。新Lite源码CI需独立核对精确head/所有job-step/完整逐名日志，不借旧两平台结论；若新CI仍无runner，记录缺口并继续固定Runtime接线，不循环等待。

- 已提交Lite Adapter契约：with_lite_options为显式构造入口，默认new/with_options继续拒绝Lite路由；original metadata先验证，内部Classic路由与公开原dialects分开。首个developer additional_tools仅接受type/id/role/tools，id只消费运输身份，原工具声明进v2；未知/错位/重复/畸形声明及原始body超限在Key/POST前拒绝。Lite-only与两dialect声明逐请求门控；native不携Lite头、additional_tools或parallel flag。v2 source另存lite_single_tool_call布尔，None代表Classic，false/true分别为Lite无约束/本地单调用，原始声明及策略不能改写旧组。NativeTools共享终态校验在JSON/SSE工具/完整history交付前拒绝多调用，failed/incomplete也检查，slot可复用；无native生成约束承诺。定向6/0/0，日志/tmp/caidex-ollama-lite-{red,green,focused}.log；RED是旧mapping400，非编译失败。完整本地workspace381/0/41、Ollama69逐名一次、既有固定Runtime39/0/0、Clippy全targets-D warnings/fmt/diff已过；日志同前缀{workspace-final,all-real-final,clippy-final}.log。此处39项是已提交Adapter阶段的旧回归；随后未提交的4项实际Lite Runtime及43项完整回归已本地通过，见本次核查恢复点，三平台/Live仍未验。全局codex-cli 0.160.1可用；本机没有CI专用.tools/codex目录，定位脚本ENOENT不影响本轮实际Runtime运行。Adapter源码已提交/push；实际Code Mode测试当前已实现，待文档/提交及新CI收尾。

- CI恢复（17:38 UTC）：37816328543精确head 8ef915d77ed4331b8bfb94e83f6b809d31da7fb4；Windows113445789083/Linux113445789222 completed/success，全部17steps成功或条件跳过。完整raw1611/1924行逐名Ollama69/OpenAI11/Custom7/Google90和旧Runtime各一次；workspace Linux381/Windows376（ignored41/39）、Runtime39/38均0failed，Linux native credentials1。原始日志/tmp/caidex-ci-37816328543-{linux,windows}-raw.log，当前status为同前缀audit-status.json。macOS113445788687仍queued零steps，三平台完整check未执行、不认领成功；待macOS实际运行后再验。未来含新增4项的源码CI预期workspace381/376/380、ignored45/43/43，固定Runtime43/42/42，须以实际日志为准，不把本次CI当其代证。
