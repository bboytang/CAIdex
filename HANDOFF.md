# CAIdex 项目交接

更新：2026-10-08。先读本文件，再检查 Git/AGENTS.md；沿 V3 恢复点继续，不重新规划已确认架构。

## 当前任务与恢复点

F/G：固定Gemini Runtime接线已实现，基线main f43b5f7，尚未提交/新CI；唯一审查已完成无Critical/Important，1覆盖Minor暂缓（thought豁免/无tools/未opt-in none）。经典/Lite三轮签名历史含实际app-server重启、Lite审批Code Mode marker执行/原样result及重启不重复、经典静态MCP调用/结果及重启不重复、classic/Lite interrupt socket关闭均已有本地通过；双调用负例暴露共享Gateway SSE错误被固定Runtime忽略，已最小response.failed修复并有效RED→GREEN。本地完整workspace312passed/0failed/39ignored、完整实际Runtime37passed/0failed/0ignored与Clippy/fmt/diff已过。最终复验已过；唯一审查已结束，裁定14项及成本在Gemini文档；正在续接提交/push/精确源码三平台CI。上次git add因额度用尽导致自动审批审核无法完成而未执行，并非不安全裁定；本轮重试已授权操作。保持V3；商业Full/F/G全部与H–R未完成。

完整项目/文档公开、提交/push/CI及离线合成fixture已获授权，不重复询问。不读取用户模型Key，不调用商业API；合成fixture不授予商业Full。

## 已完成

- A–E：V3/UI/品牌原件、固定上游、Rust workspace/CI、真实app-server facade，审批/Queue/Steer/Goal/MCP/PTY离线回归；执行端Broker、Windows keyring/Linux Secret Service/受保护文件/env、脱敏及CLI凭据入口。
- F：经典/Lite canonical wire、SSE/opaque/工具原文、loopback Gateway独立token/取消/背压/TLS/安全错误、ModelProvider六方法及Registry证据门槛；Custom/OpenAI当前离线范围已三平台验收。
- Anthropic：原生Messages/Models、工具/图片/推理/结构输出/Runtime参数、增量SSE、六方法；v3编译前缀/认证组织绑定、v4动态发现/原始输出绑定；经典显式禁用网页后实际MCP发现/执行/重启恢复、Lite审批Code Mode执行/取消已三平台验收。默认经典cached web_search明确拒绝且Key/POST为0，非Full。
- 共享tool_search专用call/output契约已验：原tools/arguments、client call_id和完整终态保留，由Runtime执行。
- Gemini：Models分页、generateContent JSON、原生SSE、真实streamGenerateContent HTTP、共享slot/安全错误/TLS/取消/Drop/背压/正常EOF已三平台验收。新history.rs保留完整native response/request、所有SSE chunks、Part/thoughtSignature/未知字段/大数；模型/原请求归属及完整展示组校验，所选候选Responses输出、精确usage及JSON/SSE原文下一POST回放已三平台验收。新ToolMap保存原始function/custom/namespace，稳定alias及schema精确编解码；显式v2历史与实际native tools逐值绑定，v1不自动升级。 请求编译基础已验：经典/Lite、初始system/messages、完整组/实际前缀绑定、签名Part/缺省role、function/custom配对和完整结果组按native调用顺序输出；source及结果内数组原序保留。结构输出已验：显式格式/tool组合能力、native responseFormat、保留schema与strict子集拒绝；本地URI片段一次解码/语法/循环门控及签名前缀绑定。其余Runtime参数已验：metadata/cache hint显式本地保留，include唯一性/null门控、delivery/access不等价拒绝；verbosity显式提示位于初始system之后，serviceTier显式原生类映射，两者进入完整v1/v2前缀后再回放。推理已验：显式budget/level映射、summary显示/context本地声明及完整请求绑定；JSON/SSE摘要/签名/媒体三轮回放保持。图片/工具结果媒体已验：执行端独立MIME能力、用户inlineData及显式per-Part detail、工具FunctionResponse.parts/displayName单次引用；有界data URL、媒体/文本顺序及JSON/SSE/落盘三轮回放。

## 未完成与下一步顺序

