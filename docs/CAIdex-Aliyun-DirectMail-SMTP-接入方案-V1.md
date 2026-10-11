# Aliyun DirectMail SMTP 接入方案 V1（待确认）

## 授权、基线和停止条件

2026-10-11用户要求先提交接入方案、文件清单、测试与生产切换步骤，确认前不实施、不修改生产、不发送额外真实邮件。用户报告已创建no-reply@caidex.cn、设置DirectMail SMTP密码并实际发信成功；这是用户已验证事实，不冒充本会话对CAIdex流程的验证，也不读取该密码。

只读核实GitHub最新main与本地HEAD均为955085db636f9509da16f941d2d65bbfea1e7ff5，工作区起点干净。现有[生产部署记录](CAIdex-云服务部署-20261011.md)记载运行源码c522a57、schema5、mail_disabled=true；历史I-7/部署审计状态保持。本轮仅方案和交接，不运行SMTP认证/发信、不建立生产连接、不改变源码或迁移、不启动J。

确认方案后的有限目标：复用EmailSender与现有注册/邮箱验证/找回流程，加入独立Aliyun SMTP适配器，保留Brevo/Resend；通过模拟SMTP、真实隔离PG、相关Rust检查及精确提交CI后封存，等待另一次生产切换确认。权限、Chat/Memory同步行为和协议、本地SQLite schema5、Host、模型、GUI、新队列/自动重试/供应商自动切换、异地备份均不在范围。

## 接入方式

新增AliyunSmtpEmail，实现现有EmailSender；当前ProviderEmail继续承担Brevo/Resend HTTP发送。只把现有mail_tasks预算/抑制/任务预留/结果记录和模板提取为两适配器共用的私有MailLedger/函数，避免复制安全规则。生产入口用一个有限枚举选择HTTP、Aliyun或关闭，保持现有mail_disabled行为；不引入新的邮件框架/路由架构。

