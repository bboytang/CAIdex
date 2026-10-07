# CAIdex 项目交接

更新：2026-10-07。先读本文件，再检查 Git/AGENTS.md；沿 V3 恢复点继续，不重新规划已确认架构。

## 当前任务与恢复点

F/G：Gemini结构输出转换已完成本地、唯一审查的一次Important修复及精确源码三平台验收（源码a097b16322f9e680119c4d3d4a656ecaebee42ab，CI37690749429）。继续其余Runtime参数转换，再六方法/Registry/Gateway/固定Runtime；不重复已验阶段、不改V3架构。完整F/G未验收，H–R尚未实施。

完整项目/文档公开、提交/push/CI及离线合成fixture已获授权，不重复询问。不读取用户模型Key，不调用商业API；合成fixture不授予商业Full。

## 已完成

- A–E：V3/UI/品牌原件、固定上游、Rust workspace/CI、真实app-server facade，审批/Queue/Steer/Goal/MCP/PTY离线回归；执行端Broker、Windows keyring/Linux Secret Service/受保护文件/env、脱敏及CLI凭据入口。
- F：经典/Lite canonical wire、SSE/opaque/工具原文、loopback Gateway独立token/取消/背压/TLS/安全错误、ModelProvider六方法及Registry证据门槛；Custom/OpenAI当前离线范围已三平台验收。
- Anthropic：原生Messages/Models、工具/图片/推理/结构输出/Runtime参数、增量SSE、六方法；v3编译前缀/认证组织绑定、v4动态发现/原始输出绑定；经典显式禁用网页后实际MCP发现/执行/重启恢复、Lite审批Code Mode执行/取消已三平台验收。默认经典cached web_search明确拒绝且Key/POST为0，非Full。
- 共享tool_search专用call/output契约已验：原tools/arguments、client call_id和完整终态保留，由Runtime执行。
- Gemini：Models分页、generateContent JSON、原生SSE、真实streamGenerateContent HTTP、共享slot/安全错误/TLS/取消/Drop/背压/正常EOF已三平台验收。新history.rs保留完整native response/request、所有SSE chunks、Part/thoughtSignature/未知字段/大数；模型/原请求归属及完整展示组校验，所选候选Responses输出、精确usage及JSON/SSE原文下一POST回放已三平台验收。新ToolMap保存原始function/custom/namespace，稳定alias及schema精确编解码；显式v2历史与实际native tools逐值绑定，v1不自动升级。 请求编译基础已验：经典/Lite、初始system/messages、完整组/实际前缀绑定、签名Part/缺省role、function/custom配对和完整结果组按native调用顺序输出；source及结果内数组原序保留。结构输出已验：显式格式/tool组合能力、native responseFormat、保留schema与strict子集拒绝；本地URI片段一次解码/语法/循环门控及签名前缀绑定。推理已验：显式budget/level映射、summary显示/context本地声明及完整请求绑定；JSON/SSE摘要/签名/媒体三轮回放保持。图片/工具结果媒体已验：执行端独立MIME能力、用户inlineData及显式per-Part detail、工具FunctionResponse.parts/displayName单次引用；有界data URL、媒体/文本顺序及JSON/SSE/落盘三轮回放。

## 未完成与下一步顺序

