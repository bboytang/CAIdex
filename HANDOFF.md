# CAIdex 项目交接

更新：2026-10-08。每次先读本文件、AGENTS.md及Git状态；按V3续接，不重规划架构。详细历史证据留docs各阶段验收文档。

## 当前任务

F/G：Ollama native history当前协议范围已三平台验收，源码ad3a49176b4dc6044454d800a09b4c7131dc9d86已push，[CI37728309340](https://github.com/bboytang/CAIdex/actions/runs/37728309340)精确head全部job/step终态及逐名日志已核对。本次仅阶段收尾文档；下一步媒体/结构/其余工具及固定Runtime。不是实际Ollama Runtime/daemon/商业Full，完整F/G及H–R未完成。

## 已完成

- A–E当前范围：V3/UI/品牌/固定上游、Rust workspace/三平台CI、真实app-server facade、审批/Queue/Steer/Goal/MCP/PTY离线回归；执行端Broker、Windows keyring/Linux Secret Service/受保护文件/env、脱敏与CLI凭据入口。
- F/G：经典/Lite canonical wire、ModelProvider六方法/Registry证据门槛、独立Custom Responses、OpenAI原生；loopback Gateway独立token、取消/背压/TLS/安全错误已三平台验。
- Anthropic：原生Models/Messages、JSON/SSE/参数/工具/媒体/推理/结构输出、v3前缀与组织绑定、v4动态发现；经典显式禁用web后MCP发现/执行/重启、Lite Code Mode审批执行/取消已三平台离线验。默认cached web拒绝，非Full。
- Gemini：Models、native JSON/SSE/HTTP、完整Part/签名/未知字段/大数/v1v2载体、工具映射、请求/媒体/推理/结构/参数、六方法/Profile/Registry/Gateway已三平台验。本轮真实Runtime新增7测试：默认拒绝4路径、经典/Lite各3轮签名含磁盘重启、Lite批准后实际marker/custom结果原文回放且重启不重复、经典静态MCP结果/重启零重跑、Lite双调用拒绝无审批/执行/载体、两路径interrupt关闭native socket。
- 本轮context显式opt-in允许3头只留本地；Lite单调用策略显式opt-in在正常EOF交付前校验，超限整轮失败，不承诺Google生成控制。共享Gateway本地流错误改response.failed/response.error，固定Runtime显示安全code，HTTP错误格式不变，无重试。

- Ollama：六方法/原生目录与配置交集、独立无认证或正确ollama Bearer、经典文本/function历史、typed JSON/SSE/失败终态/Gateway隔离/取消/Drop/slot已有10项。新增6项show/thinking：Custom bounded POST、显式同origin端点、原生声明精度/Unknown/绑定snapshot、JSON/SSE exact等级与bool/default/Unsupported/边界，16项三平台离线通过。native history显式opt-in新增10项（共26）：v1 profile/model/compiled prefix/display group绑定、JSON3轮/SSE2轮、文本进度/延迟工具及坏终态/取消；三平台通过。未支持控制仍在Key/POST前拒绝，复用Custom transport/OpenAI Models parser。

## 下一步顺序

1. 接Ollama媒体/结构/其余工具路径及固定Runtime（包括item表示/本地headers/developer/include/summary）。Custom在typed terminal后停止读取native，不保证HTTP clean EOF；不冒用Google EOF承诺。caller reasoning仅过codec，未映射native output保留但下轮显式拒绝；不要重做已验history/show/Gemini。
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

- Ollama尚无真实daemon/模型或本Adapter固定Runtime证据；show/精确think已有三平台证据，native history显式opt-in三平台已验；developer/context/Lite及其他高级控制明确拒绝。snapshot绑定模型ID，非来源认证/version锁/实时刷新；native vision声明不是Adapter媒体支持，详docs/Ollama。README旧Anthropic简介滞后、Google未列，以HANDOFF/各验收文档为准，不据旧简介重做已完成阶段。

- Gemini实际Runtime阶段唯一独立审查无Critical/Important，1覆盖Minor暂缓：thought-call豁免、无tools、未opt-in none缺直接专项。14排除项裁定/成本留Gemini文档；不派复审。
- 旧Minor：prefix-only v1v2完整组互换、非空projection pending取消/Drop、满槽取消/Drop、ProtoJSON替代整数/空ID表示、部分usage下界矛盾、thought-only phase；Anthropic重启第三轮/完整Lite custom结果覆盖等，详Provider文档，未认领修复。
- comment-only native chunk刷新Provider idle但不转发；实际Runtime下游idle单独未验。同步SecretStore读开始后不可强停，只保证取消后不POST。商业Key/签名真实性/Full、生产Host权限、UI/iOS/真机/签名均未验；Rust macOS CI不是iOS。
- 上次git add因额度用尽导致自动审批审核无法完成而未执行，本轮已成功提交/push/CI，阻塞解除。完整项目/文档公开及离线fixture已授权，不重复询问；不读用户模型Key/调用商业API。
- .git普通沙箱只读，提交/push require_escalated；gh bboytang，push用 git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main，不改全局/输出凭据。普通沙箱loopback受限；完整测试用授权环境+TMPDIR=/var/tmp，保留/tmp/.git保护。本机缺Windows/Xcode/gnome-keyring-daemon，对应GitHub验证。

## Git / 文件 / 验证

- branch main跟踪origin/main，源码ad3a491已push；本次收尾提交仅HANDOFF/Ollama/Model-Gateway文档，无未完成源码。futures-util仅从dev移到原有版本的dependency，Cargo.lock未变；精确CI已通过，结束前核对收尾提交/push及实际Git状态。
- 基准：docs/CAIdex-实施计划-V3.md、CAIdex-UI-规范-V1.md，原V2只作需求背景。相关代码model/providers/ollama、model/providers/custom/src/catalog.rs、model/providers/google、model/gateway、model/core、runtime/bridge/tests/{real_runtime.rs,fixtures}；其他模块credentials/core、apps/cli、upstream/codex、.github/workflows/ci.yml、scripts。
- 本轮本地Ollama26/0/0、workspace338/0/39、旧Runtime37/0/0、Clippy -D warnings/fmt/diff通过；SSE保护有效RED→GREEN。日志/tmp/caidex-ollama-history-{codec,json,stream-red,stream-green,provider-final,workspace-final,all-real-final,clippy-final}.log。
- [CI37728309340](https://github.com/bboytang/CAIdex/actions/runs/37728309340)：headad3a491、3job/所有step终态均核对；Ollama26/OpenAI11/Custom7/Google90及旧实际Runtime每名逐平台各一次。workspace Linux338/Windows333/macOS337，0失败、ignored39/37/37；旧Runtime37/36/36，0失败/0ignored；Linux native凭据1及fmt/Clippy/schema/doctor通过，不是Ollama新Runtime证据。
- CI/tmp/caidex-ci-37728309340-{status.json,watch.log,linux.log,windows.log,macos.log}，逐名脚本/tmp/caidex-ollama-history-ci-check.py exit0；macOS CLI缓存protocol截断，已直接下载完整job raw，替换对应protocol段重验，保留macos-{raw,cli-truncated}.log。watch/三日志/原始日志下载均exit0；跨机器以已提交文档/CI为准，/tmp不保证保留。
- 环境磁盘满已解除：只cargo clean本项目可再生target缓存释放16.9GiB；受截断新测试已恢复并完整复验，10项名字核对，中断运行不计通过。当前不再阻塞；下轮先查df，不累积旧构建缓存。
