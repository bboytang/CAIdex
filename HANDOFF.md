# CAIdex 项目交接

更新：2026-10-06。开始工作先读本文件，再检查 Git；按下一步继续，不重新规划已确认方向。

## 当前任务

- A/B/C 已完成，D Runtime 边界已验收。当前 E 凭据核心及 CLI 管理已实现，本地编译/安全回归通过，待本轮三平台 CI 与原生存储验收。
- 用户已允许完整项目、原始方案、HANDOFF、UI/架构/实施文档公开到 `bboytang/CAIdex`；提交、push、CI 继续沿用授权。

## 已完成

- V3 执行计划、UI 规范、品牌原件归档、项目续接规则、Rust workspace、固定依赖和三平台 CI 已建立。原始 V2 仅历史参考，已确认调整优先。
- D：真实 app-server 双向 JSONL/facade、完整常规/实验方法门控、未知字段保留、审批/输入转交、取消/溢出关闭、不自动重试；19 项协议回归通过。
- D 真实 Runtime：历史/resume/fork、Queue CRUD/reorder/page/自动启动、审批 accept/cancel、Plan 输入、Steer/interrupt、apply_patch、MCP 工具/资源/elicitation、PTY/长进程、Goal 生命周期/预算/blocked、经典压缩已验证。Linux 15 项、Windows/macOS 各 14 项在旧 CI 通过，详见能力对照。
- E：`credentials/core` Broker 固定 owner，reference 按 owner/provider/profile/kind 隔离；Secret 不可 Serialize/Display/Clone、释放时自身缓冲清零；配置状态仅公开元数据。
- E：Windows native keyring、Linux Secret Service（显式 feature，无 mock/fallback）、明确只读环境映射、Unix 私有文件 backend 已实现。文件保护含 0700/0600、symlink/hardlink/Git 拒绝、持有目录 FD、原子替换/fsync。
- E：结构化字段/HTTP header/已知值脱敏；已替换/删除秘密仍遮蔽。native 错误安全分类，不带原始 payload；锁定/访问拒绝与底层长度上限有专用错误。
- E：CLI `credentials status|set|remove` 已接入；明确 store/owner/provider/profile。set 必须 `--stdin` 管道输入，无 get/export 或秘密参数，env 只读。坏编码/空值/超长不覆盖原值。

## 未完成

- E 本轮 Windows 原生 CRUD/Unicode/超长替换与 Linux 隔离 Secret Service CI 待运行；iOS 已定义 SecretStore 接入契约，Swift Keychain/UniFFI 原生实现待 M。UI/Host/Gateway/同步模块尚未接入凭据与诊断脱敏。
- F–R：模型核心/Gateway/Provider、生产持久化 Host、独立 Chat/同步、Windows/iOS UI、SSH、完整英文 CLI/attach、生产 Relay、签名/打包/UAT 未完成；未配置模型 Key 或调用商业模型。
- macOS Rust CI 不等于 iOS 应用构建；尚无 iOS 工程。其他原生工具/模型能力按后续相关阶段逐项补验收。

## 下一步顺序

1. 提交当前 E 代码/锁文件/CI/文档并 push，使用已验证 gh 凭据 helper。检查 Linux/Windows/macOS CI，修复实际失败；Linux native 只能通过私有服务脚本运行，不用用户 keyring。
2. 记录 CI 链接、平台原生验证及限制；更新本交接与凭据验收文档后再确认 E 当前范围验收。不要把未来 UI/Host/iOS 接入标为完成。
3. F/G：先核对锁定上游模型元数据与经典 Responses、Lite/code_mode_only 协议，写最小模型核心/Gateway 与离线回归；保留 opaque 推理/签名。实际付费 API 或用户凭据复用/创建前明确授权。
4. H/I→Windows→SSH/iOS→CLI→Relay→R，按 V3 验收；iOS simulator 测试及无签名 archive 在 GitHub macOS runner。

## 重要架构决定

- Windows Tauri 2 + React/TypeScript/Rust；iOS SwiftUI（初期 Swift 5/iOS 17+）+ UniFFI 共享 Rust；CLI 继承上游英文体验；首批 Windows 11 x64、Linux x86_64 Host/CLI。
- 使用真实固定 Runtime，不造第二套 Agent；上游 commit `d27764b82f7118f674371e6d6e76271d9d606edb`，CLI `0.160.1`/tag `rust-v0.160.1`。协议实验 opt-in；常规 104 客户端/10 服务端/83 通知，实验 167/11/83。
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
- `runtime/bridge/`、`upstream/codex/`：固定 Runtime/协议/回归，本轮未改；`assets/brand/`：四份原始品牌资产未改。
- branch `main` 跟踪 `origin/main`；HEAD `fb1100c`，D 验证代码 `5a593a4`。当前未提交：E crate/CLI/测试/设计文档/私有服务脚本、workspace/锁文件/CI/secret 文件忽略、README、本交接。无其他已知用户修改。

## 测试结果

- 当前本地 E：workspace check、fmt、Clippy `-D warnings`、workspace 测试通过：凭据核心 12 项（含 native 错误分类）、CLI 3 项、Secret compile_fail 1 项、既有协议 19 项；环境 child 由父测试显式运行。CLI 最后补充非法输入/保留空格后 focused 回归再次通过。
- `bash -n scripts/test-linux-secret-service.sh`、`git diff --check` 通过；Cargo.lock 已更新。当前 Linux native ignored，等私有服务 CI；真实 Runtime 集成未因本轮改动在本机重复运行，由本轮 CI 回归。
- D 基准 [CI 37523663697](https://github.com/bboytang/CAIdex/actions/runs/37523663697) 全三平台 success：协议 19、真实集成 Linux 15/其他 14、fmt/Clippy/schema 指纹/offline doctor；不能当作 E 原生验收。
- iOS、商业模型/其他 Provider、Windows Shell 批准后真实执行、真机/UAC/签名均尚未执行。所有当前新增回归用合成秘密，不读用户模型 Key。