1. Gemini其余Runtime参数：先核对固定Runtime实际出站字段与Anthropic已有模式，处理verbosity及client_metadata/prompt_cache_key/include/stream_options/service_tier/access_programs等；metadata只作执行端明确保留的本地归属，不能冒充原生cache/用户ID/SLA。verbosity若用提示需显式意图映射并进入原prefix，缺原生等价约束仍unsupported；先行为RED→GREEN，再实际HTTP/唯一审查/新三平台CI。
2. 参数/历史与接线门控：完整v1/v2原native request绑定含maxOutputTokens/toolConfig/thinkingConfig/responseFormat，参数改变会明确拒绝旧历史，不自动松绑。parallel_tool_calls=false且启用工具当前明确unsupported，原生无单调用限制，固定Lite默认false须在Runtime接线前找等价保证或保持不支持，不能用提示词冒充硬约束；后置system/developer、strict/defer_loading/tool_search/web_search仍unsupported，grammar仅指导。
3. 接六方法/Registry/Gateway和固定Runtime，经典/Lite、审批/工具/取消/持久恢复分别离线验证；每阶段新源码三平台CI，不用旧证据代验。
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
- Anthropic cached网页/grammar硬约束仍无等价证据；重启第三轮回复精确比较、Lite落盘custom result完整比较两项覆盖Minor暂缓，详见Anthropic文档。
- 未配置真实模型Key；商业推理/签名真实性/Full、生产Host权限、客户端UI/iOS/真机/签名未验。macOS Rust CI不是iOS应用构建。
- .git普通沙箱只读，提交/push用require_escalated。gh为bboytang；旧helper缺workflow scope，发布用git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main，不改全局或输出凭据。
- 普通沙箱禁止loopback；/tmp/.git触发凭据Git拒绝保护。完整测试用获授权执行环境及TMPDIR=/var/tmp，不削弱保护。本机无Windows/Xcode/gnome-keyring-daemon，对应检查用GitHub。
- 同步SecretStore读取开始后不可强停，但取消后不POST；无默认Key探测/fallback或自动重试。

## 文件与 Git 状态

- branch main跟踪origin/main，已验结构输出源码a097b16322f9e680119c4d3d4a656ecaebee42ab已公开提交/push且CI通过；收尾HANDOFF、Gemini及Model-Gateway文档独立提交（当前HEAD见git log），已同步，工作区干净，无遗留未提交源码/文档修改。
- 执行基准：docs/CAIdex-实施计划-V3.md、CAIdex-UI-规范-V1.md；原V2仅背景，冲突以V3为准。详细历史证据留Runtime-能力对照及各Provider/Credentials/Model-Gateway验收文档。
- 当前结构输出日志：/tmp/caidex-google-structured-{red,green,ref-red,ref-green,http,workspace,clippy}.log；唯一审查/tmp/caidex-google-structured-review.md及独立复现/tmp/caidex-google-ref-review-repro.log。跨机器以提交文档/GitHub为准。
- 当前相关：model/providers/google/src/{structured,reasoning,media,request,history,tools,client,content,stream,transfer}.rs、tests/{structured,reasoning,media,request,http,history,tools}.rs；下一步参考Anthropic request/reasoning/structured/runtime_parameters及model/core，不复制HTTP栈。
- 代码边界：credentials/core、apps/cli、model/core/providers/gateway、runtime/bridge、upstream/codex、.github/workflows/ci.yml、scripts；assets/brand四份原件未改。

## 最近验证

- 结构输出6项有效RED→GREEN；唯一独立审查发现1项Important：URI片段未解码/校验语法会导致循环漏检、合法引用拒绝、非法编码/tilde被当字面名。共享路径一次修复，第7项回归独立RED→GREEN，structured7/request8/reasoning5通过；无Critical/新增Minor，不重复派审。18项排除裁定及成本见Gemini文档，旧Minor未改。
- 既有HTTP1扩大至budget/level、经典/Lite各三POST：JSON/SSE、schema/推理、摘要/签名/媒体/工具关联及落盘保持；改schema/不支持pattern在新增Key读取前编译拒绝。HTTP无独立实现前RED。修复后完整workspace294passed/0failed/32ignored、Google72个名字各通过一次、Clippy -D warnings/fmt/diff通过。
- 精确源码a097b16322f9e680119c4d3d4a656ecaebee42ab的[CI37690749429](https://github.com/bboytang/CAIdex/actions/runs/37690749429)三平台completed/success，head及全部步骤核对。Google catalog4/content3/history6/HTTP23/request8/media6/reasoning5/stream4/structured7/tools6共72个名字逐平台各通过一次；workspace Linux294/Windows289/macOS293，0失败，ignored32/30/30；真实Runtime Linux30/WindowsmacOS29，0失败/0ignored；fmt/Clippy/native credentials/schema/doctor通过。macOS容量注释不影响成功；Rust CI不是iOS应用验收。
- /tmp/caidex-ci-37690749429-status.json及{-watch,-linux,-windows,-macos}.log；逐项核验沿/tmp/caidex-google-request-ci-check.py。所有回复/Key/字节合成，没有商业Full、新增实际Gemini Runtime接线或生成值/业务内容求值证据。