1. 提交/push当前源码与文档，启动新精确SHA CI；核对三个job/所有step终态、Google90及实际Google7测试逐名各一次、全部Runtime与workspace。唯一审查已完成无Critical/Important，1覆盖Minor暂缓；14排除项逐项裁定/成本留Gemini文档，不派复审。最终本地workspace312/0/39、实际Runtime37/0/0、Clippy/fmt/diff已验。Google新context/单调用2回归与实际Gemini7测试要逐名核对，不能以局部绿代完整套件。
2. 已验范围使用固定公开model_catalog_json配置复用gpt-5.5/gpt-6.1-sol模板（来源已精确固定commit核对；仅alias/展示描述/supports_search_tool=false），web_search=disabled。高级默认经典/Lite、basic经典web、basic Lite默认parallelfalse仍Key/POST前拒绝，不使用未知模型fallback/过滤Gateway工具。
3. 新context opt-in3头只留本地，turn-state拒绝；单调用交付策略opt-in与0调用限制，无Google生成硬承诺/自动重试。共享Gateway本地流错误改response.failed/response.error，固定Runtime才显示safe error code；经典/Lite工具/恢复/取消验收不是商业Full，发现/网页/strict/deferred/后置system仍unsupported。完成唯一审查、裁定与精确源码三平台CI后补文档收尾。Gateway绝对deadline/native网络chunk刷新idle，实际Runtime comment-only idle独立待验。
4. Gemini → 兼容API/Ollama → V3 H/I → Windows → SSH/iOS → CLI → Relay → R。iOS simulator/无签名archive在GitHub macOS；真机/UAC/签名/逐模型商业报告独立验收。

## 重要架构决定

- Windows11x64 Tauri2 + React/TypeScript/Rust；Linuxx86_64 Host/CLI；iOS17+/Swift5 SwiftUI + UniFFI。GUI中文可切英文，CLI英文。只用assets/brand用户原件，UI尽量1:1官方参考，模型/Key放设置。
- 固定真实Codex0.160.1，commit`d27764b82f7118f674371e6d6e76271d9d606edb`；一个Runtime，不造第二套Agent。JSONL facade保留未知方法/字段，实验opt-in；经典与Lite/Code Mode分别验收。
- Gateway只做Responses HTTP/SSE适配，不执行工具；Runtime负责发现/审批/沙箱/执行。关闭request/stream重试，loopback token只注入隔离子进程；HTTPS校验不可削弱。
- reasoning.encrypted_content在CAIdex只是版本化敏感JSON载体，不是加密/认证。native wire为回放权威；Anthropic v3绑定实际system/tools/messages/认证组织，v4保存初始声明及原始discoveries；旧v1–v3不自动升级。Gemini v1绑定model/原native request，v2另存canonical声明并核对实际native tools，保存raw chunks并重建验证派生response；结构校验不认证伪造整组或签名真实性。生产历史权限留H/I。
- Anthropic动态发现需要执行端inline-tools-2026-09-15 beta、模型discovery/system支持及预期组织；初始tools不变，新增定义按位置追加，不重写signed前缀。Unknown≠Supported，目录/名称/fixture不授予Full/Compatible，须LiveRuntime报告。
- Chat独立无Shell/Git/项目写权限；Remote使用执行Host Key，手机不读已存Host Key，同步不含凭据。模型切换轮次边界生效，跨Provider关联分支/新线程。
- Host独立后台，SQLite journal先落盘再广播、快照补缺口、请求幂等、审批首次有效。GUI/SSH退出不结束任务，不盲重跑未知结果，不承诺外部操作恰好一次。
- Remote先SSH后Noise/Snow Relay；Host切换不迁移活动线程。Chat自托管HTTPS同步、SQLite/outbox/cursor/冲突分支/tombstones；服务端为信任边界，不宣称历史E2EE。无签名archive不等于可安装IPA。

## 问题 / 环境 / 暂缓项

- Gemini工具映射审查无Critical/Important，1项覆盖Minor暂缓：完整有效v1/v2组的prefix-only互换尚无专项回归，现有显式版本核对正确，不能夸大覆盖。上轮history2项代码Minor暂缓：缺prompt的部分usage已知下界与total矛盾仍返回usage=null；只有thought函数调用的STOP可能令可见text phase为commentary。旧满槽取消/Drop直接专项覆盖、ProtoJSON替代整数表示等Minor详见Gemini文档；重复/部分usage更新专项已由新history回归补齐，不重复列为未补。
- 上阶段覆盖Minor：wrapper取消回归在text delta后队列已空，非空投影pending队列取消/Drop尚无专项；原生连接/slot释放与无terminal/history已验，不夸大立即丢弃已缓冲进度。
- Anthropic cached网页/grammar硬约束仍无等价证据；重启第三轮回复精确比较、Lite落盘custom result完整比较两项覆盖Minor暂缓，详见Anthropic文档。
- 未配置真实模型Key；商业推理/签名真实性/Full、生产Host权限、客户端UI/iOS/真机/签名未验。macOS Rust CI不是iOS应用构建。
- .git普通沙箱只读，提交/push用require_escalated。gh为bboytang；旧helper缺workflow scope，发布用git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main，不改全局或输出凭据。
- 普通沙箱禁止loopback；/tmp/.git触发凭据Git拒绝保护。完整测试用获授权执行环境及TMPDIR=/var/tmp，不削弱保护。本机无Windows/Xcode/gnome-keyring-daemon，对应检查用GitHub。
- 同步SecretStore读取开始后不可强停，但取消后不POST；无默认Key探测/fallback或自动重试。

