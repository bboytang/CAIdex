# I-3 独立 Chat 与本地缓存：设计与验收

2026-10-11：用户确认 I-2 独立审计通过并明确批准开发 I-3。基线 main 6176529dc8fca874db878a881028f1e49953881b；I-2 功能 951f92e、精确 CI 38135955674。确认来源为用户，本实施会话不冒充独立审计。

## 目标与有限范围

新增独立共享 Rust Chat 核心：复用 ModelProvider/Router 的 Responses 流，不启动 Runtime/Host，不声明或执行工具。提供 SQLite 版本化本地缓存，游客与可信 issuer/user_id 各自分库，项目在账户库内分 conversation；正文、附件、草稿、待完成轮次和 outbox 均随原归属。模型绑定在切账户时清空，Bearer/ApiKey 必须属于当前本机 Chat namespace，拒绝 Host owner。

消息按父消息形成不可变 DAG：并发请求/编辑产生兄弟分支，不覆盖旧消息；Provider 原生签名、推理 carrier、工具输出不进入可同步正文。流式文本暂存轮次草稿，终态原子追加 assistant 消息及本地 outbox；取消、断流、切账户或进程重启封为 interrupted，不重发未知模型 POST。完成状态复用云端 complete/interrupted，具体结束原因单独表达。

附件作为有界 SQLite BLOB 与元数据，绑定 conversation，与消息一起事务保存引用；模型发送只包含调用者明确选择的同 conversation 附件。请求/响应文本、历史长度、分页、单附件和库大小有界。建立本地变更 outbox，不启动网络同步、不把 pending 当上传许可。

复用已有 Unix/Windows 私有目录保护实现，必要时抽到共享凭据基础设施供 Host 和 Chat 同用；保持 Host 行为与审批架构。账户确认仅增加非秘密身份 getter，不改变 token 信任/存储。

## 非目标与依赖

I-4 Memory Engine/检索/Embedding、I-5 云 Chat 同步/控制/cursor/删除协议、I-6 Memory 同步、I-7 运维，以及 J/M/P 正式客户端不属于本轮。不给账户登录增加项目执行权限；不复制 Host Key、不读取真实用户 Key、不调用商业 API/真实邮件、不生产部署。PostgreSQL 0001/0002 不重写，本轮没有云端数据通道，因此不添加假同步 API。

依赖 I-1 scope/不可变消息契约、I-2 信任身份与账户隔离、F/G ModelProvider/Broker、H 私有目录模式；SQLite 为本地缓存，独立于 Host journal。普通 Chat 的项目标记仅用于本地分区，不宣称服务器项目 ACL 授权。

## 验收与停止

GitHub 三平台执行 workspace fmt/Clippy/测试及既有固定 Runtime 回归；新测试覆盖不可变分支、附件跨会话拒绝、账户/游客隔离、错误与取消/迟到回复、重启不重发、事务失败不丢旧数据、持久 outbox、Host 凭据拒绝及输出 carrier/工具隔离。内部可运行演示使用本地合成 Responses HTTP 服务与既有 Custom Adapter，经真实流收发/附件/切账户/重启，输出可核对 JSON；不当商业 Live/正式 CLI/GUI/iOS 平台验收。

通过条件：上述功能、相关测试和精确提交 CI 实际成功，最小 diff 已检查，源码/原始日志/可复现演示与限制封存。达到后停止功能开发，等待 I-3 独立审计及用户下一里程碑批准；失败须修复或明确记录阻断，不以排队/推送当通过。

## 实现与运行

共享 crate `caidex-chat` 位于 `chat/core`。`Namespace::account`从I-2可信确认身份派生分区；`offline_account`只选择本地缓存，不授云认证/同步许可。issuer+user_id+测试域以SHA256分目录，游客独立；每库校验自己的namespace、application_id CAIC与schema版本1，拒绝未知库/未来版本、符号链接及Unix硬链接，独占文件锁防两进程重复恢复。账户切换取消旧流、保存旧分区中断消息、清空模型绑定，迟到文本/结果凭generation和operation双重拒绝；登出选择游客，不把账户缓存当游客。

