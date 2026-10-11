# CAIdex I-2 认证/会话设计与验收 V1

2026-10-11用户确认I-1独立审计通过并批准下一步；本里程碑按V3既定顺序实施I-2。I-1审计通过来自用户确认，不以实施会话自查替代独立审计。起点main/HEAD/origin/main均`d0075114aef5b2761c4c94c07c7edc0853087984`，工作区干净，磁盘剩余1.1GiB。不使用ponytail技能。

## 目标与有限任务

在I-1 PostgreSQL17/pgvector0.8.2上实现真实本地认证服务及共享契约：邮箱验证注册、密码登录/恢复/重新认证、邮箱变更、Passkey注册/登录/撤销；EmailSender接口；独立auth_sessions、设备列表、刷新轮换/重放检测、单独/其他设备撤销；CLI公开客户端PKCE/Device授权和取消/迟到清理，固定issuer/client/audience/端点归属。秘密不进入日志、普通同步载荷或模型配置。独立认证DB角色不具有Chat/记忆/项目数据权限。

实施顺序：①核对契约/依赖并记录边界；②版本2迁移、最小认证核心和HTTP试用服务；③真实隔离数据库/合成邮箱及Passkey正负例、并发/重放/撤销/取消测试；④差异检查、提交push main、精确CI与证据封存。

依赖：I-1不可变迁移与RLS，既有Rust共享核心、安全存储能力和固定Runtime身份域。不得改写0001迁移或解释执行端CredentialRef.owner。

## 非目标与待真实验收

I-3及后续Chat/cache/Memory/sync状态机、正式CLI命令/TUI、Windows/iOS原生界面及系统Passkey关联、商业模型/真实用户Key、真实邮件投递、生产部署未授权。本地测试issuer/RP只允许localhost，不能宣称完成生产域名注册；生产issuer/RP/客户端注册需明确独立信任配置及域名所有权，模型Endpoint/项目/Profile不能覆盖。遗失邮箱和全部Passkey时不自动核验、不给显示名恢复权；具体人工核验属于既定待设计审查项，当前拒绝无证明恢复。

## 通过与停止条件

- 密码经成熟Argon2id库哈希、随机盐、版本参数；有界并发哈希，测量本机时间/内存，不宣称2核2GB压力验收。
- 验证/恢复材料只存摘要或服务端安全状态；绑定用途/身份/请求，限期/限次/重发及地址/账户/IP/全局限流，原子一次性消费；重复/未知地址对外不泄漏存在性。
- 成熟WebAuthn库验证challenge、RP/origin、签名/UV；公钥持久化，不保存私钥；测试软件认证器不冒充Windows Hello/iOS真机。
- opaque access及refresh仅存安全摘要；轮换原子、旧refresh重放撤销family，合法并发/响应丢失明确重新认证，不无限宽限旧token；撤销确认后的受保护请求立即拒绝。
- 敏感操作重新认证，不能撤销最后可用认证方式；恢复撤销旧会话/凭据并发送通知，user_id不变；账户停用阻断会话且不删除其他账户数据，完整同步删除/本地队列界面留I-6/J/P/R。
- CLI仅已有账户、无Client Secret；S256/loopback/state/client/redirect绑定，一次性授权及Device间隔/slow_down/拒绝/过期/取消正确；取消或迟到只清理本次会话，不启动同步、不影响其他有效会话。
- 真正连接非owner/non-superuser/NOBYPASSRLS角色，参数化查询与事务ROLLBACK/COMMIT避免串账户；新认证角色不能读取Chat/记忆/项目。复验I-1相关边界。
- 所有相关测试、fmt/clippy、三平台共享契约及Linux真实服务数据库CI通过；精确功能SHA、原始证据、试用步骤、限制可复现。封存后停止，等待I-2独立审计；不自行授审计通过、不启动I-3。

## 当前状态

上轮因持久盘ENOSPC停止最终验证；2026-10-11进度/磁盘核查时已有3.7GiB可用，项目目录与/tmp写入/fsync成功，尚未重跑Docker/构建/真实DB。中间108组通过不含最后登录撤销并发修复；最后代码Clippy/共享5项通过，真实DB复验、提交/push/精确CI未完成。不得授I-2通过，恢复步骤和部分证据见HANDOFF。

用户随后要求后续构建/测试尽量在GitHub并继续I-2。完整构建、真实数据库并发断言与三平台回归转GitHub Actions；本地只做必要静态/语法/差异检查。`scripts/i2-auth.py`补独立`caidex-auth-local`进程的注册→邮箱验证→登录→me操作，以Linux `/proc/PID/status`实测RSS/峰值RSS，以`/proc/PID/stat`的utime+stime差值实测CPU；不计构建、测试客户端或PostgreSQL，峰值包含服务启动。测量使用新合成账户/私有受保护测试邮箱，结束只停自身进程/删除自身fixture，不认领2核2GB压测；最终数值和精确SHA须由实际CI确认。

## 实现与边界

