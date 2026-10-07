# CAIdex 项目交接

更新：2026-10-07。开始工作先读本文件，再检查 Git；按下一步继续，不重新规划已确认方向。

## 当前任务

- A/B/C、D/E 与 F 第一至第三步当前范围已验收。F/G 原生 OpenAI 离线 Adapter 源码 e7253c8 已推送，[CI 37573720266](https://github.com/bboytang/CAIdex/actions/runs/37573720266) 三平台全部 success。下一项为 Anthropic 原生适配；F/G 整体与 H–R 尚未完成。按 V3 既有授权使用本地协议服务，不读/创建真实模型 Key。
- 用户已允许完整项目、原始方案、HANDOFF、UI/架构/实施文档公开到 `bboytang/CAIdex`；提交、push、CI 继续沿用授权。

## 已完成

- V3 执行计划、UI 规范、品牌原件归档、项目续接规则、Rust workspace、固定依赖和三平台 CI 已建立。原始 V2 仅历史参考，已确认调整优先。
- D：真实 app-server 双向 JSONL/facade、完整常规/实验方法门控、未知字段保留、审批/输入转交、取消/溢出关闭、不自动重试；19 项协议回归通过。
- D 真实 Runtime：历史/resume/fork、Queue CRUD/reorder/page/自动启动、审批 accept/cancel、Plan 输入、Steer/interrupt、apply_patch、MCP 工具/资源/elicitation、PTY/长进程、Goal 生命周期/预算/blocked、经典压缩已验证；本轮 CI 再次通过，详见能力对照。
- E：`credentials/core` Broker 固定 owner，reference 按 owner/provider/profile/kind 隔离；Secret 不可 Serialize/Display/Clone、释放时自身缓冲清零；配置状态仅公开元数据。
- E：Windows native keyring、Linux Secret Service（显式 feature，无 mock/fallback）、明确只读环境映射、Unix 私有文件 backend 已实现。文件保护含 0700/0600、symlink/hardlink/Git 拒绝、持有目录 FD、原子替换/fsync。
- E：结构化字段/HTTP header/已知值脱敏；已替换/删除秘密仍遮蔽。native 错误安全分类，不带原始 payload；锁定/访问拒绝与底层长度上限有专用错误。
- E：CLI `credentials status|set|remove` 已接入；明确 store/owner/provider/profile。set 必须 `--stdin` 管道输入，无 get/export 或秘密参数，env 只读。坏编码/空值/超长不覆盖原值。

- F 第一步：新增 model/core，保留经典/Lite wire/未知 item/event/opaque 与工具字符串；增量 SSE/UTF-8/换行/帧上限/终态/序号/响应身份验证。真实固定 Runtime 经典和 Lite 两轮请求/推理回放通过，仅合成回复。
- F 第二步（三平台已验）：model/gateway 只监听 loopback、随机独立访问 token、固定模型/endpoint/dialect、执行端 Broker 认证；SSE/非流式 JSON、单槽背压、真实断开/取消、超时、429/HTTP 错误分类和诊断脱敏。真实 Runtime 经典/Lite 各两轮通过，interrupt 在两条路径均实际关闭上游 socket；没有调用商业模型或执行 Code Mode 工具。
- F 第三步（三平台已验）：ModelProvider 六方法实际实现、CanonicalResponse/能力 Registry/版本化兼容性报告、独立 Custom Responses client。配置清单无自动发现/Full 标签，未知能力/上限不编造；Broker reference metadata 无秘密。HTTP-date、四项请求/两项响应 context header、单槽失败保存、取消/Drop/单并发 slot 释放、TLS 可信/未知 CA/错 hostname/过期 fixture 通过。
- F/G OpenAI 当前范围（三平台已验）：原生 Models GET、配置可调用模型交集、ProviderCatalog 证据门槛、scope/Broker、默认 store=false 与前台门控；原生两轮完整 output 回放、经典/Lite、Gateway 注入入口及独立 deadline/metadata/响应头门控。新 OpenAI 11/Gateway 注入 4 项各平台通过；真实 Runtime 新增 3 项，累计 Linux 23、Windows/macOS 各 22 项。

## 未完成

- iOS 已定义 SecretStore 接入契约，Swift Keychain/UniFFI 原生实现待 M。Gateway 已接入 Broker/诊断脱敏；UI/Host/同步尚未接入，不宣称未来全部输出通道已验证。
- F/G 其余能力及 H–R：其他原生 Provider/模型自动发现与实际兼容性报告、原生 opaque history 重建、生产持久化 Host、独立 Chat/同步、Windows/iOS UI、SSH、完整英文 CLI/attach、生产 Relay、签名/打包/UAT 未完成；未配置真实模型 Key 或调用商业模型。
- macOS Rust CI 不等于 iOS 应用构建；尚无 iOS 工程。其他原生工具/模型能力按后续相关阶段逐项补验收。

## 下一步顺序

1. 接入 Anthropic：先核实官方 Messages/Models schema、认证/分页、tool/reasoning 与 SSE/取消契约，检查 V3 和原 V2 对原生 history/Code Mode 的要求；用合成 fixture 实现并验收 ModelProvider 六方法及 Gateway。复用现有 Broker/core/传输边界，避免另造 Runtime；不重做 OpenAI。
2. 随后 Gemini→兼容 API/Ollama。Models 清单不提供 Codex 兼容性，不由可用模型名称猜 Full 或 capabilities。OpenAI 官方依据与验收限制在独立 Provider 文档。
3. 各 Adapter 原生请求/响应/工具/usage/reasoning/images/结构化输出/context/prompt compatibility 与兼容性报告逐项落实；经典与 Code Mode/opaque 多轮/切换分别验收。真实 API/用户 Key 前明确授权；不重做 A–E 或重复请求公开授权。
4. H/I→Windows→SSH/iOS→CLI→Relay→R，按 V3 验收；iOS simulator 测试及无签名 archive 在 GitHub macOS runner。离线开发已授权，真实 Key/付费调用仍须另行授权。

## 重要架构决定

- Windows Tauri 2 + React/TypeScript/Rust；iOS SwiftUI（初期 Swift 5/iOS 17+）+ UniFFI 共享 Rust；CLI 继承上游英文体验；首批 Windows 11 x64、Linux x86_64 Host/CLI。
- 使用真实固定 Runtime，不造第二套 Agent；上游 commit `d27764b82f7118f674371e6d6e76271d9d606edb`，CLI `0.160.1`/tag `rust-v0.160.1`。协议实验 opt-in；常规 104 客户端/10 服务端/83 通知，实验 167/11/83。
- Lite 使用 /responses 与内部 header，指令/工具在 input 中，稳定前缀；core 仅保留收到的 wire。non-OpenAI 上游会清除 encrypted_function_args，未知签名也可能被 Runtime 丢弃；F/G Adapter 需验证原生 opaque history，必要时最小补丁，不宣称被删除的数据已保留。
- Gateway 为 Rust 嵌入库，可注入独立 ModelProvider，已有 Custom/原生 OpenAI；其他原生 Adapter 待接入。Runtime profile 同时禁用 request/stream 重试，token 只在隔离子进程环境。HTTPS 默认验证，TLS 本地正/负 fixture 已验；未验真实 Provider。已开始的同步 SecretStore 读取不能强行中止，取消后不 POST；HTTP-date 转安全秒数元数据、不重试。仅固定上游四项请求/两项响应 context headers；OpenAI org/project 单独来自固定执行端配置，不向客户端开放任意原生认证头。
- 普通 Chat 独立于 Codex Host，无 Shell/Git/项目写权限；按提供商实际能力展示工具。Remote 用执行 Host Key，手机不读已保存 Host Key；历史同步不含凭据。
- Gateway 面向 Responses HTTP/SSE，保留不透明签名/推理数据，不执行工具。经典 gpt-5.5 与 Lite/code_mode_only gpt-6.1-sol 路径不同，旧未知模型 fallback fixture 不能代表全部兼容性；F/G 分别验收。
- 模型切换在轮次边界生效；跨提供商关联分支/新线程承接适配历史。Host 独立运行、SQLite 事件持久化后广播、快照补缺口、请求幂等、审批首次有效处理；不承诺外部工具恰好执行一次。
- UI 尽量 1:1 参考官方，缺失部分自主补齐；用户品牌原件为准。CAIdex 模型/API Key 放“设置 → 模型与提供商”，清楚区分本机与 Host 归属。
- Remote 先 SSH 后 Relay，生产 Relay 首版成熟方案端到端加密；Host 切换不迁移活动线程。Chat 自动同步自托管 VPS，SQLite/outbox/cursor、冲突分支/删除 tombstone；HTTPS 服务端为信任边界，不承诺历史同步 E2EE。
- iOS 先测试/无签名构建，未进入 TestFlight；无签名产物不等于可安装 IPA。真机、UAC、签名留独立验收。

## 问题 / 阻塞

- 原自动审批额度问题已恢复，依赖下载/编译成功。正常沙箱注入 `/tmp/.git` 导致文件保护测试被拒绝；相同代码在获授权的正常执行环境通过，未削弱保护。
- `.git` 普通执行只读，提交/push 用 require_escalated。GitHub CLI 2.45.0 已核验登录 `bboytang`、scope repo/workflow；普通沙箱网络受限时 auth status 的 invalid 提示不能作为凭据失效证据。
- 旧 Git helper 缺 workflow scope。发布使用 `git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main`，不输出凭据或修改用户全局配置。
- 本机没有 Windows/Xcode/gnome-keyring-daemon，相关验证转 GitHub。无真实模型、客户端逐状态截图/真机验证；按计划保留明确验收项。

## 文件与 Git 状态

- `docs/CAIdex-实施计划-V3.md`、`docs/CAIdex-UI-规范-V1.md`、`docs/CAIdex-Runtime-能力对照.md`：执行与验收基准；原 V2 在 docs 归档。
- `credentials/core/`、`apps/cli/src/credentials.rs`、`apps/cli/tests/credentials.rs`、`docs/CAIdex-Credentials-设计与验收.md`：E 实现/回归/限制。
- `.github/workflows/ci.yml`、`scripts/test-linux-secret-service.sh`、Cargo.toml/lock：新增 E 三平台与 Linux 私有服务验收。
- `model/core/`、`model/providers/custom/`、`model/gateway/`、`docs/CAIdex-Model-Gateway-设计与验收.md`：F 协议/模型接口/Registry/共享 HTTP client；Runtime dev-dependency 接入 Gateway 两轮/取消回归，生产 Runtime 未改。
- `model/providers/openai/`、`model/gateway/tests/injected.rs`、`docs/CAIdex-OpenAI-Provider-设计与验收.md`：原生 Models/Responses、scope/store/前台限制、Gateway 注入的当前恢复点。
- `runtime/bridge/`、`upstream/codex/`：固定 Runtime/协议/回归；本轮新增经典/Lite wire 回归/fixture，生产 Runtime 未改；`assets/brand/`：四份原始品牌资产未改。
- branch `main` 跟踪 `origin/main`，功能源码 e7253c8 已提交/push/三平台验收；验收文档与交接记录已在 fdc7254 提交/push。额度中断后复核：GitHub main 与本机一致，工作区无遗留源码或未跟踪文件、无残留构建进程。本次仅更新文档恢复点，续接以实际 Git status/log 为准。

## 测试结果

- [CI 37552752561](https://github.com/bboytang/CAIdex/actions/runs/37552752561)，源码 `42fdaa3`：ubuntu-24.04、windows-2022、macos-15 全部 success；fmt/Clippy/workspace、固定 schema 重新生成与 SHA/offline doctor 通过。
- F 核心 20/provider 7/Gateway 18 项三平台通过；provider 测试含 TLS 四种证书与直接六方法、真实 socket 取消/未消费队列 deadline。真实 Runtime Linux 20、Windows/macOS 各 19 项通过，包含经典/Lite 两轮与两条路径 interrupt 实际 EOF/reset、无重放；全部是合成推理数据，不代表商业模型或 Code Mode 工具执行已验。
- 首轮 Windows 可信 TLS fixture 失败已以明确 CA/leaf 名称、用途/AKI/有效期修正，新 CI 正/负例全部通过；未修改生产 TLS 策略或跳过测试。Cargo.lock 保留全部旧版本，新增 23 package（含独立 crate/证书测试依赖）。
- E 凭据回归 Linux 13 项（含私有服务 native），Windows 8 项（含 Credential Manager native/UTF-16 超长替换拒绝），macOS 11 项（Unix 文件/环境核心，无原生 Mac backend）。CLI Linux/macOS 各 3 项、Windows 2 项；各平台 Secret compile_fail 1 项。环境 child 由父测试显式运行，不重复计数。
- 既有 Runtime 协议 19 项与同 clock tick 目录隔离回归 1 项三平台通过。首轮时钟目录碰撞已修复（原子序号/创建成功才取得清理所有权），未禁用或盲目重试失败测试。
- 本机 workspace/fmt/Clippy/真实 Runtime 20 项及单并发 slot 释放/TLS 定向复验通过。E 安全文件/CLI 无已知回归，既有 headless no-default-features/脚本 bash -n 已验；本机 native Linux Secret Service 未执行，由 CI 私有服务验证。
- 日志 `/tmp/caidex-ci-37552752561.log` 用于本机复查，跨机器以 GitHub 链接为准。所有凭据回归用合成秘密，不读用户模型 Key；真实 Runtime 使用本地 fixture/隔离临时项目。
- 本轮 [CI 37573720266](https://github.com/bboytang/CAIdex/actions/runs/37573720266)，源码 e7253c8：三平台 fmt/Clippy/workspace/schema/doctor 与既有 native keyring 回归全部通过。OpenAI 11/Gateway 注入 4/core 20/Custom 7/Gateway HTTP 18 项各平台通过；真实 Runtime Linux 23、Windows/macOS 各 22 项，新增原生经典/Lite 两轮及两路径 interrupt 实际 EOF/reset 通过。本机 workspace/Runtime 23/fmt/Clippy/diff 通过；日志 `/tmp/caidex-ci-37573720266.log`。不代表商业模型或 Code Mode 工具执行已验。
- 中断后重新查询 GitHub：上述 CI completed/success，三个 job 均 success，提交 SHA 与 e7253c8 一致；验收文档 fdc7254 已在远端。未修改源码，本次不重复运行已有通过的代码测试。
- iOS、商业模型/其他 Provider、Windows Shell 批准后实际执行、真机/UAC/签名尚未执行；macOS Rust 验证不代表 iOS 构建。
