# CAIdex 凭据层：设计与验收

阶段 E；代码位置 `credentials/core`。本地编译、Clippy、安全回归和 CLI 回归已通过；平台原生读写待本轮 GitHub CI 验证。

## 执行端与 profiles

- `CredentialRef` = owner / provider / profile / kind。owner 是实际执行端持久 ID；普通 Chat 使用客户端 owner，Remote Codex 使用 Host owner。profile 为内部 ID，显示名称由设置元数据单独保存。
- ID 使用 1–64 字节小写 ASCII、数字、下划线或连字符。Windows 凭据目标不区分大小写，因此拒绝混合大小写及路径分隔符，保持各平台相同的身份语义。[Windows CREDENTIAL](https://learn.microsoft.com/en-us/windows/win32/api/wincred/ns-wincred-credentiala)
- Broker 固定一个 owner，拒绝其他 owner 的读取、保存、删除；backend 是执行进程内部接口。Host 的网络授权与设备权限仍由 H/L/Q 实现，不能将该本地检查冒称远程访问控制完成。
- UI/Remote 可返回 `CredentialStatus { reference, configured, readOnly }`。`resolve` 仅供执行进程的 Provider Adapter，未来 Host RPC 不提供读取已保存 Key 的方法。
- `Secret` 不实现 Serialize/Clone/Display，Debug 为遮蔽值；自身字符串由 Zeroizing 释放时清零。构造失败也清零；不承诺第三方 SDK/OS 所有中间副本都已清零。[Zeroizing](https://docs.rs/zeroize/latest/zeroize/struct.Zeroizing.html)

## 存储后端

| 后端 | 当前代码行为 | 当前验收 |
| --- | --- | --- |
| Windows Credential Manager | keyring 3.6.3 的显式 windows-native；独立 CAIdex service + 完整 reference | 原生 CRUD/Unicode/超长替换回归已写，待 GitHub Windows 验证 |
| Linux Secret Service | 显式 sync-secret-service + crypto-rust + vendored DBus；默认 feature 启用 | 隔离 DBus/keyring fixture 已写，待 GitHub Linux 验证 |
| Linux/Unix protected file | owner 0700 目录、0600 普通文件；拒绝 symlink/hardlink/权限过宽/Git worktree；持有目录 FD，原子替换并 fsync | 本地 5 项安全回归通过 |
| Environment | 每个 reference 明确映射变量；无默认探测或跨 profile fallback；只读 | 隔离子进程本地 5 项安全回归通过 |
| iOS Keychain | 接入同一 `SecretStore` 契约 | 原生实现/UniFFI 接入仍待 M |

- 系统存储不可用时明确返回安全错误，不自动降级到文件。用户可明确选择文件后端；文件是权限保护的明文存储，适用于原方案允许的 headless Host，不宣称已加密。
- native 错误不携带第三方原始 error 对象；避免错误附带坏编码的秘密或其他敏感数据。
- core 输入上限 16 KiB；native 超长秘密返回 SecretTooLong（底层存储字节上限），锁定或拒绝访问返回 StorageAccessDenied，坏编码返回 InvalidSecret；其他错误安全归类为 BackendUnavailable。Windows keyring 存密码为 UTF-16，底层字节上限不能直接当作 UTF-8 字符数。错误分类回归不输出第三方 payload。
- 凭据路径默认应在执行端应用数据目录。protected file 拒绝 Git worktree，secret/tmp 扩展名也加入 `.gitignore`；环境变量来源不被写回磁盘。
- keyring 必须选择真实平台 feature，无适用 feature 时该库会采用 mock，因此当前 SystemStore 仅在 Windows 或启用相应 feature 的 Linux 导出。[keyring 3.6.3](https://docs.rs/keyring/3.6.3/keyring/)

## 统一脱敏

- Redactor 处理诊断副本：敏感字段/HTTP header（Authorization、api_key、apikey、x-api-key、access_token、refresh_token、client_secret、cookie 等）及已注册完整秘密值。
- Broker 保存/读取时注册秘密，替换/删除后旧值仍被遮蔽，直到 Broker 释放。结构化 JSON、嵌套数组、JSON 文本及独立 header 行均有回归。
- 不对 Runtime 历史或 Provider wire 原文执行破坏性脱敏；诊断、崩溃报告、HTTP tracing/analytics 的接入点必须用安全副本。未来同步采用只含公开元数据的 DTO；不依赖全文替换来保证任意文档无秘密。
- 目前尚未接入 Host/UI/Gateway/同步等模块，不能声称所有未来输出通道均已验收。

## CLI 管理入口

- `caidex credentials status|set|remove --owner ID --provider ID --profile ID --store system|file|env`；kind 默认为 api-key，可显式选择 access-token/refresh-token/client-secret。
- file 必须提供 `--directory`（Unix、绝对路径、Git 外）；env 必须提供 `--variable`，只读。backend 必须明确指定，不自动读取其他环境变量或 fallback。
- set 必须带 `--stdin` 并通过管道输入 UTF-8 后关闭 stdin；拒绝交互终端输入，尚未实现掩码提示。只移除一个末尾 LF/CRLF，不裁剪空格。输入有长度上限，坏编码/空值/超长被拒绝。
- status 仅返回 reference/configured/readOnly；保存、删除返回公开 reference 和操作结果。没有 get/export 命令；参数错误不回显传入的值。
- CLI 管理的是当前执行进程的本地存储，owner 必须对应实际执行端；不代替 Host 网络权限。后续 UI 在“设置 → 模型与提供商”调用相同 Broker。

```sh
cargo run -p caidex-cli -- credentials --help
cargo run -p caidex-cli -- credentials status --owner local --provider custom --profile main --store system
```

## iOS 原生接入契约（阶段 M）

- Swift/UniFFI 适配实现 SecretStore 的 get/set/remove；Keychain generic-password 使用 CAIdex service 与完整 reference 作为账户标识。缺失返回 None，锁定/访问拒绝返回安全错误，不返回系统原始错误或秘密。
- 普通 Chat 使用设备 owner；Remote 请求只提交 Host 凭据设置/状态，手机不拉取保存的 Host Key。Keychain 不参与历史同步，禁止秘密进入 UniFFI 可序列化状态/错误 DTO。
- 实际 SecItem 读写、可访问性选项、前后台/重启行为和设备验证留在 M/R。当前 Rust trait 与自定义测试后端已验证，不冒称 Swift Keychain/UniFFI 已实现。

## 验证与恢复点

- 本机 `cargo check --workspace`、fmt、Clippy（-D warnings）、workspace 测试通过；凭据核心 12 项、CLI 3 项、不可 Serialize 的 compile_fail 1 项及既有 Runtime 协议 19 项通过。环境子进程只使用合成变量。
- 正常沙箱注入 `/tmp/.git`，文件测试因此被安全保护拒绝；在已授权的正常执行环境中复测通过，未削弱 Git worktree 检查。无需修改生产保护来迁就环境。
- Cargo.lock 已更新；`bash -n scripts/test-linux-secret-service.sh` 通过。审批额度问题已恢复，GitHub CLI 凭据经有网络执行环境核验有效。
- Windows 原生测试默认运行；Linux native 仅由脚本在临时 XDG/私有 DBus/unlocked fixture 中显式运行，禁止在用户 keyring 上执行 ignored native 测试。当前本机缺 gnome-keyring-daemon，本轮通过 GitHub 验证。
- 待完成：本轮三平台 CI/native 验证及结果记录；UI/Host/Gateway/同步输出通道接入和 iOS 原生实现仍属于后续阶段。不读取用户模型密钥，不调用商业模型。