本地SQLite与Host journal分开，initial迁移见`chat/core/src/schema.sql`；消息/会话/附件/outbox在同一数据库事务中提交，user正文只插入一次，assistant正文在结束时插入。消息更新/删除由SQLite trigger拒绝。每条消息有UUID、conversation、父消息、角色、内容、UTC毫秒时间及complete/interrupted状态/具体结束原因；sequence用于本地分页，不以客户端时间当服务器顺序。编辑以旧消息的父节点为base另发新消息，并发兄弟分支都保留；调用者选择哪条branch进入下次上下文。

每message序列化正文≤1MiB、历史≤128条/文本合计≤1MiB、附件引用≤4条/不重复、单附件及单次历史附件总量≤8MiB、分页1～1000、待运行轮次≤16、总deadline120秒、数据库65536个4096字节页（约256MiB）。Provider还执行自身更严格的请求/能力限制，附件上限不表示每个Provider都能接收8MiB文件。支持image/png/jpeg、application/pdf和text/plain元数据；附件内容不代实际格式/病毒认证。

流式draft逐检查点落盘，最终只保存assistant可见text/refusal；推理/签名、headers、原始response及工具记录不写入消息/outbox。工具输出被拒为invalid，永不运行。create/attach/send只产生本地待同步变更引用，不包含上传认证、云ack或内存同步许可；operation复用拒绝再推理。崩溃恢复封存pending草稿为interrupted，取消/失败/断流保留可见部分；不自动重试未知模型POST。磁盘写失败回滚事务，旧消息/附件/outbox保留，pending轮次留待存储恢复后处理。

已有Unix0700/Windows单SID继承ACL实现移至`credentials/core/src/private_directory.rs`，Host调用共享实现；Chat不依赖Host/Runtime。I-2只新增非秘密身份getter；执行凭据仍在既有Broker/Adapter内解析。Chat模型Bearer/ApiKey owner必须等于当前namespace的`credential_owner()`，配置者在本机单独建立该Broker。模型配置跨账户不会继承，Host owner不能绑定。

本地或GitHub执行：

```sh
cargo test -p caidex-chat --locked
python3 scripts/i3-chat.py --output i3-chat.json
```

演示脚本创建自己的临时私有目录和loopback HTTP SSE服务，运行`cargo run -p caidex-chat --example local_chat`，复用真正CustomResponsesProvider+ModelRouter；四个请求演示游客附件收发/并发分支、A收发、B无法读取A及必须另配模型、游客原数据保留和A重启恢复。合成账户仅演示本地分区，不冒充真实账户登录或云Chat同步。成功输出JSON：guest_messages=6、account_a_messages=2、http_requests=4、outbox_uploads=0及隔离/恢复布尔检查通过。异常退出打印安全错误，测试客户端和临时目录由脚本finally清理；不会清理用户缓存/全局Codex。

## 限制与后续归属

本地文件是受OS权限保护的明文，包括BLOB附件；同一OS用户/管理员仍可读，不宣称加密/E2EE。文件系统故障/满盘可能使保存和pending封存失败，未成功事务不授已保存/已结束；清出合法空间后重新打开恢复，不能删除用户缓存当修复。内部Core暂按本地账户和conversation项目标记分区，不授云项目ACL。网络同步/Chat控制/ack/cursor/tombstone/删除载荷清理归I-5，Memory Engine归I-4；正式平台UI及CLI另按既定阶段。macOS Rust不代iOS Simulator/Archive/真机。真实模型及附件兼容性仍依F/G-Live由用户最终自验。

## 最终验证与停止状态

