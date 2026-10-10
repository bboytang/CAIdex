# CAIdex

基于真实 Codex Runtime 的 Windows、iOS 和 CLI 多模型客户端，目标支持独立 Chat、远程 Codex、官方统一 CAIdex 账户、跨模型长期记忆与可选云同步。当前仍处于基础工程开发阶段，完整客户端及账户/记忆/云服务尚未实现。

正式目标架构：CAIdex 统一运营账户及 PostgreSQL + pgvector 云服务；Windows/iOS/Linux CLI本地缓存/记忆与Host journal保留SQLite且逻辑分离。记忆同步默认关闭，独立于自动记忆和 Chat 历史同步，首次开启须登录并确认范围；关闭采用账户级权威状态，支持保留或删除原云端记忆。记忆归属不可变 user_id，Chat/整合/Embedding 模型分开，跨模型共享不迁移活动Codex线程。CLI只登录已有账户（Windows/iOS注册），桌面用浏览器PKCE、VPS/SSH用Device Code；登录继承账户已有记忆同步状态，Enabled不等于自动上传未授权历史本地数据。账户登录不授予 Host 执行权限，用户模型 API Key 仍留在执行端；未登录/云故障时本地能力按既有权限继续。

规划云服务使用可替换 EmailSender（Brevo 优先、Resend 备用）及早期单实例 VPS，只做本机有限备份与用户导出，无异地备份；整台 VPS/磁盘损坏可能数据库和备份全损，本地缓存不保证全量恢复。详细边界与全部待实施验收见[Account/Memory/Cloud 设计](docs/CAIdex-Account-Memory-Cloud-设计与验收-V1.md)，按 V3 在 H/I 及后续客户端阶段实现，不代表当前已有账户/Memory Engine/UI。

当前可以运行：执行端凭据 Broker、脱敏及 CLI 凭据状态/保存/删除，固定版本 app-server 的双向 JSONL 适配、Runtime facade（线程/轮次/Steer/interrupt、能力门控、审批/输入转交）、协议回归测试与离线 `caidex doctor`。完整协议调用入口保留常规和实验方法；具体模型/工具能力仍需逐项验证，详见能力对照。doctor 只创建隔离的临时线程，不启动模型轮次、不执行项目命令、不读取用户 Codex 登录配置。

完整CLI目标是共享真实Host/固定Codex Runtime的英文TUI、无头exec、持久task、模型/Provider/Profile、账户会话、本地及跨端记忆、Remote与扩展管理；账户认证不代Host审批权限，exec与后台任务语义分开。详见[CLI完整交互与验收规范V1](docs/CAIdex-CLI-完整交互与验收规范-V1.md)，H/I提供底层契约，P整合CLI，R实测。**这些是规划，当前可执行CLI仍仅doctor、credentials status/set/remove、版本/帮助，CLI-01～34均未执行。**

CLI规范已核对固定0.160.1的命令、配置与无头审批行为：普通exec默认Never，特定AutoReview配置按最终解析策略处理；规划中的task继承Host持久任务审批配置。F/G DeepSeek实际Runtime离线接线已完成三平台验收，Qwen实际Runtime也已完成三平台离线验收，下一步OpenRouter；规范补全不表示提前开展H/I/P/R实现。

完整CLI规范及V3、账户、UI、凭据设计已正式衔接；固定stdin、观察端detach、超时已确认/未知及登录取消规则保留。十一份固定上游源码与六项版本/help复核后，补清旧Profile显式导入/冲突拒绝、命令作用域及账户认证端点信任隔离；模型Endpoint或项目配置不能接收Account Token。34项CLI验收仍全部待实现、未执行。Qwen custom/v3历史也已独立三平台验收，当前F/G恢复点为OpenRouter完整政策绑定历史；namespace/custom已精确三平台验收；上下文/effort/逐route正文控制已三平台验收；Qwen Classic/Lite固定Runtime新增7项、累计57/56/56项及Qwen72项已精确三平台通过；精确源码和CI证据见HANDOFF。

续接复核补清无头 `exec fork` 的ForkOnly：不带prompt只创建分支，成功不代表执行任务；恢复目标不存在时不静默新建任务。此为P/R待实现契约，当前开发CLI仍无exec/fork/resume。F/G控制组合独立推进，验证结果见Qwen专属文档和HANDOFF，CLI文档核对不代验源码。

