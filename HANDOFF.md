# CAIdex 项目交接

更新：2026-10-06。开始工作先读本文件，再检查 Git；按下一步继续，不重新规划已确认方向。

## 当前任务

- A/B/C 已完成，D Runtime 边界已验收。E 凭据核心及 CLI 当前模块范围已验收，三平台 CI 全通过（4cab5c1 / 37546559022）。当前 F 第一步协议核心已实现，本机 15 项核心及 17 项真实 Runtime 通过，待三平台 CI；HTTP Gateway/Adapters 未实现；未来 UI/Host/同步/iOS 接入仍按后续阶段验收。
- 用户已允许完整项目、原始方案、HANDOFF、UI/架构/实施文档公开到 `bboytang/CAIdex`；提交、push、CI 继续沿用授权。

## 已完成

- V3 执行计划、UI 规范、品牌原件归档、项目续接规则、Rust workspace、固定依赖和三平台 CI 已建立。原始 V2 仅历史参考，已确认调整优先。
- D：真实 app-server 双向 JSONL/facade、完整常规/实验方法门控、未知字段保留、审批/输入转交、取消/溢出关闭、不自动重试；19 项协议回归通过。
- D 真实 Runtime：历史/resume/fork、Queue CRUD/reorder/page/自动启动、审批 accept/cancel、Plan 输入、Steer/interrupt、apply_patch、MCP 工具/资源/elicitation、PTY/长进程、Goal 生命周期/预算/blocked、经典压缩已验证。Linux 15 项、Windows/macOS 各 14 项在旧 CI 通过，详见能力对照。
- E：`credentials/core` Broker 固定 owner，reference 按 owner/provider/profile/kind 隔离；Secret 不可 Serialize/Display/Clone、释放时自身缓冲清零；配置状态仅公开元数据。
- E：Windows native keyring、Linux Secret Service（显式 feature，无 mock/fallback）、明确只读环境映射、Unix 私有文件 backend 已实现。文件保护含 0700/0600、symlink/hardlink/Git 拒绝、持有目录 FD、原子替换/fsync。
- E：结构化字段/HTTP header/已知值脱敏；已替换/删除秘密仍遮蔽。native 错误安全分类，不带原始 payload；锁定/访问拒绝与底层长度上限有专用错误。
- E：CLI `credentials status|set|remove` 已接入；明确 store/owner/provider/profile。set 必须 `--stdin` 管道输入，无 get/export 或秘密参数，env 只读。坏编码/空值/超长不覆盖原值。

- F 第一步：新增 model/core，保留经典/Lite wire/未知 item/event/opaque 与工具字符串；增量 SSE/UTF-8/换行/帧上限/终态/序号/响应身份验证。真实固定 Runtime 经典和 Lite 两轮请求/推理回放通过，仅合成回复。

## 未完成

- iOS 已定义 SecretStore 接入契约，Swift Keychain/UniFFI 原生实现待 M。UI/Host/Gateway/同步模块尚未接入凭据与诊断脱敏，不宣称未来全部输出通道已验证。
- F/G 其余能力及 H–R：HTTP Gateway/Provider/完整模型接口、生产持久化 Host、独立 Chat/同步、Windows/iOS UI、SSH、完整英文 CLI/attach、生产 Relay、签名/打包/UAT 未完成；未配置模型 Key 或调用商业模型。
- macOS Rust CI 不等于 iOS 应用构建；尚无 iOS 工程。其他原生工具/模型能力按后续相关阶段逐项补验收。

## 下一步顺序

1. 本轮 F 代码尚未提交，先提交并完成三平台 CI（本地 fmt/Clippy/workspace/真实 Runtime 17 项通过），更新验收结果；不重做 A–E 或重复请求公开授权。
2. F 第二步：读模型/Gateway 设计文档，实现本地 HTTP/SSE Gateway + Custom Responses Adapter，执行端 Broker 认证/显式模型路由。离线验证转发、实际 socket 取消、断开/超时/背压、HTTP 429 与安全诊断；Gateway 不执行工具、不自动重放 POST。
3. 随各 Provider Adapter 实现完整接口/模型 Registry 和兼容性报告；经典与 Code Mode/opaque 多轮/切换分别验收。真实 API/用户 Key 前明确授权，当前离线工作可继续。
4. H/I→Windows→SSH/iOS→CLI→Relay→R，按 V3 验收；iOS simulator 测试及无签名 archive 在 GitHub macOS runner。