不可变功能源码[`bc421f01993531c378da852b7a63be207658b91c`](https://github.com/bboytang/CAIdex/commit/bc421f01993531c378da852b7a63be207658b91c)，[精确CI38138552078](https://github.com/bboytang/CAIdex/actions/runs/38138552078)四job实际checkout该完整SHA并全部成功。封存提交仅文档/证据，不以封存HEAD冒充功能CI源码。

| 环境/job | workspace通过/失败/ignored | 新Chat测试（计入workspace） | 固定Runtime额外 | 其他证据 |
| --- | --- | --- | --- | --- |
| Linux/114467405810 | 696/0/83 | 7 | 81/0 | Host42（已计入workspace），原生凭据附加1，Chat/H-1/2/3演示成功 |
| macOS/114467405812 | 695/0/81 | 7 | 80/0 | Host42（已计入workspace），Chat/H-1/2/3演示成功 |
| Windows/114467405820 | 689/0/81 | 6 | 80/0 | Host41（已计入workspace），额外Host lib4，Windows ACL边界及Chat/H-1/2/3演示成功 |
| 数据库/114467405723 | 不计入workspace | 无 | 无 | 真PG17.10/vector0.8.2，I-1 118具名检查/0，I-2 110具名检查/0、共享认证5测试 |

Linux/macOS多一项Unix权限/symlink/hardlink/Git路径检查；Windows按实际ACL路径和Host过程边界验，不把Unix项算通过。所有平台fmt/Clippy/授权JS语法、schema/doctor通过；ignored不是通过。I-2联合集成Rust是一个测试，110具名断言组不能当110个Rust测试。SQLite容量故障由max_page_count注入并断言旧消息/outbox不变，不填满runner物理磁盘，也不冒充I-7 VPS磁盘增长/恢复压测。

三平台演示原输出逐份封存：4个真实loopback HTTP请求、6条游客/2条A消息、附件和并发分支可观察，B与游客隔离、切换清模型、重启保留原归属，outbox_uploads=0。运行脚本只能证明内部客户端+共享Core，不代正式CLI/GUI/云同步。

[四job/各步骤/计数/演示/源码指纹及原日志](evidence/i3-ci.json)、[同提交真实认证回归](evidence/i3-auth-regression.json)、[两轮开发失败与八份原日志](evidence/i3-development-failed.json)是完整证据索引。首轮38138193270在云Id/SQL整数接口编译失败；第二轮38138337723因测试Error glob歧义失败，已修正；这些失败不授I-3通过。失败及最终原日志均校验SHA256，本轮未发现ENOSPC/No space left on device。旧全部验收证据字节不变，外部依赖版本/source/checksum不变，Provider Adapter/固定Runtime/PostgreSQL源码不变；Host仅共享私有目录封装，Windows生产ACL脚本与I-2基线逐字一致。

本地仅格式/脚本语法/差异/文档链接/证据及指纹检查，无完整重编译。本机盘39GiB/已用35GiB/可用3.7GiB（91%），未清理用户/全局Codex/有效缓存。构建和完整回归由GitHub执行。

只读复验归档（仓库根执行，不调用商业API）：

```sh
python3 - <<'PYVERIFY'
import gzip, hashlib, json, subprocess
from pathlib import Path
base = Path('docs/evidence')
report = json.loads((base / 'i3-ci.json').read_text())
assert report['conclusion'] == 'success' and len(report['jobs']) == 4
for path, digest in report['source_sha256'].items():
    raw = subprocess.check_output(['git', 'show', report['source_sha'] + ':' + path])
    assert hashlib.sha256(raw).hexdigest() == digest, path
for job in report['jobs']:
    compressed = (base / job['log']).read_bytes()
    assert job['conclusion'] == 'success' and job['checkout_sha'] == report['source_sha']
    assert hashlib.sha256(compressed).hexdigest() == job['gzip_sha256']
    assert hashlib.sha256(gzip.decompress(compressed)).hexdigest() == job['raw_sha256']
print('I-3 immutable source and four original job logs verified')
PYVERIFY
```

当前开发通过条件已满足，已停止新增功能。I-3独立审计未由实施会话授予；由独立新会话/其他模型只读审计上述不可变源码、完整调用路径、实际证据和限制。审计阻断在当前范围修复并复验；用户确认I-3通过且明确批准后才进入I-4。