固定smtpdm.aliyun.com:465，连接建立即隐式TLS，验证系统信任链、主机名与SNI，TLS1.2以上；无明文、降级、任意SMTP URL或跳过证书校验配置。SMTP登录名、envelope MAIL FROM和From统一来自no-reply@caidex.cn。阿里云要求认证用户名与发件人一致，465提供加密入口：[官方SMTP说明](https://help.aliyun.com/zh/direct-mail/user-guide/send-emails-using-smtp)、[官方区域端点](https://www.alibabacloud.com/help/zh/direct-mail/smtp-endpoints)。切换时核对该发信地址所在区域与既定端点相符，不自动换区域。

建议新增精确锁定lettre=0.11.23的可选server依赖，关闭默认features，仅启用MIME builder/SMTP/Tokio-Rustls及必要证书验证，复用现有Rustls/AWS-LC和系统验证组件；不启用连接池、SMTP tracing、文件邮件或sendmail，不升级无关依赖。依赖增量/锁文件和跨平台编译在实施时核验，不能提前声称已兼容。成熟库处理SMTP协议、MIME、UTF-8和点转义；连接/读写/整体事务均有界，发送总期限服从现有5秒上限，取消关闭本次连接，不后台续发。[lettre官方功能文档](https://docs.rs/lettre/0.11.23/lettre/index.html)

## 凭据与发送状态

新增独立Broker引用：owner=caidex-service、provider=account-service、profile=aliyun-smtp、kind=client-secret；服务器私有目录仍/var/lib/caidex/secrets，0700/0600。密码仅从Broker解析，在内存中用于SMTP AUTH；不出现在配置/命令参数/URL/日志/Git/CI，不打印SMTP帧或供应商原始错误。模拟测试只用明确合成密码，真实密码不进入开发机或CI。阿里云选择不要求Brevo/Resend的mail-api/mail-webhook，但不删除它们、不读取无关凭据；device-code-pepper保留。

发送流程仍为：模板与输入校验→持久预算/抑制检查与任务预留→提交unknown→一次SMTP事务→保存结果。所有适配器共用现有全局advisory锁和日/月尝试预算，90%停止非必要通知、100%拒绝所有新尝试；失败尝试不退额度，不重置旧任务/抑制/计数。现有邮箱/IP/全局发送频率限制保持。

| 结果 | 持久状态及后续行为 |
| --- | --- |
| DATA结束后明确成功答复（通常250） | accepted，表示SMTP服务器受理；不写delivered |
| 明确不受理的SMTP负答复 | rejected，记录安全类别，不自动重发 |
| 超时、断线、取消、崩溃、结果落库失败或无法确定是否受理 | unknown，不自动重发、不自动换供应商 |
| 相同任务ID/相同payload已有accepted | 返回既有受理，不再建立SMTP发送 |
| 相同任务ID换payload或换provider | 拒绝；unknown/rejected也不重发原任务 |

Message-ID由任务UUID确定，只作关联，SMTP并不提供因此去重的保证；一次受理由本机持久任务规则控制，不宣称端到端exactly-once。未取得可信供应商消息ID时provider_id保持NULL，不将本地Message-ID冒充供应商ID，不保存原始SMTP回复。注册/找回的既有通用accepted响应也不证明发信或收件箱送达。

Aliyun当前无可信退信事件接入，callback默认拒绝；/v1/mail/events在Aliyun模式返回401，不设置未认证Webhook、不解析未经验证的退信、不伪造delivered/bounced/complaint。已有跨供应商抑制记录仍有效，新退信自动抑制能力本轮未提供。实际收到邮件只能作为指定测试邮箱的验收事实，不自动更新任务为delivered。

## 必要数据库兼容

已核查0005中的mail_tasks.provider CHECK仅允许brevo/resend，不能复用旧provider名字伪装阿里云。新增0006_aliyun_directmail.sql事务迁移，只将CHECK扩展为brevo/resend/aliyun_directmail并登记云schema6；旧迁移不改，无权限/RLS/账户数据/同步控制/epoch更改，protocol1及本地SQLite5保持。

因此需要更新cloud/core版本/迁移登记、隔离测试安装顺序与版本断言，及备份工具兼容；这属于邮件迁移的必要配套，不改变同步模块。备份识别4/5/6，恢复4先到5、再到6，恢复5到6；保留旧快照、预算/保留规则和原删除/隔离屏障。切换后取得新的schema6最新控制检查点；不明/不兼容检查点继续拒绝或使用既有缺检查点的保守隔离流程，不降低“最新”证明标准。quarantine.sql逻辑不改。

重要回滚约束：旧c522a57入口要求云schema恰为5。应用0006之后不能直接切回该旧二进制；回滚邮件使用新schema6兼容版本的mail_disabled=true，保留任务账本和全部新业务数据，不降级数据库、不覆盖在线库、不重放unknown。执行迁移前先备份，若迁移事务失败则保持旧schema5/旧服务，不继续启用邮件。

## 预计修改文件

| 文件 | 有限改动 |
| --- | --- |
| cloud/auth/src/server/email/smtp.rs（新增） | 独立Aliyun SMTP适配器、固定端点/TLS/安全结果分类 |
| cloud/auth/src/server/email.rs | 共用账本/模板、供应商枚举、有限发送器选择；保留两种HTTP适配 |
| cloud/auth/src/service.rs | 根据provider解析所需Broker引用、保留关闭状态；无密码配置项 |
| cloud/auth/Cargo.toml、Cargo.lock | 精确SMTP依赖/必要合成TLS测试依赖，核对无关依赖无升级 |
| cloud/postgres/migrations/0006_aliyun_directmail.sql（新增） | 邮件provider约束与schema6原子登记 |
| cloud/core/src/lib.rs | 云版本/迁移常量与对应测试，不改协议/权限DTO |
| cloud/postgres/operations/backup.py | schema4/5/6识别与恢复迁移，保留现有隔离/删除逻辑 |
| cloud/auth/tests/directmail.rs（新增）、cloud/auth/tests/operations.rs | TLS模拟SMTP、API流程/账本/入口/旧适配回归 |
| scripts/i2-auth.py、scripts/i7-operations.py | 新迁移与断言、SMTP测试入口/源码指纹/报告；复用现有CI job |
| 操作说明、本方案、HANDOFF及新的证据文件 | 安全配置、切换/回滚、实际验证与限制；旧证据不重写 |

预计不需改变.github/workflows/ci.yml：既有数据库job已执行上述脚本；若需调整，只登记为新增SMTP检查所必需的入口，不更改其他job。账户流程源码、账户权限DDL、Chat/Memory/Host源码不在预计修改名单；发现明确相关依赖后须先报告。

## 自动化测试与通过条件

1. 隔离loopback隐式TLS SMTP fixture，全部合成发件人/收件人/密码，测试CA仅test-auth可用，生产构建无任意端点或证书绕过入口。TLS成功后才允许AUTH；拒绝不可信、过期、错主机名证书/明文服务，断言失败时无AUTH或DATA泄露。
2. 验证AUTH/envelope/From一致、UTF-8主题正文、稳定Message-ID、点转义/CRLF注入拒绝；SMTP AUTH失败、RCPT/DATA 4xx/5xx、最终250、接收DATA后丢失答复、迟到答复、断线/超时/取消/进程重启、接受后的断线及落库失败。
3. 真实PG下同任务/并发只有一个物理发送、payload/provider冲突、unknown不重发、全局跨供应商预算/频率/90%与100%边界、既有抑制不绕过；没有成功送达证据时只accepted/unknown，回调401且不能伪造状态。状态表和日志无SMTP密码、明文验证码/收件正文。
4. 用模拟SMTP捕获合成验证码，经真实HTTP执行注册→验证→登录→找回→重设密码→新密码登录；过期/重放/错误验证码、旧密码/旧会话规则按原契约检查。代码、nonce、会话/密码不写入公开报告。不新增GUI。
5. 验证旧配置/Brevo/Resend认证回调与邮件关闭模式、0006事务失败完整回滚、schema5→6保留全部邮件/数据/RLS、旧4/5备份前向恢复6与最新B/ACL/认证隔离屏障；两个服务角色仍无额外权限。
6. 构建前检查盘/内存/缓存，本机满足条件时先定向Rust测试、fmt/Clippy和真实隔离PG/SMTP验证；当前开发机余约2.6GB需重查，不删除有效缓存。不足则GitHub Actions。提交后必须核验精确SHA的全部相关CI及原始日志/指纹，失败不授通过；先开发封存，再等待生产切换授权。

## 生产切换步骤（未执行，需另行确认）

1. 用户确认生产切换，并指定自己控制的测试收件邮箱、注册/找回两次最小测试和现有日/月预算；核对SMTP区域、官方地址/已验域名状态。用户此前单封测试成功不等于CAIdex两流程通过。
2. 只读核对现有运行提交、配置、私有目录、角色与备份；必要时先保持mail_disabled=true。制作并校验迁移前本机备份，安装经精确CI通过的server-only新release，保留旧release。
3. 用户在服务器用隐藏输入/既有安全秘密源将SMTP密码通过标准输入写入Broker；只输出保存/存在状态，不将密码提供给本会话或放shell历史。引用命令为：

   ```sh
   sudo -u caidex /opt/caidex/releases/NEW_SHA/bin/caidex credentials set \
     --owner caidex-service --provider account-service --profile aliyun-smtp \
     --store file --directory /var/lib/caidex/secrets --kind client-secret --stdin
   ```

   上述为接收秘密标准输入的命令，不在终端直接明文键入秘密；实施时提供完整隐藏输入步骤。已有pepper/其他供应商凭据不轮换、不删除。
4. 应用0006、切换兼容的新应用与备份工具，仍关闭邮件，核验schema/权限/备份/隔离恢复；仅做TLS和SMTP AUTH/NOOP连通认证预检查，不发邮件。若失败，关闭模式保持，不自动回退供应商。
5. 原子更新非秘密配置provider=aliyun_directmail、from=no-reply@caidex.cn，保留已确认预算，最后mail_disabled=false并重启；确认无真实密码日志、无未认证Webhook。新备份与控制检查点必须正确记载schema6。
6. 只向指定邮箱执行真实注册/验证/登录与找回/重设/登录，用户在实际收件箱取得验证码并安全输入；核对收到时间/必要邮件头与DB的accepted/unknown记录，不能把SMTP250代收件箱验收。除明确授权的这些步骤外不额外发信；安全通知若属于流程必要发送，也须在切换确认中说明并计入预算。
7. 问题时原子恢复mail_disabled=true并重启当前兼容release，不改账户/资料/epoch，不退预算/重发unknown、不降级或恢复覆盖在线库。记录实际结果/限制，封存后停止，I-7与部署独立审计仍须独立进行，不启动J。

确认点：批准以上邮件相关源码、最小schema/备份兼容配套、模拟SMTP/Rust测试和精确CI；这次确认不自动批准生产迁移/改凭据/启邮件/真实发信，生产切换仍另行等待用户明确确认。