## 开发与验证

需要 Rust 1.99.0、Python 3、Codex CLI 0.160.1。Node 22.23.3 用于 CI 工具。

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p caidex-cli -- doctor
node scripts/verify-codex-schema.mjs
```

doctor 输出 JSON 验证结果；版本不匹配直接报错。可通过 `CAIDEX_CODEX_BIN` 指定实际 Codex 可执行文件。

Windows npm 的 `.cmd` 包装器不适合作为原生进程接口。CI 安装固定 npm 包后用以下命令定位原生二进制，再将返回路径设为 `CAIDEX_CODEX_BIN`：

```sh
npm install --prefix .tools/codex --no-audit --no-fund --ignore-scripts @openai/codex@0.160.1
node scripts/codex-binary.mjs
```

## 工程位置

- `runtime/bridge`：上游 stdio 边界、Runtime facade、方法清单、审批/用户输入与事件转交。
- `model/core`：ModelProvider 六方法接口、模型能力/兼容性 Registry、经典/Lite 请求/完整回复/工具/usage 视图、增量 SSE 和流生命周期。
- `model/providers/custom`：可独立调用的 Custom Responses 推理 client，供普通 Chat 与 Gateway 共用；配置模型列表、Broker 认证、显式 context headers、TLS 验证、取消/超时/背压与安全错误。
- `model/providers/openai`：原生 Models 发现/Responses 适配器，显式执行端组织/项目与凭据引用；复用共享传输，默认无服务端历史存储。当前使用合成协议服务验证，真实 API 兼容性待验。
- `model/providers/anthropic`：原生 Messages/Models/SSE、Responses 转换和绑定历史；Gateway、经典 MCP 发现/执行/重启和 Lite Code Mode 审批/执行/取消已三平台离线验收，默认缓存网页搜索及商业模型兼容性待验。
- `model/providers/google`：Gemini 原生 Models/generateContent、媒体/推理/结构输出和绑定历史；Gateway 与固定经典/Lite Runtime 离线接线已三平台验收，商业模型兼容性待验。
- `model/providers/ollama`：复用共享传输的原生 Responses 适配，六方法、能力/推理、媒体/结构输出、绑定历史和 deferred 工具发现已三平台离线验收；固定经典 Runtime MCP 执行/重启及 Lite Code Mode 审批/执行/取消/磁盘恢复亦已三平台验证，真实 daemon/模型兼容性待验。
- `model/providers/deepseek`：原生 Models 目录、六方法和经典文本 JSON/SSE，复用共享传输与独立执行端凭据；基础 Gateway、显式本地上下文、developer 优先指令与 verbosity 编译已三平台离线验收；新增显式 Classic 函数/namespace、明文推理绑定历史及执行端 effort 映射也已分别完成精确三平台离线验收；新增独立显式 custom apply_patch 与 v2 历史绑定（grammar 仅作指导）已精确三平台离线验收；显式 Runtime+history 组合新增本地 summary auto/context all_turns/include 控制，DeepSeek42 项及 workspace423/418/422 已精确三平台离线验收；多 reasoning/content-part 与交错流索引及未知扩展边界已精确三平台离线验收（DeepSeek46 项，workspace427/422/426）；显式 Lite custom→function、本地单调用及 v3 原生历史绑定已精确三平台离线验收（新增11项/共57项，workspace438/433/437）；默认仍 Classic-only，grammar 不承诺原生约束。其他高级控制和真实模型兼容性待验；固定Classic/Lite实际Runtime新增7项、累计Runtime50项本地通过（审批/临时执行/磁盘恢复/拒绝与取消），新接线已以1b501ba/[CI37886226991](https://github.com/bboytang/CAIdex/actions/runs/37886226991)独立三平台通过（Runtime50/49/49），不代表生产Host或Live/Full。
- `model/providers/qwen`：原生分页目录/六方法、显式地域工作区与执行端Key、基础Classic文本JSON/SSE；复用Custom传输，默认不存服务端历史，不支持控制Key前拒绝。基础14项及workspace/Clippy本地通过，精确3638d13/[CI37888877980](https://github.com/bboytang/CAIdex/actions/runs/37888877980)三平台离线通过（workspace452/447/451，既有固定Runtime回归50/49/49）；新增显式本地Runtime控制、verbosity指令及逐route effort映射，7项新增/21项及workspace459/0/52、Clippy/fmt本地通过，精确311224b/[CI37890668777](https://github.com/bboytang/CAIdex/actions/runs/37890668777)三平台已验（workspace459/454/458，既有Runtime回归50/49/49）；本轮显式原生summary绑定历史新增11项/共32项本地定向通过，workspace470/0/52及Clippy/fmt/diff已通过，精确a3e7e6d/[CI37929549601](https://github.com/bboytang/CAIdex/actions/runs/37929549601)三平台完整通过（workspace470/465/469，既有Runtime50/49/49，Qwen32每名一次）；本轮新增显式function/namespace、native allowed_tools选择与成对结果/v2工具历史，11项新增/43项定向通过，workspace481/0/52及Clippy/fmt/diff本地通过，精确c5c021d/[CI37932689731](https://github.com/bboytang/CAIdex/actions/runs/37932689731)三平台完整通过（workspace481/476/480，既有Runtime50/49/49，Qwen43每名一次）；上述43项证据不涵盖后续custom/Lite；独立验收见后续条目，此43项CI不涵盖实际Qwen Runtime；当前证据见后续条目，商业Live/Full尚未验收，详见[Qwen设计与验收](docs/CAIdex-Qwen-Provider-设计与验收.md)。
- Qwen当前custom子阶段：显式`with_custom_tool_mapping()`将自由文本工具编译成function(input:string)，恢复原身份并以v3历史绑定策略；grammar仅指导，解析/审批/执行仍在真实Runtime。新增11项/共54项及最终workspace492/0/52、Clippy/fmt/diff本地通过，源码ae0b2d0/[CI37939652724](https://github.com/bboytang/CAIdex/actions/runs/37939652724)三平台完整通过（workspace492/487/491、既有Runtime50/49/49，Qwen54每名一次）；这是离线协议验证，该CI不涵盖后续Lite；该CI不涵盖实际Qwen Runtime；当前证据见后续条目，商业Live/Full仍待。
- Qwen Runtime/history组合：双策略显式本地消费summary/context/include，统一验证v1/v2/v3 carrier的native effort内部契约，旧effort不再次映射。8项新增/62项定向及workspace500/0/52、Clippy/fmt/diff本地通过；精确源码0858493/[CI37974303497](https://github.com/bboytang/CAIdex/actions/runs/37974303497)三平台完整通过（workspace500/495/499、既有Runtime50/49/49，Qwen62逐名一次），不授实际Qwen Runtime/Live/Full。Lite及实际Qwen Runtime独立验收见下条。
- Qwen显式Lite Adapter：`with_lite_options`编译additional_tools/developer稳定ID，复用custom→function与本地最多单调用交付，v4绑定Lite历史政策；默认Classic不放宽，grammar/parallel不宣称原生生成约束。新增8项/Qwen70、workspace508/0/52、既有固定Runtime50/0/0及Clippy/fmt/diff本地通过；精确源码3ec8ca7c2d429ccfdad403134d6450acff2f5163/[CI37978406850](https://github.com/bboytang/CAIdex/actions/runs/37978406850)三平台完整通过（workspace508/503/507、既有Runtime50/49/49、Qwen70逐名一次），该CI不涵盖实际Qwen Runtime，当前证据见下条；商业Live/Full未验。
- Qwen实际Classic/Lite固定Runtime：新增7项真实审批/隔离工具执行、磁盘恢复不重跑、拒绝与取消；最小修复显式runtime_context下typed developer/user消息稳定ID，新增Provider2项。精确bf94c9d/[CI37982550340](https://github.com/bboytang/CAIdex/actions/runs/37982550340)三平台完整通过：workspace510/505/509、实际Runtime57/56/56、Qwen72；旧通过名精确+2/+7。Codex仍是唯一执行/审批真源，商业Live/Full、生产Host及iOS构建未验。
- `model/providers/openrouter`：原生目录/六方法、基础Classic文本JSON/SSE，固定无状态与路由策略，复用Custom传输与执行端Broker；14项及workspace524/0/59、既有Runtime57/0/0、Clippy/fmt本地通过，精确9563df0/[CI38004040230](https://github.com/bboytang/CAIdex/actions/runs/38004040230)三平台完整通过（workspace524/519/523、既有Runtime57/56/56，OpenRouter14逐名一次）。工具/推理历史/Lite/实际OpenRouter Runtime及商业Live/Full仍待。OpenRouter上下文/effort新增9项、本地23项与workspace533/0/59、既有Runtime57/0/0、Clippy/fmt/diff已验；默认拒绝与身份隔离保持，精确92d0612/[CI38005373396](https://github.com/bboytang/CAIdex/actions/runs/38005373396)三平台已验（workspace533/528/532、既有Runtime57/56/56，OpenRouter23逐名保持）。
- OpenRouter逐route正文控制：显式verbosity文本指导与service_tier请求映射，默认缺策略/未知/跨档静默降级拒绝，实际tier保留；新增8项/共31项、workspace541/0/59、既有Runtime57/0/0、Clippy/fmt/diff本地通过，精确2799a4c/[CI38006780427](https://github.com/bboytang/CAIdex/actions/runs/38006780427)三平台通过（workspace541/536/540、既有Runtime57/56/56，OpenRouter31逐名一次）。工具/后端绑定历史与实际Runtime另验。
- `model/gateway`：Rust 库形式的本地 Responses HTTP/SSE Gateway，可注入原生 ModelProvider；生产 Host/CLI 配置入口和剩余兼容 API 接入待实现。
- `credentials/core`：执行端身份/profile 隔离、系统/环境/Unix 文件存储与诊断脱敏。
- `apps/cli`：开发阶段诊断与本地凭据管理命令；最终 CLI/共享 Host 接入仍待实现。
- `upstream/codex`：版本/源码锁定、协议基准和上游许可证。
- `assets/brand`：用户指定的品牌原件。
- `docs/CAIdex-实施计划-V3.md`：阶段顺序与验收条件。
- `docs/CAIdex-CLI-完整交互与验收规范-V1.md`：完整英文CLI/命令契约、PKCE/Device登录、账户级记忆同步、Host/审批/exec与34项待实施验收。
- `docs/CAIdex-Account-Memory-Cloud-设计与验收-V1.md`：官方账户/PostgreSQL、长期记忆、账户级同步开关、隐私/删除/本机备份及待实施验收矩阵。
- `docs/CAIdex-Model-Gateway-设计与验收.md`：模型协议/真实 wire 验证与 Gateway 恢复点。
- `docs/CAIdex-OpenAI-Provider-设计与验收.md`：原生模型发现、认证/存储边界与离线验收。
- `docs/CAIdex-Anthropic-Provider-设计与验收.md`：Anthropic 原生协议、工具发现与固定 Runtime 离线验收。
- `docs/CAIdex-Gemini-Provider-设计与验收.md`：Gemini 原生协议、绑定历史与固定 Runtime 离线验收。
- `docs/CAIdex-Ollama-Provider-设计与验收.md`：Ollama 配置、明确拒绝的控制项与三平台离线验收。
- `docs/CAIdex-DeepSeek-Provider-设计与验收.md`：DeepSeek 原生契约、基础 Adapter 范围与待验证能力。
- `docs/CAIdex-Runtime-能力对照.md`：完整固定协议清单、实现范围与 CLI 对照验收状态。
- `docs/CAIdex-UI-规范-V1.md`：UI 布局和 CAIdex 功能入口。
- `HANDOFF.md`：当前进度、验证与下一步；每次续接先读。

当前适配层是本地传输组件，尚不包含生产 Host 的持久化、重连和生命周期管理。CLI 可通过 `caidex credentials --help` 查看凭据管理用法；没有秘密导出命令，保存只接受显式管道输入。普通 Chat、模型 Key 设置页面和 Windows/iOS 页面尚未实现。

## CI 与许可证

基础 CI 在 Linux、Windows 和 macOS 验证 Rust 工程及离线协议。iOS 应用工程建立后加入 simulator 测试和无签名 archive；目前 macOS Rust 检查不等同于 iOS 构建。

上游 Codex 与归档协议遵循 `upstream/codex/LICENSE`、`NOTICE`。CAIdex 自有代码及品牌的对外分发许可证尚未确定。


OpenRouter后端绑定历史的前置路由政策已本地实现：显式`with_backend_selection`逐route配置单一`provider.only`，保留禁fallback与参数要求，调用方不能覆盖；不升级能力、不将base slug冒充实际稳定endpoint。新增5项/Provider36、本地workspace546/0/59及既有Runtime57/0/0、Clippy/fmt/diff通过；源码`2c9954c`/[CI38008446175](https://github.com/bboytang/CAIdex/actions/runs/38008446175)精确三平台已验，workspace546/541/545、既有Runtime57/56/56，全通过名精确旧CI+5。该阶段新增范围为路由政策；后续工具及历史进度见下段与HANDOFF。


OpenRouter显式逐route Classic平面function工具已实现：须配置backend后启用with_native_tools，声明/选择/文本结果配对及JSON/SSE终态门控；工具仍由固定Codex执行，Provider只交付数据。新增15项/Provider51定向通过；旧36项保持，原生opaque/未知输出扩展无损。工具SSE缓冲到终态核验，会延迟文本显示；完整本地workspace561/0/59、既有固定Runtime57/0/0、Clippy/fmt/diff通过，通过名精确旧workspace+15；源码`65001cd`/[CI38010128508](https://github.com/bboytang/CAIdex/actions/runs/38010128508)精确三平台已验，workspace561/556/560、既有Runtime57/56/56、OpenRouter51每名一次，完整通过名精确旧CI+15；该源码尚不含namespace/custom；新增高级工具进度见下段，完整政策绑定历史/Lite/实际Runtime仍待。


OpenRouter逐route显式advanced工具已实现：须先配置backend/native_tools，namespace/function/custom原生身份与自由文本保留，具名高级/allowed_tools通过精确声明子集+auto/required编译一次；kind配对与混合SSE终态校验、默认拒绝、source/compiled预算及Gateway身份隔离覆盖。新增12项/Provider63定向通过，旧51项保持；完整本地workspace573/0/59、既有固定Runtime57/0/0、Clippy/fmt/diff及18份Markdown/95本地链接/22锚点通过；通过名精确旧workspace+12、旧Runtime不变，源码`ca2cf58`/[CI38047726929](https://github.com/bboytang/CAIdex/actions/runs/38047726929)精确三平台已验，workspace573/568/572、既有固定Runtime57/56/56、OpenRouter63每名一次，完整通过名精确旧CI+12，无遗漏/重复。完整政策绑定历史/Lite/实际OpenRouter Runtime与商业Live/Full未验；工具流文本仍延迟到终态。详见[OpenRouter验收](docs/CAIdex-OpenRouter-Provider-设计与验收.md)和[HANDOFF](HANDOFF.md)。


OpenRouter完整政策绑定原生历史已实现、19项新增定向通过，Provider82：逐route显式开启，完整原生reasoning/签名/未知扩展与JSON/SSE保存在载体，Runtime可保留投影整组+完整原生前缀校验后原序回放；绑定执行端CredentialRef/profile/配置endpoint、backend政策/model/能力/原正文控制与工具政策，默认关闭。源码仅OpenRouter，无依赖/共享执行器改动；本轮workspace592/0/59、既有固定Runtime57/0/0、Clippy/fmt/diff及18份Markdown/99链接/22锚点通过，旧通过名精确+19、旧63测试正文与Cargo.lock保持；精确源码三平台已验，证据见末段。文本终态缓冲、完整前缀二次增长由预算限制；载体非加密/来源认证，backend政策不证明实际endpoint稳定。summary/context/include、Lite、实际OpenRouter Runtime与商业Live/Full仍待。详见[OpenRouter验收](docs/CAIdex-OpenRouter-Provider-设计与验收.md)和[HANDOFF](HANDOFF.md)。


精确源码`ebd8c9a2668816a72eb7f0d5d3a3de85a912bba1`/[CI38050867010](https://github.com/bboytang/CAIdex/actions/runs/38050867010)整体completed/success。linux job114209551728，workspace592/0/59、既有固定Runtime57/0/0，raw2234行/通过名650；windows job114209551790，workspace587/0/57、既有固定Runtime56/0/0，raw1920行/通过名643；macos job114209551777，workspace591/0/57、既有固定Runtime56/0/0，raw1931行/通过名647；各17steps成功或条件跳过，OpenRouter82/Qwen72/DeepSeek57每名每平台一次；全通过名精确旧CI38047726929+19，无遗漏/重复，旧Runtime名完全保持。watch和三份最终完整日志下载exit0，ci-check通过；Linux首次下载返回zip无效，缺日志的首次checker未通过，重试下载exit0后逐名完整核对通过，不是CI失败；/tmp/caidex-openrouter-history/及/tmp/caidex-ci-38050867010-{linux,windows,macos}-raw.log与status.json只作补充，仓库证据足够跨机器恢复。这些既有Runtime回归不是实际OpenRouter接线、商业Live/Full或iOS应用构建。


OpenRouter显式原生summary/context/include组合已实现，新增8项，Provider90定向通过：双Runtime上下文/历史策略，逐route summary/context支持声明，nullable/已知include原样native转发；原source与完整允许值、一次effort/verbosity/tier编译绑定，未配置summary/context仍拒绝对应非空值、既有v1无新政策字段兼容。共享执行器/HTTP/依赖不变，本轮workspace600/0/59、既有固定Runtime57/0/0、Clippy/fmt/diff及18份Markdown/103链接/22锚点通过，精确源码三平台已验；下一步Lite→实际OpenRouter Runtime，商业Live/Full未验。context有模型限制，不按slug猜支持；文本终态缓冲/完整前缀二次增长和预算限制保留。详见[OpenRouter验收](docs/CAIdex-OpenRouter-Provider-设计与验收.md)和[HANDOFF](HANDOFF.md)。


精确源码`fa42c2214e2c3ef9ac40ba8971e83db12cbab5c8`/[CI38052051289](https://github.com/bboytang/CAIdex/actions/runs/38052051289)整体completed/success。linux job114212988549，workspace600/0/59、既有固定Runtime57/0/0，raw2242行/通过名658；windows job114212988428，workspace595/0/57、既有固定Runtime56/0/0，raw1928行/通过名651；macos job114212988540，workspace599/0/57、既有固定Runtime56/0/0，raw1939行/通过名655；各17steps成功或条件跳过，OpenRouter90/Qwen72/DeepSeek57每名每平台一次；全通过名精确旧CI38050867010+8，无遗漏/重复，旧Runtime名完全保持。watch和三份最终完整日志下载exit0，ci-check通过；Linux首次gh日志出口exit0但仅1217行/64通过名，首次checker拒绝不完整记录；只读job logs API重取2242行/658通过名后逐名检查通过，不是CI失败；/tmp/caidex-openrouter-controls/及/tmp/caidex-ci-38052051289-{linux,windows,macos}-raw.log与status.json只作补充，仓库证据足够跨机器恢复。这些既有Runtime回归不是实际OpenRouter接线、商业Live/Full或iOS应用构建。


## OpenRouter Lite Adapter（完整本地已验）

OpenRouter Lite步骤2已完成：显式with_lite_options保留route Classic/Lite元数据，native Classic传输；首项developer additional_tools/稳定ID原生工具编译，developer消息ID复用显式Runtime上下文；namespace/custom与grammar原样，parallel=false复用JSON/SSE终态单调用门控。v2完整政策载体绑定原始工具前缀/ID/parallel及既有scope/model/backend/正文/前缀，Classic v1不变，public decoder也核对内部Lite政策和版本。新增10项，Provider100/0/0；覆盖JSON/SSE、多轮原序恢复、非法前缀与async/deferred、跨Classic/篡改拒绝、预算、取消/deadline/Drop/满槽释放和Gateway隔离。初期fixture方法/字段类型编译问题已修正；首可执行99/1为第二轮SSE response.created ID夹具不一致，修正后100/0/0，无已知失败。共享栈/执行器/依赖不变；实际OpenRouter Runtime和商业Full仍未验。Lite步骤3已完成：workspace610/0/59、既有固定Runtime57/0/0、最终OpenRouter100/0/0、Clippy全workspace/all-targets -D warnings、fmt/diff和18份Markdown/103本地链接/22锚点通过。全通过名精确旧workspace600+10，旧Runtime57保持；旧90测试正文逐字保持，仅history模块追加Lite接线，Cargo.lock/共享源码/其他Provider/Runtime/workflow未改。目录返回Classic/Lite配置且Unknown保持；无工具Lite默认策略请求已验。精确新源码三平台CI待核验，下一步实际固定Classic/Lite Runtime接线。
