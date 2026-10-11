# I-7 本机运行与恢复操作说明

这是隔离开发验证的操作接口；未安装服务/定时任务，未部署VPS，也未发送真实邮件。授权、证据与未验边界见 [I-7验收](../../../docs/CAIdex-I7-运行与恢复-设计与验收-V1.md)。

## 独立复现

Linux x86_64、Rust 1.99.0、Python3、Docker、PG17客户端；保留项目固定工具链。脚本只创建自己的临时容器/目录，使用固定PG17.10/pgvector0.8.2镜像，无外部邮件或模型调用。必须能访问Docker daemon、cgroup v2与宿主动态库 `/usr/lib/x86_64-linux-gnu`；编译不计资源曲线，PG/HTTP/服务进程在同一2CPU/2GiB cgroup内运行。

```sh
python3 scripts/i7-operations.py --output /absolute/new/i7-report.json
python3 scripts/i2-auth.py --output /absolute/new/account-report.json
python3 scripts/i5-chat-sync.py --output /absolute/new/chat-report.json
python3 scripts/i6-memory-sync.py --output /absolute/new/memory-report.json
```

输出不得已存在；报告包含精确HEAD/源码指纹、具名断言、SLO、资源曲线、恢复点/耗时及隔离标志。预期末尾 `I7_RESULT` failures=0，任何失败退出非零。正式GUI/CLI、iOS、真实邮件与生产容量不由这个测试授通过。

## 服务入口与秘密

```sh
cargo build --release -p caidex-auth --features server --locked
./target/release/caidex-auth-service /absolute/private/configuration
```

配置目录必须Git外、绝对规范路径、0700、当前用户拥有；`config.json`为0600普通单链接文件。配置示例：

```json
{
  "port": 8080,
  "issuer": "https://accounts.example.test/",
  "rp_id": "accounts.example.test",
  "auth_db": "host=/var/run/postgresql user=caidex_auth_service dbname=caidex",
  "data_db": "host=/var/run/postgresql user=caidex_data_service dbname=caidex",
  "secrets": "/absolute/private/service-secrets",
  "provider": "brevo",
  "from": "account@example.test",
  "daily_budget": 100,
  "monthly_budget": 1000,
  "database_volume": "/absolute/postgres-data-volume"
}
```

额度仅示例，不是供应商免费额度承诺；上线前从官方核对并配置实际预算。服务仅绑定127.0.0.1，issuer/RP必须是同一HTTPS域名。两个数据库登录角色分别只继承既有 `caidex_auth` / `caidex_app`，无owner/SUPERUSER/BYPASSRLS/建库建角色/schema CREATE，使用本机Unix socket；配置拒绝数据库密码。应由数据库管理员预先安装bootstrap与0001～0005事务迁移，配置本机peer/受限认证。不得把postgres管理员给HTTP服务。

用既有 `caidex credentials set --owner caidex-service --provider account-service --profile PROFILE --store file --directory /absolute/private/service-secrets --kind KIND --stdin` 从标准输入提供以下三项，不能把秘密放命令参数、JSON、日志、源码或聊天：

| PROFILE | KIND | 内容 |
|---|---|---|
| device-code-pepper | client-secret | 随机32字节base64url无padding，重启保持不变 |
| mail-api | api-key | 当前明确选定供应商Key |
| mail-webhook | client-secret | Brevo独立bearer，或Resend `whsec_`签名Key |

入口启动验证磁盘预留128MiB，低于预留拒绝数据写请求；释放空间后恢复。读取最少控制、停止/删除清理按既有控制面继续，数据库自身ENOSPC仍可能使清理暂不可用。生产服务3个PG连接、单worker和有限请求/超时。反向代理/TLS/域名、公开流量IP限流、进程监管/告警、供应商轮换与真实部署另验；当前按实际TCP peer计IP，不接受任意转发头。

`POST /v1/mail/events` 最多64KiB/5秒，读取body前单worker准入；Brevo验证独立bearer，Resend验证原始body的Svix HMAC/id/timestamp（±300秒）。供应商需配置对应签名/认证，不能公开无认证hook。只更新已匹配的供应商消息ID；硬退信/投诉跨供应商持久抑制，不能被迟到delivered撤销。

邮件记录只存任务/目的/收件hash/payload hash/供应商ID/状态/时间；尝试在HTTP前持久计费预算，全局UTC日/月累计，90%停止非必要通知，100%拒绝。超时/不明结果保留unknown，拒绝同任务重发或自动换供应商；没有持久明文验证码队列，进程崩溃可能丢失一次未完成发送。没有自动unknown对账；已失去消息ID的回调不能猜测关联。用户显式重试依赖既有新挑战和限流。预算/抑制表当前不GC，长期增长需监控，不承诺50GB生产容量。