`cloud/auth`默认只编译跨平台账户信任/会话与成对安全存储契约；`server`启用Linux认证服务，`test-auth`额外启用软件认证器，不能进入生产认证器选择。依赖锁定Argon2 0.5.3、webauthn-rs 0.5.5、oauth2 5.0.0。新锁文件只增加依赖，不升级既有包。固定Codex0.160.1、Runtime/Host/Provider/执行凭据源码不变。

0002迁移在独立管理员事务中添加认证角色、issuer/Passkey状态及challenge、旧refresh摘要、公开客户端grant、限流表；失败完整回滚，0001未改写。全库24表，其中23私有表FORCE RLS。认证角色仅访问必要身份/凭据/会话/安全事件，不读Chat/Memory/项目，不改不可变user_id/同步权威字段；数据角色不能读认证材料。服务拒绝superuser/BYPASSRLS/可建角色/库等危险连接；只接本地Unix socket，数据请求使用独立连接、参数化`bind_session($1)`和事务，结束/失败清绑定。此部署不是TCP明文数据库连接示例。

密码要求至少15个Unicode字符、最多1024字节，Argon2id v19/m65536/t3/p1和随机盐；每个服务实例只有一个在途认证请求/哈希任务，拥挤时限流，不无界排队。邮箱仅支持ASCII dot-atom与DNS域名，域名小写，保留local部分大小写、点与+别名。注册/恢复返回通用`accepted`，只表示受理；邮箱验证前不建账户、不登录。随机256位验证材料只存SHA256，服务端验证用途、期限、会话和次数，重发废弃旧材料；邮箱/用户/IP/全局限制独立。Passkey状态只存服务端私有表，不能交给cookie/客户端；成熟库校验challenge、RP/origin、签名和UV，公钥/计数状态持久化，私钥不入库。软件认证器伪造UV仅用于fixture，不能据此认领真人验证或平台硬件通过。

新账户的Memory Sync/Chat同步保持关闭，`me`读取服务端权威状态。邮箱变更保持user_id，密码变更/邮箱变更/恢复撤销旧会话；恢复同时撤销旧Passkey等凭据，以验证邮箱和新密码重新建立认证方式。敏感变更须短期一次性密码/Passkey proof，不能撤销最后可用认证方式。账户删除接口当前明确返回`account_disabled`及`data_erasure: pending_I6_I7`：立即停用账户/凭据/会话，不冒充完整同步与数据擦除。

access/refresh各256位随机opaque材料，仅存摘要；access10分钟，refresh30天闲置期限。轮换用事务/用户与会话锁串行化，旧摘要保留；重放旧refresh立即撤销同session family。严格策略下合法并发轮换只有一次成功，另一旧token请求按重放处理，成功返回的新token也失效；响应丢失后不能重试旧refresh恢复，须重新认证。无旧token宽限。设备/安装UUID和名字仅展示，不代身份或Host授权；设备数量无默认上限。列表每页200条，`next_cursor`用作下页`after_session_id`，游标受同账户RLS约束；分页过程中新增/撤销仍可能改变可见状态，不保证历史快照。

CLI固定公开client `caidex-cli`/audience `caidex-data-v1`，无长期secret。独立Trust验证issuer及所有固定端点；生产仅HTTPS根issuer，本地test仅`http://localhost:port/`，两类凭据命名空间分离，不能由模型Endpoint/Profile覆盖。PKCE使用成熟oauth2库S256和256位CSRF state，callback仅允许带显式非零端口的127.0.0.1或[::1] `/callback`，校验state/redirect并只消费一次。授权页只登录已有账户并明确Approve/Deny；授权码绑定client/issuer/audience/redirect/S256，一次兑换。Device Grant有短码HMAC、5秒起始间隔/slow_down/拒绝/过期/一次性兑换。PKCE起始为CAIdex JSON契约，token兑换采用标准OAuth form和成熟客户端；不宣称通用OIDC身份提供者或任意第三方OAuth客户端兼容。

取消secret绑定本次grant；取消与兑换均锁定grant，迟到取消只撤销本次签发会话，保留原浏览器和其他会话。PKCE本地`LoginContext`必须将认证后`me`的authorization_context匹配本次state，再向既有SecretStore一次保存access/refresh对并读回核对；取消/保存失败进入CleanupRequired，调用方必须执行本次grant取消/会话撤销，不能启动同步。重开保存条目状态为saved_unverified，须联网重新确认再恢复功能。凭据owner用独立安装命名空间，不混用执行端CredentialRef.owner或云user_id。P负责正式CLI监听器/命令/TUI/Device轮询与退出事件整合，J/M负责原生账户/平台Passkey与系统安全存储；当前共享契约和隔离真实协议测试不能代这些正式客户端验收。

EmailSender携带最小收件地址/目的/语言/期限/随机邮件ID及需要的验证材料，本地仅私有进程邮箱或Git外0700/文件0600的受保护sandbox。发送限制5秒，失败/超时不能回滚安全撤销或泄漏恢复资格；不记录原始邮件/邮箱/令牌/DB错误参数。投递不保证、无持久重试队列，返回accepted不表示排队或送达；可靠提供商/额度/中国网络送达在I-7真实验收。认证表历史/限流桶目前不裁剪，生产长期存储维护/高负载/备份恢复尚未验证。

## 可复现验证与本地试用