## 重要架构决定

- Windows Tauri 2 + React/TypeScript/Rust；iOS SwiftUI（初期 Swift 5/iOS 17+）+ UniFFI 共享 Rust；CLI 继承上游英文体验；首批 Windows 11 x64、Linux x86_64 Host/CLI。
- 使用真实固定 Runtime，不造第二套 Agent；上游 commit `d27764b82f7118f674371e6d6e76271d9d606edb`，CLI `0.160.1`/tag `rust-v0.160.1`。协议实验 opt-in；常规 104 客户端/10 服务端/83 通知，实验 167/11/83。
- Lite 使用 /responses 与内部 header，指令/工具在 input 中，稳定前缀；core 仅保留收到的 wire。non-OpenAI 上游会清除 encrypted_function_args，未知签名也可能被 Runtime 丢弃；F/G Adapter 需验证原生 opaque history，必要时最小补丁，不宣称被删除的数据已保留。
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
- `model/core/`、`docs/CAIdex-Model-Gateway-设计与验收.md`：当前 F 协议核心及下一步；Runtime tests 新增经典/Lite fixture，model core 仅为 Runtime dev-dependency。
- `runtime/bridge/`、`upstream/codex/`：固定 Runtime/协议/回归；本轮新增经典/Lite wire 回归/fixture，生产 Runtime 未改；`assets/brand/`：四份原始品牌资产未改。
- branch `main` 跟踪 `origin/main`，HEAD `7806241`；未提交：model/core、workspace/锁文件/CI model 路径、Runtime dev-dependency/经典与 Lite 真实回归/fixture、模型设计文档/本交接。E 验证基准 4cab5c1，无其他已知用户修改。

## 测试结果

- 当前 F 本机：fmt/Clippy/workspace tests 与真实 Runtime 17 项通过，模型核心 15 项；与 E/19 协议/目录隔离回归无已知回归。本轮三平台 CI 待发布；以下 CI 链接为上一 E 基准。

- [CI 37546559022](https://github.com/bboytang/CAIdex/actions/runs/37546559022)，代码 `4cab5c1`：ubuntu-24.04、windows-2022、macos-15 全部 success；fmt/Clippy/workspace 测试、固定 schema 重新生成与 SHA/offline doctor 均通过。
- E 凭据回归 Linux 13 项（含私有服务 native），Windows 8 项（含 Credential Manager native/UTF-16 超长替换拒绝），macOS 11 项（Unix 文件/环境核心，无原生 Mac backend）。CLI Linux/macOS 各 3 项、Windows 2 项；各平台 Secret compile_fail 1 项。环境 child 由父测试显式运行，不重复计数。
- 既有 Runtime 协议 19 项与同 clock tick 目录隔离回归 1 项三平台通过；真实 Runtime 集成 Linux 15 项、Windows/macOS 各 14 项通过。首轮时钟目录碰撞已修复（原子序号/创建成功才取得清理所有权），未禁用或盲目重试失败测试。
- 本机 workspace check/fmt/Clippy/测试、安全文件回归、CLI/非法输入/保留空格回归及真实 Runtime 15 项通过；`cargo check -p caidex-credentials --no-default-features --locked --offline` 通过，headless 不要求 native 服务。脚本 bash -n 与 final diff 检查通过。
- 日志 `/tmp/caidex-ci-37546559022.log` 可用于本机复查，跨机器以 GitHub 链接为准。所有新增凭据回归用合成秘密，不读用户模型 Key；真实 Runtime 使用本地 fixture/隔离临时项目。
- iOS、商业模型/其他 Provider、Windows Shell 批准后实际执行、真机/UAC/签名尚未执行；macOS Rust 验证不代表 iOS 构建。
