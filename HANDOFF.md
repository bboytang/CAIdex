# CAIdex 项目交接

更新：2026-10-08。每次先读本文件、AGENTS.md及Git状态；按V3续接，不重规划架构。详细历史证据留docs各阶段验收文档。

## 当前任务

F/G：Ollama `/api/show` 与精确thinking控制本地实现/验收完成，准备提交/push/核对精确源码三平台CI，当前基准main/1231663。Custom bounded POST、显式同origin端点、模型绑定snapshot，effort仅转声明中的精确think值；有效RED→GREEN，Ollama16、workspace328/0/39、固定Runtime37/0/0、Clippy/fmt/diff通过。测试入口误识别已移tests/models/mod.rs并完整重验；首阶段CI不能代证本轮。完整F/G及H–R未完成。

## 已完成

- A–E当前范围：V3/UI/品牌/固定上游、Rust workspace/三平台CI、真实app-server facade、审批/Queue/Steer/Goal/MCP/PTY离线回归；执行端Broker、Windows keyring/Linux Secret Service/受保护文件/env、脱敏与CLI凭据入口。
- F/G：经典/Lite canonical wire、ModelProvider六方法/Registry证据门槛、独立Custom Responses、OpenAI原生；loopback Gateway独立token、取消/背压/TLS/安全错误已三平台验。
- Anthropic：原生Models/Messages、JSON/SSE/参数/工具/媒体/推理/结构输出、v3前缀与组织绑定、v4动态发现；经典显式禁用web后MCP发现/执行/重启、Lite Code Mode审批执行/取消已三平台离线验。默认cached web拒绝，非Full。
- Gemini：Models、native JSON/SSE/HTTP、完整Part/签名/未知字段/大数/v1v2载体、工具映射、请求/媒体/推理/结构/参数、六方法/Profile/Registry/Gateway已三平台验。本轮真实Runtime新增7测试：默认拒绝4路径、经典/Lite各3轮签名含磁盘重启、Lite批准后实际marker/custom结果原文回放且重启不重复、经典静态MCP结果/重启零重跑、Lite双调用拒绝无审批/执行/载体、两路径interrupt关闭native socket。
- 本轮context显式opt-in允许3头只留本地；Lite单调用策略显式opt-in在正常EOF交付前校验，超限整轮失败，不承诺Google生成控制。共享Gateway本地流错误改response.failed/response.error，固定Runtime显示安全code，HTTP错误格式不变，无重试。

- Ollama：六方法/原生目录与配置交集、独立无认证或正确ollama Bearer、经典文本/function历史、typed JSON/SSE/失败终态/Gateway隔离、本地取消/Drop/slot已有10项三平台离线通过；未支持控制在Key/POST前拒绝。OpenAI Models parser移到Custom共用，不改原校验或公开类型。

## 下一步顺序

1. 完成本轮show/thinking测试与回归，再提交/push核对新源码三平台CI。元数据固定snapshot需native model匹配配置alias，不自动fetch/放松Unsupported；未声明think控制拒绝，none仅在声明false时禁用，Boolean不冒充high。官方show/thinking文档及固定e3cddc3e源码已核对；不派Gemini复审。
2. 再接Ollama native历史归属/回放、媒体/结构/工具路径及固定Runtime。随后DeepSeek/Qwen/OpenRouter等兼容API；DeepSeek Responses developer按user、未知input忽略，其models不保证created，不能直接透传/照搬严格catalog。等价路径复用transport，差异明确编译/拒绝；不据URL/目录/fixture授予Lite/Full，不下载大模型/调用商业API。
3. 兼容API/Ollama完成当前范围后，V3 H/I → Windows → SSH/iOS → CLI → Relay → R。iOS simulator/无签名archive在GitHub macOS；真机/UAC/签名/逐模型商业报告独立验。

## 重要架构决定

- Windows11x64 Tauri2+React/TypeScript/Rust；Linuxx86_64 Host/CLI；iOS17+/Swift5 SwiftUI+UniFFI。GUI中文/英文，CLI英文；只用assets/brand原件，UI尽量1:1官方参考，模型/Key放设置。
- 固定真实Codex0.160.1/d27764b82f7118f674371e6d6e76271d9d606edb，一个Runtime，不造第二Agent/HTTP栈。Gateway只适配Responses，不执行工具；Runtime负责发现/审批/沙箱/执行。未知协议字段保留、实验opt-in、经典/Lite分别验收。
- Native wire是回放权威；Gemini v1完整model/request绑定，v2另绑定canonical工具及actual native tools，raw chunks重建核对。Anthropic v3绑定compiled前缀/认证组织，v4保存发现记录。签名/未知字段不改写、不自动升级/松绑；Anthropic/Gemini自建encrypted_content只是敏感JSON载体，Ollama原生此字段是明文thinking且当前拒绝回放，均不宣称加密/密码学认证，生产访问控制留H/I。
- Gemini正例公共model_catalog_json复用精确固定gpt-5.5/gpt-6.1-sol模板，仅alias/描述/display_name/supports_search_tool=false；显式禁用web。高级默认与未opt-in Lite负例继续Key/POST前拒绝。工具搜索/web/strict/deferred/后置system未支持；grammar仅指导。local数量限制不追溯伪改旧signed历史。
- Unknown≠Supported，Configured/ProviderCatalog/ProtocolFixture不授予Full/Compatible，须LiveRuntime报告。
- Chat独立无Shell/Git/项目写权限；Remote用执行Host Key，手机不读已存Host Key，同步不含凭据。模型轮次边界切换，跨Provider关联分支/新线程。
- Host后台、SQLite journal先落盘再广播、快照补缺口/请求幂等/审批首次有效，不盲重跑未知结果、不承诺外部exactly-once。Remote先SSH后Noise/Snow Relay；活动线程不迁移Host。Chat自托管HTTPS+SQLite/outbox/cursor/冲突/tombstones，服务端为信任边界，无历史E2EE承诺；unsigned archive不是可安装IPA。