## 文件与 Git 状态

- branch main跟踪origin/main，HEAD f43b5f7；本轮未提交：Cargo.lock、runtime/bridge/Cargo.toml及Google actual harness/HTTP fixture、新google_model_catalog.json、Google client/provider/request/response_stream和http/provider.rs门控及2回归、共享Gateway transfer/http错误事件修复、HANDOFF/Gemini/Runtime/Gateway文档。无本轮提交/CI。
- 执行基准：docs/CAIdex-实施计划-V3.md、CAIdex-UI-规范-V1.md；原V2仅背景，冲突以V3为准。详细历史证据留Runtime-能力对照及各Provider/Credentials/Model-Gateway验收文档。
- 上阶段日志/tmp/caidex-google-projection-{red,green}.log、/tmp/caidex-google-provider-{red,green,boundaries-red,boundaries-green,workspace,clippy,projection-final}.log；唯一审查/tmp/caidex-google-provider-review.md。CI status/watch/linux/windows/macos日志/tmp/caidex-ci-37696809902-*，逐名脚本/tmp/caidex-google-request-ci-check.py已递归识别http/provider.rs前缀。跨机器以提交文档/GitHub为准。
- 首次边界编译因磁盘满失败，不是有效RED；仅清理本项目target/debug/incremental约6.3G后重跑复现。未删源码/用户文件、未改保护。当前本机codex-cli0.160.1可用；本轮历史正例已有实际RED日志/tmp/caidex-google-runtime-history-red.log。
- 当前相关：model/providers/google/src/{provider,response_stream,runtime_parameters,structured,reasoning,media,request,history,tools,client,content,stream,transfer}.rs、tests/{http/provider.rs,response_stream,runtime_parameters,structured,reasoning,media,request,http,history,tools}.rs；下一步参考Anthropic provider/projection/response_stream、model/core/provider.rs及registry、model/gateway，不复制传输栈。
- 代码边界：credentials/core、apps/cli、model/core/providers/gateway、runtime/bridge、upstream/codex、.github/workflows/ci.yml、scripts；assets/brand四份原件未改。

## 最近验证

- 当前本轮：context1（有效RED→GREEN，/tmp/caidex-google-runtime-context-{red,green}.log）；单调用1含JSON/SSE×4场景（有效RED→GREEN，/tmp/caidex-google-runtime-cardinality-{red,green}.log）。实际默认拒绝1测试/4模式（/tmp/caidex-google-runtime-defaults.log）、经典两轮签名/磁盘载体1（/tmp/caidex-google-runtime-history-green.log）均exit0。此前默认模式错误期望及includeThoughts缺省none是fixture修正，不算生产RED；真正context RED见history-profile-red.log。后续新增Lite历史有效RED parallelfalse→显式profile GREEN。经典/Lite三轮+重启2测试通过history-restart-green.log；工具集成首次4绿/2红（Lite重复读已删除marker的测试错误已修），Lite执行精确结果/恢复已通过tools-check.log；双调用实际safe错误RED→GREEN multi-green.log，共享Gateway6坏流场景有效RED→GREEN gateway-error-{red,green}.log。MCP静态工具/恢复与两模式interrupt已局部绿。本轮完整workspace312passed/0failed/39ignored（普通workspace不执行37项ignored实际Runtime/native服务）；另完整实际Runtime37passed/0failed/0ignored，新Google7名字各一次；Clippy -D warnings/fmt/diff通过。移除无用fixture模式后最终workspace-final/all-real-final日志同数通过；唯一审查已完成无Critical/Important，1覆盖Minor暂缓（thought调用豁免/无tools/未opt-in none）；14排除项/成本见Gemini文档。新CI尚未运行。
- 上阶段源码530ead5535b6bc6cd8896317b7360c7872a98571/[CI37696809902](https://github.com/bboytang/CAIdex/actions/runs/37696809902)三平台completed/success：Google88逐名一次；workspace Linux310/Windows305/macOS309，0失败，ignored32/30/30；旧实际Runtime Linux30/其他29。唯一审查无Critical/Important，pending队列覆盖Minor暂缓，裁定详Gemini文档。这些不是新增Gemini Runtime/商业Full/iOS证据。
