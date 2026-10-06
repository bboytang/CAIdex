# CAIdex

基于真实 Codex Runtime 的 Windows、iOS 和 CLI 多模型客户端，支持独立 Chat、远程 Codex 与自托管历史同步。当前处于基础工程开发阶段，完整客户端尚未实现。

当前可以运行：固定版本 app-server 的双向 JSONL 适配、协议回归测试、离线 `caidex doctor`。doctor 只创建隔离的临时线程，不启动模型轮次、不执行项目命令、不读取用户 Codex 登录配置。

## 开发与验证

需要 Rust 1.99.0、Python 3、Codex CLI 0.160.1。Node 22.23.3 用于 CI 工具。

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p caidex-cli -- doctor
```

doctor 输出 JSON 验证结果；版本不匹配直接报错。可通过 `CAIDEX_CODEX_BIN` 指定实际 Codex 可执行文件。

Windows npm 的 `.cmd` 包装器不适合作为原生进程接口。CI 安装固定 npm 包后用以下命令定位原生二进制，再将返回路径设为 `CAIDEX_CODEX_BIN`：

```sh
npm install --prefix .tools/codex --no-audit --no-fund --ignore-scripts @openai/codex@0.160.1
node scripts/codex-binary.mjs
```

## 工程位置

- `runtime/bridge`：上游 stdio 边界、请求关联、通知/请求分流。
- `apps/cli`：开发阶段诊断命令；最终 CLI/共享 Host 接入仍待实现。
- `upstream/codex`：版本/源码锁定、协议基准和上游许可证。
- `assets/brand`：用户指定的品牌原件。
- `docs/CAIdex-实施计划-V3.md`：阶段顺序与验收条件。
- `docs/CAIdex-UI-规范-V1.md`：UI 布局和 CAIdex 功能入口。
- `HANDOFF.md`：当前进度、验证与下一步；每次续接先读。

当前适配层是本地传输组件，尚不包含生产 Host 的持久化、重连和生命周期管理。普通 Chat、模型 Key 设置和 Windows/iOS 页面尚未实现。

## CI 与许可证

基础 CI 在 Linux、Windows 和 macOS 验证 Rust 工程及离线协议。iOS 应用工程建立后加入 simulator 测试和无签名 archive；目前 macOS Rust 检查不等同于 iOS 构建。

上游 Codex 与归档协议遵循 `upstream/codex/LICENSE`、`NOTICE`。CAIdex 自有代码及品牌的对外分发许可证尚未确定。