Linux需要Rust1.99.0、Docker、Python3和本地daemon；先检查磁盘/内存，不删除有效缓存。以下命令创建随机独立无网络/无发布端口的PostgreSQL17.10+pgvector0.8.2容器，使用固定镜像digest、私有Unix socket、独立非owner角色，结束只删除自身容器/临时fixture。需要首次拉取镜像和锁定依赖；不接生产库或真实邮箱。

```sh
cargo fmt --all -- --check
cargo clippy -p caidex-auth --features test-auth --all-targets --locked -- -D warnings
cargo test -p caidex-auth --features test-auth --lib --locked
python3 scripts/i1-database.py --output /var/tmp/caidex-i1-recheck.json
python3 scripts/i2-auth.py --output /var/tmp/caidex-i2-recheck.json
```

输出路径必须尚不存在。I-1检查只独立重建0001，I-2检查重建0001+0002并注入迁移失败；源码指纹在cargo前记录，运行后再次比对，变化则拒绝封存。`I2_CHECK`是具名断言组，不是Rust测试数量；每组可能包含多个条件，真实联合集成只有一个Rust测试。脚本同时输出PG/vector版本、本机注册/验证/登录时间和Argon内存参数；配置参数不是实际RSS/2核2GB压力测量。三平台默认workspace验证共享契约；真实认证服务仅Linux数据库job验证。

同一脚本在集成检查后构建并启动独立本地服务，测量一个注册/验证/登录流程，报告`measurements`中的`standalone_service_register_verify_login`含实际RSS/CPU和runner CPU/内存环境。脚本的完整JSON报告与具名检查打印到GitHub数据库job原始日志，可按精确run/job下载；本地复现仍可使用上述命令，但日常完整构建/回归优先GitHub。

手工试用服务入口：先为**新的私有本地测试库**按管理员顺序执行`cloud/postgres/bootstrap.sql`、0001、0002；建立一个仅继承caidex_auth的LOGIN角色和另一个仅继承caidex_app的LOGIN角色。使用Git目录外新建的0700目录作为邮件sandbox，每个服务实例使用独立目录；不能共享首次初始化pepper目录。设置`CAIDEX_AUTH_DB`和`CAIDEX_DATA_DB`为上述两角色Unix socket DSN，执行：

```sh
cargo run -p caidex-auth --features server --locked --bin caidex-auth-local -- 18080 /var/tmp/caidex-account-private-sandbox
```

该示例仅监听127.0.0.1，issuer/RP为localhost；只使用合成`@example.test`邮箱和测试密码，不暴露公网。受保护邮箱文件是本地测试秘密，只在自身账户私有目录查看，不能复制到Git/日志/截图。下面所有action使用`POST /v1/auth/{action}` JSON；涉及现有会话时以Bearer携带opaque access，先用真实`me`确认身份，不把fixture UID或token内容当身份。

| 操作 | 请求与可观察结果 |
| --- | --- |
| 注册/确认 | register `{email,password}`→accepted；从私有sandbox取register验证材料；verify `{purpose:"register",token}`→verified/user_id；未验证时login拒绝 |
| 登录/状态 | login `{email,password,platform,installation_id?,display_name?}`→独立会话/令牌；me `{}`→权威UID、当前SID、默认同步关；sessions `{}`及`{after_session_id}`→安全元数据分页 |
| 刷新/撤销 | refresh `{refresh_token,client_id,issuer,audience}`→成对新token；revoke `{}`退出本设备、`{session_id}`撤销同账户设备、`{others:true}`撤销其他设备；`{display_name}`改本设备显示名。动作互斥，非法类型拒绝 |
| 重认证/身份 | reauth `{password}`→短期proof；change-email `{email,proof}`发验证材料，verify `{purpose:"email",token}`后旧会话失效；change-password `{password,proof}`立即要求重新登录 |
| Passkey | passkey-register `{proof}`→server options/request，浏览器navigator.credentials.create；finish提交原始credential；passkey-login `{email}`/finish；passkey-proof/finish提供敏感proof；credential-list/revoke管理方式 |
| 恢复/停用 | recover `{email}`通用受理，verify `{purpose:"recover",token,password}`撤销旧材料和全部会话；delete-account `{proof}`→停用且明确I6/I7擦除待做 |
| PKCE | pkce `{client_id,issuer,audience,redirect_uri,code_challenge,code_challenge_method:"S256",state}`→authorization_uri/request/cancel_secret；浏览器授权→loopback code/state；token标准form兑换，client侧消费匹配callback并验证me再保存 |
| Device/取消 | device `{client_id,issuer,audience}`→user_code/device_code/verification_uri/interval；打开`/device`登录已有账户并明确审批；token按Device标准grant_type轮询，遵守interval/slow_down；cancel `{cancel_secret}`只清本次授权 |

密码/验证材料/令牌在真实操作时不能放入共享shell历史或命令参数。HTML仅提供已有账户授权登录/Passkey入口，不是正式账户管理GUI，也没有HTTP邮箱接口。Ctrl-C停止本地服务；测试数据库/邮件由操作者按其自己创建的资源范围移除，不清用户资料或缓存。
