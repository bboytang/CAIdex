# CAIdex 项目交接

更新：2026-10-08 17:52 UTC（Ollama Lite实际Runtime三平台离线验收完成；下一步DeepSeek兼容API）。每次先读本文件、AGENTS.md及Git状态；按V3续接，不重规划架构。详细历史证据留docs各阶段验收文档。

## 当前任务

F/G：Ollama当前离线Adapter与固定Classic MCP/Lite Code Mode范围已三平台验收：源码f25179bf6e5446735b2323fc5169007ecfea947a已push，[CI37818503514](https://github.com/bboytang/CAIdex/actions/runs/37818503514)3job及每个17steps成功或条件跳过，完整raw逐名核对通过。workspace381/376/380（0failed、ignored45/43/43）、固定Runtime43/42/42（0failed/ignored）；Ollama69/OpenAI11/Custom7/Google90及全部Runtime每名各平台一次。新增4项Lite实际审批/隔离执行/磁盘重启不重复/多调用拒绝/流及审批取消已验；原两轮macOS容量失败保留，新提交累计补齐证据。下一步DeepSeek目录/六方法经典基础JSON/SSE，契约已核对并记录docs/DeepSeek；真实模型Live/Full、完整F/G和H–R未完成。

## 已完成

- A–E当前范围：V3/UI/品牌/固定上游、Rust workspace/三平台CI、真实app-server facade、审批/Queue/Steer/Goal/MCP/PTY离线回归；执行端Broker、Windows keyring/Linux Secret Service/受保护文件/env、脱敏与CLI凭据入口。
- F/G：经典/Lite canonical wire、ModelProvider六方法/Registry证据门槛、独立Custom Responses、OpenAI原生；loopback Gateway独立token、取消/背压/TLS/安全错误已三平台验。
- Anthropic：原生Models/Messages、JSON/SSE/参数/工具/媒体/推理/结构输出、v3前缀与组织绑定、v4动态发现；经典显式禁用web后MCP发现/执行/重启、Lite Code Mode审批执行/取消已三平台离线验。默认cached web拒绝，非Full。
- Gemini：Models、native JSON/SSE/HTTP、完整Part/签名/未知字段/大数/v1v2载体、工具映射、请求/媒体/推理/结构/参数、六方法/Profile/Registry/Gateway已三平台验。本轮真实Runtime新增7测试：默认拒绝4路径、经典/Lite各3轮签名含磁盘重启、Lite批准后实际marker/custom结果原文回放且重启不重复、经典静态MCP结果/重启零重跑、Lite双调用拒绝无审批/执行/载体、两路径interrupt关闭native socket。
- 本轮context显式opt-in允许3头只留本地；Lite单调用策略显式opt-in在正常EOF交付前校验，超限整轮失败，不承诺Google生成控制。共享Gateway本地流错误改response.failed/response.error，固定Runtime显示安全code，HTTP错误格式不变，无重试。

- Ollama：原生目录/六方法、show/thinking、bound v1/v2 history、图片/结构交付、Runtime context/verbosity、namespace/custom mapping、client search/deferred、Lite请求与单调用交付已实现并三平台离线验；当前Provider69项。实际固定Classic MCP发现/执行/磁盘恢复及Lite Code Mode审批/执行/取消/恢复已三平台验。原始字符串/未知大数保持，未支持控制在Key/POST前拒绝，仍共享Custom transport/Broker与固定Runtime，无真实daemon/Live/Full。

## 下一步顺序

1. 按docs/CAIdex-DeepSeek-Provider-设计与验收.md实现原生Models目录及六方法经典基础JSON/SSE：缺created必须有效，不能放松现有OpenAI parser；字段/未知数字保持、目录与显式路由交集不授予Live/Full。复用Custom transport/Broker、独立DeepSeek凭据归属。先检查现有Custom/OpenAI配置及请求/流入口，新增有效样本/拒绝边界回归，再最小实现，完成后完整检查与精确源码三平台。
2. 随后DeepSeek固定Runtime/工具/namespace/Lite：兼容表已确认developer降级user、未知input/内置tools忽略、parallel flag忽略、summary/encrypted_content/verbosity不等价，必须显式编译/拒绝及绑定历史。再Qwen/OpenRouter；不下载模型或调用商业API。Ollama三平台当前范围已完成，无变更不重跑、不重造其Adapter或工具执行器。
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

- Ollama尚无真实daemon/模型/Live；固定Classic MCP及Lite Code Mode实际Runtime三平台离线正例已通过。Lite Adapter6项、实际Lite Runtime4项分别验证，不混用。show/thinking、native history、images/结构交付、runtime context/verbosity的既有三平台范围不重做；默认高级能力仍明确拒绝。snapshot仅绑定模型ID，非来源认证/version锁；native vision不自动开启媒体。README、docs/Ollama与Model-Gateway已同步实际Lite本地验收；精确源码三平台证据见CI37818503514与docs/Ollama。

- Gemini实际Runtime阶段唯一独立审查无Critical/Important，1覆盖Minor暂缓：thought-call豁免、无tools、未opt-in none缺直接专项。14排除项裁定/成本留Gemini文档；不派复审。
- 旧Minor：prefix-only v1v2完整组互换、非空projection pending取消/Drop、满槽取消/Drop、ProtoJSON替代整数/空ID表示、部分usage下界矛盾、thought-only phase；Anthropic重启第三轮/完整Lite custom结果覆盖等，详Provider文档，未认领修复。
- comment-only native chunk刷新Provider idle但不转发；实际Runtime下游idle单独未验。同步SecretStore读开始后不可强停，只保证取消后不POST。商业Key/签名真实性/Full、生产Host权限、UI/iOS/真机/签名均未验；Rust macOS CI不是iOS。
- 断开前额度不足造成的自动审批中断已恢复；依赖修改漏记已补齐，无当前审批阻塞。完整项目/文档公开及离线fixture已授权，不重复询问；不读用户模型Key/调用商业API。
- .git普通沙箱只读，提交/push require_escalated；gh bboytang，push用 git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main，不改全局/输出凭据。普通沙箱loopback受限；完整测试用授权环境+TMPDIR=/var/tmp，保留/tmp/.git保护。本机缺Windows/Xcode/gnome-keyring-daemon，对应GitHub验证。

## Git / 文件 / 验证

- 既有Ollama show/thinking、v1历史、图片/非严格格式、offline严格输出、runtime context/verbosity、namespace/search、custom→function/v2均已有精确三平台证据，包含在本轮69项回归，不重做。v1不自动升级/混用，grammar仅指导；历史源码/RED/GREEN/完整CI日志索引留docs/Ollama。

- branch main；最新Runtime测试源码f25179bf6e5446735b2323fc5169007ecfea947a，已push；后续提交仅文档。两份Runtime测试文件已提交，无未完成源码；DeepSeek契约文档已提交；本次仅HANDOFF、README、docs/Ollama及Model-Gateway补三平台收尾证据。当前提交状态以git status为准。未改生产源码、依赖或workflow。

- 中断核查：原/tmp/caidex-ollama-lite-runtime-first.log确认4/0/0，断开时运行已成功结束，非丢失或失败。当前工作区重新完整验证：/tmp/caidex-quota-audit-{workspace,runtime,clippy}.log；workspace381/0/45（新增4项被默认ignored，另有原41项），显式固定Runtime43/0/0，旧39与新4每名各一次；Clippy全targets-D warnings/fmt/Python compile/diff均通过。只使用合成凭据与隔离临时marker，无商业API/用户Key。文档/远程备份及新精确head三平台均已补齐，未发现本次范围的功能错误。
- 基准：docs/CAIdex-实施计划-V3.md、CAIdex-UI-规范-V1.md，原V2只作需求背景。相关代码model/providers/ollama/src/{tools,mapped_tools,runtime,structured,request,history,history_stream,lib}、tests/{tools,runtime,structured,content,history}；其他模块model/providers/custom、model/core、model/gateway、runtime/bridge/tests/{real_runtime.rs,fixtures}、credentials/core、apps/cli、upstream/codex、.github/workflows/ci.yml、scripts。
- jsonschema固定0.58.6、只开arbitrary-precision、强制Offline retriever，无HTTP/file解析；新增41依赖，无既有包升级/删除，hashbrown启用依赖、bit-vec/r-efi更新多版本标识，base64保持0.22.1。标准Schema按库draft/约束校验，未知format拒绝，未知注释保留。同步求值有字节/Regex回溯限制，不承诺硬CPU抢占，留Host隔离；不重试或修补坏回答。
- 旧磁盘满已解除，只清本项目可再生target；本次df约4.8G可用，下轮先查df。完整测试TMPDIR=/var/tmp，保留/tmp/.git保护，不清源码/凭据/保护目录。本机缺Windows/Xcode/gnome-keyring-daemon，对应CI检查；iOS未建立，不以Rust macOS CI冒充iOS。

- 本轮先完成真实Codex0.160.1/direct loopback wire捕获（非Ollama Adapter正例）：/tmp/caidex-ollama-runtime-{classic,lite}-wire.json及probe.log（exit0）。经典含developer/cache/client_metadata/include/text.verbosity与custom apply_patch、tool_search/web_search；Lite含additional_tools、namespaced custom exec、parallel=false、reasoning.context=all_turns。接入前据实际字段逐项编译/拒绝，不能用伪造Core fixture冒充完整Runtime。

- Ollama已落实的边界：固定Ollama responses.go仅声明DeferLoading、不实施延迟可见性，custom/freeform不原生支持；转换后需同时绑定原始声明/kind与actual native tools，不能把只绑定native编译前缀的v1当作已绑定custom。保留v1，不自动升级/松绑；参考现有Provider codec、不引入另一Provider依赖或执行器。Custom ConfiguredModel::with_metadata要求dialects完全一致；Lite当前以公开原dialects/内部Classic路由处理，已验证；Core已有custom input delta。后置developer/unknown phase仍未支持。

- 已提交Lite Adapter契约：with_lite_options为显式构造入口，默认new/with_options继续拒绝Lite路由；original metadata先验证，内部Classic路由与公开原dialects分开。首个developer additional_tools仅接受type/id/role/tools，id只消费运输身份，原工具声明进v2；未知/错位/重复/畸形声明及原始body超限在Key/POST前拒绝。Lite-only与两dialect声明逐请求门控；native不携Lite头、additional_tools或parallel flag。v2 source另存lite_single_tool_call布尔，None代表Classic，false/true分别为Lite无约束/本地单调用，原始声明及策略不能改写旧组。NativeTools共享终态校验在JSON/SSE工具/完整history交付前拒绝多调用，failed/incomplete也检查，slot可复用；无native生成约束承诺。定向6/0/0，日志/tmp/caidex-ollama-lite-{red,green,focused}.log；RED是旧mapping400，非编译失败。完整本地workspace381/0/41、Ollama69逐名一次、既有固定Runtime39/0/0、Clippy全targets-D warnings/fmt/diff已过；日志同前缀{workspace-final,all-real-final,clippy-final}.log。此处39项是已提交Adapter阶段的旧回归；随后f25179b中4项实际Lite Runtime及43项完整回归已本地及三平台通过，Live仍未验。全局codex-cli 0.160.1可用；本机没有CI专用.tools/codex目录，定位脚本ENOENT不影响本轮实际Runtime运行。Adapter源码已提交/push；实际Code Mode测试f25179b已push且三平台收尾完成。

- CI收尾：37818503514精确headf25179bf6e5446735b2323fc5169007ecfea947a全部3job/17steps完成success或条件跳过；Linux113453190593/Windows113453190663/macOS113453190281完整raw1932/1619/1630行逐名及总数已验，workspace381/376/380、Runtime43/42/42均0failed，Linux native credentials1。原始/标注日志=/tmp/caidex-ci-37818503514-{linux,windows,macos}-raw.log及同名前缀.log，status.json/watch.log；watch会话15353、下载13870均exit0，normalize/check/available脚本同前缀/tmp/caidex-ollama-lite-runtime-ci-{normalize,check,available}.py全部exit0。旧37816328543（head8ef915d）macOS零steps容量取消整体failure，新head完整继承回归补齐证据，不假称旧CI成功、不重启原job。下一步DeepSeek仍仅文档核对，无其源码/测试/六方法/真实模型验收。