## 每日备份与淘汰

管理员身份、同机PG17 `psql/pg_dump/pg_restore`、已存在的私有父目录。只备份caidex schema，包含Chat附件BLOB、记忆、控制、ACL/认证及永久删除屏障；角色定义不在dump内，恢复目标集群必须已安装四个既有专用角色与固定扩展。

```sh
umask 077
python3 cloud/postgres/operations/backup.py --socket /var/run/postgresql --database caidex prune /absolute/private/backups
python3 cloud/postgres/operations/backup.py --socket /var/run/postgresql --database caidex backup /absolute/private/backups
```

部署获批后可用系统定时任务每日执行这两个命令（先prune，即使新备份失败仍淘汰）；本交付不安装任务。任务非零退出必须告警，不用重试掩盖预算或损坏。规则固定7天/7份/**5,000,000,000字节**，另外保留256MiB磁盘余量；锁拒绝并发备份，完整数据库大小用于保守准入，子进程文件大小硬限制拒绝预算外增长。SHA256/字节数、schema/protocol、PG/vector、源码HEAD、快照开始到结束窗口和dump耗时写manifest；成功才发布。目录整体计预算，未知文件保留；`snapshot-<epoch>-<uuid>.*` 为工具保留名字，满7天连中断partial/orphan也淘汰。勿在该名字下放用户文件。

B“停止并删除”完成后在线正文已清除，旧明文备份可能继续含已删除资料，最多7天且依赖淘汰任务实际成功。维护失败/未运行时不能宣称已删除旧备份；报警并恢复维护。私有权限不等于加密。整台VPS/同一磁盘全损可能连数据库和备份一起丢失；未配置异地或第三方备份。

每日策略正常工作时恢复数据可能缺少最多约一个备份周期，备份失败则更久；这里是计划窗口，不是生产RPO保证。schema3及更旧、向后降级、新集群角色迁移不在此工具支持范围。

## 隔离恢复与删除屏障

在线库尚可读时，先取得尽可能新的独立控制/身份/ACL/epoch/墓碑检查点。输出只含安全元数据，不含Chat/Memory正文或原始密码/令牌；身份hash等仍敏感，使用私有目录并按操作目的保留。失败时不能假设最新状态。

```sh
python3 cloud/postgres/operations/backup.py --socket /var/run/postgresql --database caidex checkpoint /absolute/private/recovery/checkpoint.json
python3 cloud/postgres/operations/backup.py --socket /var/run/postgresql --database caidex restore /absolute/private/backups/snapshot-EPOCH-UUID.json --target caidex_restore_review --checkpoint /absolute/private/recovery/checkpoint.json
```

只新建 `caidex_restore_*`，创建时禁止全部连接、撤CONNECT后才允许管理员恢复；不覆盖或删除在线库，校验失败不释放目标。schema4备份向前迁移到5。检查点必须来自同一源集群/数据库、时间晚于备份完成；只能证明其标明时点，之后操作保持未知。匹配最新记录时重建已证明ACL、撤销不明或变更身份/凭据、合并永久墓碑并清除旧B正文/来源/向量；不误删其他所有者记忆或独立Chat。所有恢复均作废旧session/refresh/challenge/CLI授权、旧队列、开启许可与世代。

没有最新检查点时省略 `--checkpoint`：全部账户暂停、旧身份/凭据撤销、所有同步关闭，旧正文仅供管理员隔离核对。**两种恢复都撤销PUBLIC/app/auth CONNECT，绝不自动发布。** 本工具没有自动解隔离命令；在部署授权、身份与当前控制/ACL/删除记录审查、重新认证/显式开启/范围授权前，不得接回客户端或重用旧令牌。检查点不得从客户端自报数据替代。

报告明确backup_window、checkpoint_point、later_operations_known=false、quarantined=true、old_sessions_valid=false。实测恢复耗时属于合成1000向量/3附件的小库，不能外推实际50GB或全VPS损失。

## 本地选定Chat导出

Rust共享核心 `Chat::export_conversation(UUID)` 返回version1 JSON，只包含当前账户选定未删除Chat、完成/中断消息及分支、进行中草稿、该Chat附件（base64）；不导出session/token/outbox/上传许可，不下载云正文、不自动导入或重放。最多4096消息/16草稿/64附件/16MiB；超限或命中既有secret guard拒绝，不截断并假称完整。切账户/删除/秘密负例已测试。未注册任意正文秘密无法自动穷举，用户仍须审查所选内容；导出只含该设备实际本地资料，不等于全云备份。GUI/CLI文件保存交互由后续正式阶段接入。