## 问题 / 暂缓项

- Ollama尚无真实daemon/模型或本Adapter固定Runtime证据；show/精确think控制新增本地已验，reasoning历史/developer/context/Lite及其他高级控制均明确拒绝。snapshot绑定模型ID，非来源认证/version锁/实时刷新；native vision声明不是Adapter媒体支持，详docs/Ollama。README旧Anthropic简介滞后、Google未列，以HANDOFF/各验收文档为准，不据旧简介重做已完成阶段。

- Gemini实际Runtime阶段唯一独立审查无Critical/Important，1覆盖Minor暂缓：thought-call豁免、无tools、未opt-in none缺直接专项。14排除项裁定/成本留Gemini文档；不派复审。
- 旧Minor：prefix-only v1v2完整组互换、非空projection pending取消/Drop、满槽取消/Drop、ProtoJSON替代整数/空ID表示、部分usage下界矛盾、thought-only phase；Anthropic重启第三轮/完整Lite custom结果覆盖等，详Provider文档，未认领修复。
- comment-only native chunk刷新Provider idle但不转发；实际Runtime下游idle单独未验。同步SecretStore读开始后不可强停，只保证取消后不POST。商业Key/签名真实性/Full、生产Host权限、UI/iOS/真机/签名均未验；Rust macOS CI不是iOS。
- 上次git add因额度用尽导致自动审批审核无法完成而未执行，本轮已成功提交/push/CI，阻塞解除。完整项目/文档公开及离线fixture已授权，不重复询问；不读用户模型Key/调用商业API。
- .git普通沙箱只读，提交/push require_escalated；gh bboytang，push用 git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main，不改全局/输出凭据。普通沙箱loopback受限；完整测试用授权环境+TMPDIR=/var/tmp，保留/tmp/.git保护。本机缺Windows/Xcode/gnome-keyring-daemon，对应GitHub验证。

## Git / 文件 / 验证

- branch main跟踪origin/main，HEAD1231663已同步；本轮未提交Custom lib bounded POST、Ollama config/lib/request、新增src/models.rs与tests/models/mod.rs、provider测试模块接线、Ollama验收文档及本交接文档。未完成项见当前任务；恢复先查实际Git状态。
- 基准：docs/CAIdex-实施计划-V3.md、CAIdex-UI-规范-V1.md，原V2只作需求背景。相关代码model/providers/ollama、model/providers/custom/src/catalog.rs、model/providers/google、model/gateway、model/core、runtime/bridge/tests/{real_runtime.rs,fixtures}；其他模块credentials/core、apps/cli、upstream/codex、.github/workflows/ci.yml、scripts。
- 当前本轮本地：Ollama16/0/0、workspace328/0/39、实际Runtime37/0/0、Clippy -D warnings/fmt/diff通过；精确thinking有有效RED→GREEN，旧guard与后续轮次call_id复用证据保留。日志/tmp/caidex-ollama-show-{provider-final,workspace-final,all-real-final,clippy-final}.log，旧thinking-{red,green}.log；新三平台CI尚待。
- [CI37719887330](https://github.com/bboytang/CAIdex/actions/runs/37719887330)：head95b0f6b、3job及所有step终态均核对；Ollama10/OpenAI11/Custom7/Google90及实际Runtime Linux37/其他36每个名字各一次。workspace Linux322/Windows317/macOS321，0失败、ignored39/37/37；实际Runtime37/36/36，0失败/0ignored；Linux native凭据1、fmt/Clippy/schema/doctor通过。watch、三日志下载、逐名脚本exit0。
- 日志/tmp/caidex-ollama-{provider-final,workspace-final,all-real-final,clippy-final}.log；CI日志/tmp/caidex-ci-37719887330-{status.json,watch.log,linux.log,windows.log,macos.log}，逐名脚本/tmp/caidex-ollama-ci-check.py。跨机器以已提交文档/CI为准，/tmp不保证保留。
- 环境磁盘满已解除：只cargo clean本项目可再生target缓存释放16.9GiB；受截断新测试已恢复并完整复验，10项名字核对，中断运行不计通过。当前不再阻塞；下轮先查df，不累积旧构建缓存。
