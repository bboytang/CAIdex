# CAIdex 项目交接

更新：2026-10-08（断开后核对）。每次先读本文件、AGENTS.md及Git状态；按V3续接，不重规划架构。详细历史证据留docs各阶段验收文档。

## 当前任务

F/G：Ollama custom/freeform→native function与v2声明/kind/发现前缀绑定已三平台离线验收，源码f481fc8f94a746a27f0cbb7f05227cfa43f1abbb已push，[CI37787010246](https://github.com/bboytang/CAIdex/actions/runs/37787010246)精确head全部job/step及完整逐名日志通过。断点草稿的多行grammar误拒绝与native custom输入事件提前转发已修复。本轮deferred客户端可见性已定向通过，固定Classic Runtime发现/执行/磁盘重启正例已通过；最终本地workspace375/0/41、固定Runtime39/0/0及Clippy/fmt/diff已通过，源码提交/三平台CI待完成，Lite随后；整体F/G及H–R未完成。

## 已完成

- A–E当前范围：V3/UI/品牌/固定上游、Rust workspace/三平台CI、真实app-server facade、审批/Queue/Steer/Goal/MCP/PTY离线回归；执行端Broker、Windows keyring/Linux Secret Service/受保护文件/env、脱敏与CLI凭据入口。
- F/G：经典/Lite canonical wire、ModelProvider六方法/Registry证据门槛、独立Custom Responses、OpenAI原生；loopback Gateway独立token、取消/背压/TLS/安全错误已三平台验。
- Anthropic：原生Models/Messages、JSON/SSE/参数/工具/媒体/推理/结构输出、v3前缀与组织绑定、v4动态发现；经典显式禁用web后MCP发现/执行/重启、Lite Code Mode审批执行/取消已三平台离线验。默认cached web拒绝，非Full。
- Gemini：Models、native JSON/SSE/HTTP、完整Part/签名/未知字段/大数/v1v2载体、工具映射、请求/媒体/推理/结构/参数、六方法/Profile/Registry/Gateway已三平台验。本轮真实Runtime新增7测试：默认拒绝4路径、经典/Lite各3轮签名含磁盘重启、Lite批准后实际marker/custom结果原文回放且重启不重复、经典静态MCP结果/重启零重跑、Lite双调用拒绝无审批/执行/载体、两路径interrupt关闭native socket。
- 本轮context显式opt-in允许3头只留本地；Lite单调用策略显式opt-in在正常EOF交付前校验，超限整轮失败，不承诺Google生成控制。共享Gateway本地流错误改response.failed/response.error，固定Runtime显示安全code，HTTP错误格式不变，无重试。

- Ollama：六方法/原生目录与配置交集、独立无认证或正确ollama Bearer、经典文本/function历史、typed JSON/SSE/失败终态/Gateway隔离/取消/Drop/slot已有10项。新增6项show/thinking：Custom bounded POST、显式同origin端点、原生声明精度/Unknown/绑定snapshot、JSON/SSE exact等级与bool/default/Unsupported/边界，16项三平台离线通过。native history显式opt-in新增10项（共26）：v1 profile/model/compiled prefix/display group绑定、JSON3轮/SSE2轮、文本进度/延迟工具及坏终态/取消；三平台通过。图片/非严格格式显式opt-in新增6项（共32）：内联/带图结果/JSON&SSE原文、回放、语义拒绝、预算及默认，三平台通过。严格结构输出新增7项（共39）：offline Schema/精确数字/refs、JSON&SSE终态保护、history开关/工具延迟、拒绝/部分状态、取消/Drop/预算/slot复用，三平台通过。未支持控制仍在Key/POST前拒绝，复用Custom transport/OpenAI Models parser及既有base64版本。

## 下一步顺序

1. 接deferred/Lite item转换及固定Runtime工具正例，复用已验custom/freeform→function与v2、native namespace/function/search、共享transport和现有codec，不重做已验范围。strict已由本地交付校验兑现，不重做；原生生成质量留Live验收。Custom在typed terminal后停止读取native，不保证HTTP clean EOF；不冒用Google EOF承诺。caller reasoning仅过codec，未映射native output保留但下轮显式拒绝；不要重做已验图片/格式/history/show/Gemini。
2. 随后DeepSeek/Qwen/OpenRouter等兼容API；DeepSeek Responses developer按user、未知input忽略，其models不保证created，不能直接透传/照搬严格catalog。等价路径复用transport，差异明确编译/拒绝；不据URL/目录/fixture授予Lite/Full，不下载大模型/调用商业API。
3. 兼容API/Ollama完成当前范围后，V3 H/I → Windows → SSH/iOS → CLI → Relay → R。iOS simulator/无签名archive在GitHub macOS；真机/UAC/签名/逐模型商业报告独立验。

## 重要架构决定

- Windows11x64 Tauri2+React/TypeScript/Rust；Linuxx86_64 Host/CLI；iOS17+/Swift5 SwiftUI+UniFFI。GUI中文/英文，CLI英文；只用assets/brand原件，UI尽量1:1官方参考，模型/Key放设置。
- 固定真实Codex0.160.1/d27764b82f7118f674371e6d6e76271d9d606edb，一个Runtime，不造第二Agent/HTTP栈。Gateway只适配Responses，不执行工具；Runtime负责发现/审批/沙箱/执行。未知协议字段保留、实验opt-in、经典/Lite分别验收。
- Native wire是回放权威；Gemini v1完整model/request绑定，v2另绑定canonical工具及actual native tools，raw chunks重建核对。Anthropic v3绑定compiled前缀/认证组织，v4保存发现记录。签名/未知字段不改写、不自动升级/松绑；各Provider自建encrypted_content只是敏感JSON载体，Ollama原生此字段是明文thinking，仅显式native history允许绑定回放，均不宣称加密/密码学认证，生产访问控制留H/I。
- Gemini正例公共model_catalog_json复用精确固定gpt-5.5/gpt-6.1-sol模板，仅alias/描述/display_name/supports_search_tool=false；显式禁用web。高级默认与未opt-in Lite负例继续Key/POST前拒绝。工具搜索/web/strict/deferred/后置system未支持；grammar仅指导。local数量限制不追溯伪改旧signed历史。
- Unknown≠Supported，Configured/ProviderCatalog/ProtocolFixture不授予Full/Compatible，须LiveRuntime报告。
- Chat独立无Shell/Git/项目写权限；Remote用执行Host Key，手机不读已存Host Key，同步不含凭据。模型轮次边界切换，跨Provider关联分支/新线程。
- Host后台、SQLite journal先落盘再广播、快照补缺口/请求幂等/审批首次有效，不盲重跑未知结果、不承诺外部exactly-once。Remote先SSH后Noise/Snow Relay；活动线程不迁移Host。Chat自托管HTTPS+SQLite/outbox/cursor/冲突/tombstones，服务端为信任边界，无历史E2EE承诺；unsigned archive不是可安装IPA。

## 问题 / 暂缓项

- Ollama尚无真实daemon/模型/Live；本轮新增固定Classic Runtime MCP发现/执行/磁盘重启本地正例，三平台待验；Lite仍无正例。show/精确think已有三平台证据，native history显式opt-in三平台已验；默认developer/context/Lite及其他高级控制明确拒绝；本轮runtime context及显式verbosity映射已三平台验收。snapshot绑定模型ID，非来源认证/version锁/实时刷新；native vision声明不自动开启Adapter媒体；显式images/非严格格式/严格交付校验均三平台已验，生成grammar/实际daemon/Lite Runtime仍未验，详docs/Ollama。README旧Anthropic简介滞后、Google未列，以HANDOFF/各验收文档为准，不据旧简介重做已完成阶段。

- Gemini实际Runtime阶段唯一独立审查无Critical/Important，1覆盖Minor暂缓：thought-call豁免、无tools、未opt-in none缺直接专项。14排除项裁定/成本留Gemini文档；不派复审。
- 旧Minor：prefix-only v1v2完整组互换、非空projection pending取消/Drop、满槽取消/Drop、ProtoJSON替代整数/空ID表示、部分usage下界矛盾、thought-only phase；Anthropic重启第三轮/完整Lite custom结果覆盖等，详Provider文档，未认领修复。
- comment-only native chunk刷新Provider idle但不转发；实际Runtime下游idle单独未验。同步SecretStore读开始后不可强停，只保证取消后不POST。商业Key/签名真实性/Full、生产Host权限、UI/iOS/真机/签名均未验；Rust macOS CI不是iOS。
- 断开前额度不足造成的自动审批中断已恢复；依赖修改漏记已补齐，无当前审批阻塞。完整项目/文档公开及离线fixture已授权，不重复询问；不读用户模型Key/调用商业API。
- .git普通沙箱只读，提交/push require_escalated；gh bboytang，push用 git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main，不改全局/输出凭据。普通沙箱loopback受限；完整测试用授权环境+TMPDIR=/var/tmp，保留/tmp/.git保护。本机缺Windows/Xcode/gnome-keyring-daemon，对应GitHub验证。

## Git / 文件 / 验证

- branch main跟踪origin/main，开始HEAD/origin main 16400e1、干净。本轮未提交Ollama lib/request/mapped_tools/history、tests/tools/deferred与mod及tests/history、Runtime Harness/Responses fixture及HANDOFF；deferred与精确content:null规则均有效RED→GREEN，Ollama63逐名一次、workspace375/0/41、固定Runtime39/0/0与Clippy/fmt/diff通过；尚未提交/三平台验，无新依赖。
- 基准：docs/CAIdex-实施计划-V3.md、CAIdex-UI-规范-V1.md，原V2只作需求背景。相关代码model/providers/ollama/src/{tools,mapped_tools,runtime,structured,request,history,history_stream,lib}、tests/{tools,runtime,structured,content,history}；其他模块model/providers/custom、model/core、model/gateway、runtime/bridge/tests/{real_runtime.rs,fixtures}、credentials/core、apps/cli、upstream/codex、.github/workflows/ci.yml、scripts。
- jsonschema固定0.58.6、只开arbitrary-precision、强制Offline retriever，无HTTP/file解析；新增41依赖，无既有包升级/删除，hashbrown启用依赖、bit-vec/r-efi更新多版本标识，base64保持0.22.1。标准Schema按库draft/约束校验，未知format拒绝，未知注释保留。同步求值有字节/Regex回溯限制，不承诺硬CPU抢占，留Host隔离；不重试或修补坏回答。
- 前阶段严格输出源码bd3995e/CI37770508610三平台已通过；详细依赖/Schema/JSON/SSE与历史验收证据留docs/Ollama，不重做。
- 旧磁盘满已解除，只清本项目可再生target；目前约5.2G可用，下轮先查df。完整测试TMPDIR=/var/tmp，保留/tmp/.git保护，不清源码/凭据/保护目录。本机缺Windows/Xcode/gnome-keyring-daemon，对应CI检查；iOS未建立，不以Rust macOS CI冒充iOS。

- 本轮先完成真实Codex0.160.1/direct loopback wire捕获（非Ollama Adapter正例）：/tmp/caidex-ollama-runtime-{classic,lite}-wire.json及probe.log（exit0）。经典含developer/cache/client_metadata/include/text.verbosity与custom apply_patch、tool_search/web_search；Lite含additional_tools、namespaced custom exec、parallel=false、reasoning.context=all_turns。接入前据实际字段逐项编译/拒绝，不能用伪造Core fixture冒充完整Runtime。

- 前阶段请求入口源码d934fe1/[CI37775057618](https://github.com/bboytang/CAIdex/actions/runs/37775057618)三平台已验：本地归属消费、leading developer→system、history下include/auto summary/all_turns、显式verbosity指令，编译前缀绑定及预算；未知/后置developer/turn-state仍拒绝。详细日志/逐名证据留docs/Ollama。

- 下步事实已只读核对：固定Ollama responses.go仅声明DeferLoading、不实施延迟可见性，custom/freeform不原生支持；转换后需同时绑定原始声明/kind与actual native tools，不能把只绑定native编译前缀的v1当作已绑定custom。保留v1，不自动升级/松绑；参考现有Provider codec、不引入另一Provider依赖或执行器。Custom ConfiguredModel::with_metadata要求dialects完全一致，Lite转换须检查metadata/route；Core已有custom input delta。后置developer/unknown phase仍未支持。



- 当前工具范围已实现/三平台验证：with_native_tools自动bound history、namespace说明编入成员且前缀绑定、原生别名碰撞、client search按序发现/重复一致/配对、legacy ToolName精确匹配、JSON/SSE工具身份/args/item与call ID碰撞及跨轮复用拒绝、failed/incomplete search无done。6项有效RED旧flat compiler400→GREEN；最终workspace363/0/40（Ollama51逐名一次）、固定Runtime38/0/0（Ollama拒绝4模式）、Clippy全targets-D warnings/fmt/diff通过。日志/tmp/caidex-ollama-native-tools-{red,green,workspace-final,clippy-final,all-real-final}.log。src/tools.rs与tests/tools新增，lib/request/history_stream及真实Runtime Harness关联改动；源码40edb9a已push，三平台CI37779554830通过；custom/Lite/daemon/Full未验。

- 工具三平台收尾：CI37779554830精确head 40edb9a，3job/全部step终态成功或条件跳过；完整原始日志逐名Ollama51/OpenAI11/Custom7/Google90与固定Runtime各一次。workspace Linux363/Windows358/macOS362，0失败、ignored40/38/38；Runtime38/37/37，0失败/0ignored，Ollama新opt-in第四模式只为拒绝边界。Linux native credentials1及fmt/Clippy/schema/doctor通过。日志/tmp/caidex-ci-37779554830-{status.json,watch.log,linux-raw.log,windows-raw.log,macos-raw.log}（1910/1591/1602行）及step标注日志，/tmp/caidex-ollama-native-tools-ci-{normalize,check}.py，watch/下载/check exit0。macOS排队后正常完成，没有重启/重跑；跨机器以原CI/提交文档为准。

- Custom当前阶段已验：with_custom_tools_as_functions显式opt-in，单input string function、original声明/kind与actual native tools的v2绑定、发现前缀子集、JSON/SSE三轮原始arguments回放；v1不自动升级/混用，grammar仅指导。已修复草稿多行grammar校验及native custom input delta/done提前转发；两项有效RED/GREEN，7项新增（Ollama58逐名一次），最终本地workspace370/0/40、固定Runtime38/0/0、Clippy全targets-D warnings/fmt/diff通过。日志/tmp/caidex-ollama-custom-{red,green,workspace-final,all-real-final,clippy-final}.log及custom-native-event-{red,green}.log；一次测试RequestContext.clone编译误用已修正重验，失败不计通过。deferred/Lite与Runtime工具正例仍未做，无新依赖，详细契约见docs/Ollama。

- Custom三平台收尾：源码f481fc8f94a746a27f0cbb7f05227cfa43f1abbb/CI37787010246三个job全部completed/success，全部step成功或条件跳过；完整raw逐名Ollama58/OpenAI11/Custom7/Google90及固定Runtime每个平台各一次。workspace Linux370/Windows365/macOS369（0failed，ignored40/38/38），Runtime38/37/37（0failed/ignored），Linux native credentials1及fmt/Clippy/schema/doctor通过。日志/tmp/caidex-ci-37787010246-{status.json,watch.log,linux-raw.log,windows-raw.log,macos-raw.log}（1911/1598/1609行）及step标注日志，/tmp/caidex-ollama-custom-ci-{normalize,check}.py，watch/下载/check exit0。被替代首版8540ecd/CI37785263569取消，不能代验修正版；macOS排队后正常完成。三平台Rust回归不代证实际Ollama daemon/工具执行或iOS，恢复从下一步deferred/Lite和Runtime正例继续。

- 当前恢复点：with_deferred_tool_search启用custom/v2并绑定policy；完整catalog先验parameters对象/strict/alias，再隐藏root defer=true成员，client结果后开放，原生忽略的flag不透传。重复声明只允许defer加载标记变化，其余kind/schema保持，原始root/results分别绑定。4项deferred+1项codec新增共Ollama63；deferred旧400及Runtime补reasoning.content=null旧400均effective RED→GREEN，只允许synthetic reasoning carrier缺省/null等价，数组/其他item与payload仍拒绝，临时诊断已移除。固定Runtime新增1项Classic MCP发现/执行/落盘重启，执行端显式禁用web，4次真实POST/Key，按client搜索实际返回的namespace/function调用，raw native items/落盘response与原始结果保持，重启MCP echo不重跑。最终workspace375/0/41、固定Runtime39/0/0、Clippy全targets-D warnings/fmt/Python compile/diff通过，日志/tmp/caidex-ollama-deferred-{red,green,canonical-red,canonical-green,real-first,real-green,workspace-final,all-real-final,clippy-final}.log；首次Runtime失败仅为null差异，不计通过。接下来diff→源码提交/push→精确head三平台CI→三文档收尾；Lite尚未改、实际daemon/Full未验，无新依赖。
