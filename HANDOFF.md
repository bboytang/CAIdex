# CAIdex 项目交接

更新：2026-10-07。先读本文件，再检查 Git/AGENTS.md；沿 V3 恢复点继续，不重新规划已确认架构。

## 当前任务与恢复点

F/G：Gemini v1绑定原生历史与Responses输出投影已提交/push并三平台验收，源码`421ead55b8c516021c81be277808bda37a2f886a`，[CI37672020384](https://github.com/bboytang/CAIdex/actions/runs/37672020384)completed/success。当前在实现请求编译器的依赖：ToolMap及显式v2映射历史，Google44项、完整workspace266passed/0failed/32ignored及Clippy/fmt/diff本地通过，唯一独立审查无Critical/Important、1项prefix交叉替换覆盖Minor暂缓；新源码提交/CI待完成；随后Responses请求编译，再六方法/Registry/Gateway/固定真实Runtime。不要重复已验Models/JSON/SSE/HTTP/历史或Anthropic阶段。完整F/G未验收，H–R尚未实施。

完整项目/文档公开、提交/push/CI及离线合成fixture已获授权，不重复询问。不读取用户模型Key，不调用商业API；合成fixture不授予商业Full。

## 已完成

- A–E：V3/UI/品牌原件、固定上游、Rust workspace/CI、真实app-server facade，审批/Queue/Steer/Goal/MCP/PTY离线回归；执行端Broker、Windows keyring/Linux Secret Service/受保护文件/env、脱敏及CLI凭据入口。
- F：经典/Lite canonical wire、SSE/opaque/工具原文、loopback Gateway独立token/取消/背压/TLS/安全错误、ModelProvider六方法及Registry证据门槛；Custom/OpenAI当前离线范围已三平台验收。
- Anthropic：原生Messages/Models、工具/图片/推理/结构输出/Runtime参数、增量SSE、六方法；v3编译前缀/认证组织绑定、v4动态发现/原始输出绑定；经典显式禁用网页后实际MCP发现/执行/重启恢复、Lite审批Code Mode执行/取消已三平台验收。默认经典cached web_search明确拒绝且Key/POST为0，非Full。
- 共享tool_search专用call/output契约已验：原tools/arguments、client call_id和完整终态保留，由Runtime执行。
- Gemini：Models分页、generateContent JSON、原生SSE、真实streamGenerateContent HTTP、共享slot/安全错误/TLS/取消/Drop/背压/正常EOF已三平台验收。新history.rs保留完整native response/request、所有SSE chunks、Part/thoughtSignature/未知字段/大数；模型/原请求归属及完整展示组校验，所选候选Responses输出、精确usage及JSON/SSE原文下一POST回放已三平台验收。

## 未完成与下一步顺序

1. Gemini Responses请求编译：复用现有CanonicalRequest/经典/Lite输入边界；核对固定Runtime实际wire及原生generateContent，不混用Interactions签名/事件。实现system/messages、完整历史组恢复与前缀绑定；缺省native Content.role归属model。失败/过滤/MAX_TOKENS里未执行call须明确配对/拒绝，replay_content仅是原始视图，不代表可自动提交。
2. 工具声明/namespace/custom及call/result配对、图片、推理/结构输出/Runtime参数；不执行工具、不隐式降级。输出投影目前是native函数名，尚非完整工具映射。动态发现、unsupported能力按真实契约显式处理。
3. 接六方法/Registry/Gateway和固定Runtime，经典/Lite、审批/工具/取消/持久恢复分别离线验证；每阶段新源码三平台CI，不用旧证据代验。
4. Gemini → 兼容API/Ollama → V3 H/I → Windows → SSH/iOS → CLI → Relay → R。iOS simulator/无签名archive在GitHub macOS；真机/UAC/签名/逐模型商业报告独立验收。

## 重要架构决定

- Windows11x64 Tauri2 + React/TypeScript/Rust；Linuxx86_64 Host/CLI；iOS17+/Swift5 SwiftUI + UniFFI。GUI中文可切英文，CLI英文。只用assets/brand用户原件，UI尽量1:1官方参考，模型/Key放设置。
- 固定真实Codex0.160.1，commit`d27764b82f7118f674371e6d6e76271d9d606edb`；一个Runtime，不造第二套Agent。JSONL facade保留未知方法/字段，实验opt-in；经典与Lite/Code Mode分别验收。
- Gateway只做Responses HTTP/SSE适配，不执行工具；Runtime负责发现/审批/沙箱/执行。关闭request/stream重试，loopback token只注入隔离子进程；HTTPS校验不可削弱。
- reasoning.encrypted_content在CAIdex只是版本化敏感JSON载体，不是加密/认证。native wire为回放权威；Anthropic v3绑定实际system/tools/messages/认证组织，v4保存初始声明及原始discoveries；旧v1–v3不自动升级。Gemini v1绑定model/原native request，保存raw chunks并重建验证派生response；结构校验不认证伪造整组或签名真实性。生产历史权限留H/I。
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

- branch main跟踪origin/main，起点99be6c8；本轮未提交tools.rs/tests/tools.rs、history.rs v2、HTTP1、lib.rs导出、Cargo清单/lock及HANDOFF/Gemini文档。sha2沿既有锁定0.10.9只新增Google直接依赖；源码421ead5已提交/push。本次续接起点工作区干净；验收后的HANDOFF/Gemini/Model-Gateway文档更新随此次交接单独提交/push，本段前半的新工具映射代码仍未提交，待完成新验收；上次文档更新已push且起点干净。之后须核对实际Git状态；文档commit不触发路径限定CI，验收锚定上述源码SHA。
- 执行基准：docs/CAIdex-实施计划-V3.md、CAIdex-UI-规范-V1.md；原V2仅背景，冲突以V3为准。详细历史证据留Runtime-能力对照及各Provider/Credentials/Model-Gateway验收文档。
- 当前相关：model/providers/google/src/{client,content,stream,transfer,history}.rs、tests/{catalog,content,stream,http,history}.rs；下一步参考Anthropic request/tools/projection/binding及model/core，不复制HTTP栈。
- 代码边界：credentials/core、apps/cli、model/core/providers/gateway、runtime/bridge、upstream/codex、.github/workflows/ci.yml、scripts；assets/brand四份原件未改。

## 最近验证

- 本轮tools6初始501 RED→GREEN，namespace说明丢失独立RED→GREEN；实际HTTP1扩大已测codec集成范围（无单独实现前RED），Google44、本地workspace266/0fail/32ignored与Clippy/fmt/diff通过。日志 /tmp/caidex-google-tools-{red,map-green-history-red,green,namespace-red,http-green,workspace,clippy}.log；唯一审查无Critical/Important，prefix-only v1/v2交换覆盖Minor暂缓；新源码提交/CI未完成。
- 上轮history6/实际HTTP1有效RED→GREEN；本地Google37、workspace259passed/0failed/32ignored，Clippy -D warnings/fmt/diff通过。唯一独立审查无Critical/Important，代码Minor和排除项裁定留Gemini文档。
- 精确源码421ead55b8c516021c81be277808bda37a2f886a的CI37672020384三平台completed/success且所有步骤已核对；每个平台Google catalog4/content3/history6/HTTP20/stream4共37个测试名各通过一次。workspace Linux259/Windows254/macOS258（0失败，ignored32/30/30），真实Runtime Linux30/WindowsmacOS29（0失败/0ignored）；fmt/Clippy/native credentials/schema/doctor通过。
- 日志：/tmp/caidex-google-history-{red,green,http-red,http-green,workspace,clippy}.log、/tmp/caidex-ci-37672020384-status.json与{,-linux,-windows,-macos}.log。跨机器以GitHub证据为准；普通workspace忽略的Runtime/原生服务由CI显式执行。
