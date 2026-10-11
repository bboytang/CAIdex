# CAIdex I-1：账户、权限与数据库

2026-10-11用户确认H-3独立审计通过并批准下一步，按[V3既定I内部顺序](CAIdex-实施计划-V3.md)实施I-1。起点main `db854bbe6b87edfda7dcd733a9c7f960032d5a65`，HEAD/origin/main/GitHub main一致、工作区干净；构建前可用1.7GiB。审计通过是用户告知，不是本实施会话自授。

## 有限目标与计划

目标：正式PostgreSQL+pgvector版本化迁移、设计第5节全部17逻辑实体、不可变user_id/作用域外键、项目ACL、最小权限服务角色与RLS；提供可重建隔离数据库及跨账户/项目/伪上下文拒绝的实际证据。共享Rust契约仅承载本里程碑用到的ID、scope和同步控制DTO，不接模型/Host执行。

步骤：①核对正式设计及数据库官方权限语义，记录范围；②实现事务迁移/角色/约束及窄共享DTO；③隔离真实PostgreSQL测试覆盖默认拒绝、认证上下文不可伪造、项目授予/撤销、tenant/scope外键、不可变记录及向量过滤；④fmt/Clippy/相关Rust与数据库测试，精确源码三平台RustCI和Linux数据库CI；⑤封存不可变SHA、日志/哈希、重建步骤及限制，停止等待独立审计。

依赖：既有H/固定Runtime/ModelProvider/Broker保持，正式Account/Memory设计及CLI身份域契约；测试使用官方pgvector/PostgreSQL镜像且按digest锁定。数据库管理员只负责建立隔离库/迁移及合成fixture，普通查询使用独立非owner/non-superuser/NOBYPASSRLS服务角色。生产认证尚未实现，合成session只验证数据库授权边界，不冒充I-2登录。

非目标：I-2及后续认证/Passkey/EmailSender/PKCE/Device实际服务、Chat/Memory Engine/同步状态机/备份恢复/GUI/完整CLI；商业API/用户Key/正式部署；数据库不是Host journal或第二套Agent。不改变自动记忆未定默认值。

通过条件：17实体及附件/向量空间契约可从空库事务创建；迁移版本不重复应用、失败无部分提交；全部私有表FORCE RLS，服务角色无DDL/TRUNCATE/角色提升/认证材料读写；缺上下文、伪user_id/无效会话、撤销/过期会话及删除账户拒绝；同连接事务结束后不串租户；授权项目读写按角色生效，确认撤销后新事务拒绝；个人/项目及跨用户引用失败；ID/正文revision不可变；新账户记忆同步默认关且不可读写云记忆；SQL关键词/精确向量候选先权限过滤。相关检查及最终精确CI通过后停止，不授I-1独立审计通过，不启动I-2。

停止条件：重大架构/明确产品语义冲突先报告；环境无法真实执行数据库检查则记录阻断，不用SQLite或静态SQL检查代验。

## 进度与证据

实现入口：`cloud/postgres/bootstrap.sql`、`migrations/0001_account_scope.sql`、`tests/fixture.sql`、`scripts/i1-database.py`及共享`cloud/core`。正式17逻辑实体另加附件/Embedding空间，合计19私有表及1迁移表；19私有表全部FORCE RLS。UUID主键/身份归属不可由应用改写，组合FK含user_id和生成scope_key，项目实体绑定项目所有者，作者单独记录且插入须为当前actor。共享项目游标分别保存数据所有者与session_user_id，组合FK绑定真实会话用户；只允许当前会话用户读写自己的游标，读者可记录阅读位置但不获数据写权。消息/revision/来源/向量正文没有UPDATE/DELETE权限。向量仅返回当前active且在有效期内的revision及ready状态。

迁移由独立管理员执行，`caidex_owner`拥有DDL，`caidex_guard`无登录/非owner/无BYPASSRLS，只能读会话/用户/项目ACL以执行3个固定search_path的窄SECURITY DEFINER函数；`caidex_app`无其角色成员资格。用户身份/认证材料对普通数据角色不开放；会话仅授权非秘密展示列。未来认证角色与认证流程归I-2，不允许用app角色创建认证会话。

`bind_session($1)`在READ COMMITTED事务内绑定opaque会话；RLS按服务器保存的SHA256 access摘要、到期/撤销、固定数据audience和活动账户解析身份，不使用请求/GUC中的user_id。直接伪造GUC也不能绕过验证。事务COMMIT/ROLLBACK后绑定清除；重复读/串行化旧快照拒绝。I-2服务须从已验证官方认证上下文传入令牌，使用参数绑定、不记录SQL参数/令牌，异常ROLLBACK并清理连接。SQL错误必须由未来API统一转安全类别，不能透传FK/唯一约束细节。撤销阻止确认后新查询；已经开始的语句/已交付数据不能回收，API须短事务及有界超时。

新账户Memory Sync为false，app不能改控制状态；记忆云数据读写还需当前账户及项目所有者enabled。共享DTO区分权威控制、待确认停止与本地范围许可。这里只是存储/权限与DTO边界，首次范围许可、CAS/epoch/关闭A/B、旧job提交/防复活、数据变更与sync_event同事务的业务方法仍由I-4/5/6完成；不能把字段和底层DML当同步状态机已验。Chat开关存储默认false，本里程碑不执行Chat同步。

本地118项数据库检查及2项Rust契约测试通过；fmt和Clippy workspace/all-targets/locked通过，全仓回归已通过，workspace684/0/83（通过/失败/忽略，ignored不当通过），最终SHA与CI如下。数据库实测PostgreSQL17.10、pgvector0.8.2；镜像digest固定。最终功能源码[`bb9c0a18493b7e55debcb961d187b7a196a52eef`](https://github.com/bboytang/CAIdex/commit/bb9c0a18493b7e55debcb961d187b7a196a52eef)已push main；[精确CI38109361127](https://github.com/bboytang/CAIdex/actions/runs/38109361127)四job全部success，原始checkout逐一为该完整SHA，所有必要step成功。实际计数如下（通过/失败/忽略）：

| 平台 / job | workspace | Host（已计入workspace） | 固定Runtime（单独显式运行） |
| --- | --- | --- | --- |
| Linux / 114381512936 | 684/0/83 | 42/0/0 | 81/0/0 |
| Windows / 114381512776 | 678/0/81 | 41/0/0 | 80/0/0 |
| macOS / 114381512862 | 683/0/81 | 42/0/0 | 80/0/0 |

各平台新cloud/core两项均一次成功；fmt/Clippy/schema/doctor/H-1/2/3真实进程演示全部成功。Linux凭据附加1/0/0、Windows额外Host lib4/0/0另记，ignored不计通过。数据库job114381512893实测118/0/0，PostgreSQL17.10/pgvector0.8.2，与本地精确功能SHA118项结果一致；初始空库失败回滚/重放拒绝/第二独立数据库重建均实测，不用静态SQL声明代替。

[完整CI逐job/step/原始日志与实际checkout/计数/源码哈希](evidence/i1-ci.json)、[精确SHA本地数据库全部断言](evidence/i1-database-local.json)、[本地检查与日志/源码指纹索引](evidence/i1-local.json)、[开发失败及修正原日志](evidence/i1-local-failed.json)已封存。所有gzip原始内容与压缩文件各有SHA256。全仓本地684/0/83在最后游标/约束修改前运行；其后精确最终core与数据库定向复验通过，精确最终CI重新跑全仓/固定Runtime，不将旧本地结果冒充最终源码完整回归。本次封存只有证据/文档，与功能SHA的功能/测试/依赖/脚本/workflow无差异。

开发验证及精确CI完成，无当前通过条件对应的剩余阻断。停止等待I-1独立只读审计，不自行授审计通过或启动I-2。

## 独立运行

取得本里程碑最终功能SHA后，在Linux及可访问本机Docker socket的环境执行：

```bash
df -h .
docker pull pgvector/pgvector@sha256:feb68f4f15446397d8cac7f4fe48fe4586de83160d1fc48b46283312d1a33966
python3 scripts/i1-database.py --output /新的路径/i1-database.json
cargo test -p caidex-cloud-core --locked
```

脚本新建随机命名、无网络/无映射端口、512MiB/2CPU、256MiB私有tmpfs测试容器；trust仅在该无网络测试容器使用，不是部署配置。管理员建角色/迁移/合成A/B/C会话与私有项目，真实普通LOGIN角色继承app来读写。按输出操作检查默认拒绝→A/B独立正文/附件/来源/向量/jobs→读者/写者授权→撤销→连接换租户→会话过期/删除账户→第二空库重建。预期全部`passed=true`、最终`failed=0`，输出JSON含完整检查/源码SHA256和版本；SQL拒绝按实际SQLSTATE断言，写出新文件不覆盖。脚本只移除自己创建的容器，镜像保留，可重复运行；不访问用户数据库/Key/Host，不发送邮件或模型请求。

## 限制与待验

- 本里程碑没有登录/Passkey/密码验证/refresh轮换/Email/HTTP服务，fixture摘要不是真实认证通过。I-2必须验证真实认证与DB上下文接入；普通数据角色不能签发/延长会话或自行开启同步。
- PostgreSQL库与应用查询契约已运行；本地SQLite缓存/账户文件隔离、完整Memory Engine/同步/恢复/GUI/CLI仍待各对应里程碑。附件这里只验归属引用/大小/hash，没有对象存储上传下载；语义向量是3维合成数值，未调用Embedding Provider。
- RLS和组合FK共同防跨scope正文引用；不可猜ID不替代认证。FK/唯一约束存在性侧信道须未来API统一隐藏。共享DTO不携带Token/Key/Host授权，仍需API输入长度与具体业务校验，DTO反序列化不是上传授权。
- PostgreSQL17+pgvector0.8.2是锁定测试基线，未验证其他数据库版本、真实Windows数据库/生产部署/负载/容量/备份容灾或邮件网络。I-1数据库服务针对Linux，Windows/macOS仅验证共享Rust和既有Runtime，不冒称iOS验收。
- 关键检查使用合成数据并实际连接PostgreSQL；测试计数不是独立安全审计、认证质量或生产吞吐证据。用户Key/商业API/生产部署未执行，F/G-Live保持待验。

参考：[PostgreSQL17 RLS与权限边界](https://www.postgresql.org/docs/17/ddl-rowsecurity.html)、[pgvector官方说明](https://github.com/pgvector/pgvector)。


## 封存证据只读核对

在仓库根目录运行以下检查，核对原日志、源码指纹和实际计数。哈希/计数检查不等于独立审计；仍须结合SQL、权限调用链及逐条负例原日志。

```python
from pathlib import Path
import gzip, hashlib, json, subprocess
root = Path("docs/evidence")
ci = json.loads((root / "i1-ci.json").read_text())
assert ci["source_sha"] == "bb9c0a18493b7e55debcb961d187b7a196a52eef"
assert ci["conclusion"] == "success" and len(ci["jobs"]) == 4
for job in ci["jobs"]:
    packed = (root / job["log"]).read_bytes()
    raw = gzip.decompress(packed)
    assert hashlib.sha256(packed).hexdigest() == job["gzip_sha256"]
    assert hashlib.sha256(raw).hexdigest() == job["raw_sha256"]
    assert job["checkout_sha"] == ci["source_sha"]
    assert ci["source_sha"] in raw.decode()
    if "database_summary" in job:
        assert job["database_summary"]["passed"] == 118
        assert job["database_summary"]["failed"] == 0
        assert len(job["database_checks"]) == 118
        assert all(c["passed"] for c in job["database_checks"])
    else:
        assert len(job["cloud_core_pass_names"]) == 2
        assert len(job["host_pass_names"]) == job["host"]["passed"]
        assert job["workspace"]["failed"] == job["fixed_runtime"]["failed"] == 0
for index in ["i1-local.json", "i1-local-failed.json"]:
    data = json.loads((root / index).read_text())
    for item in data.get("logs", data).values():
        packed = (root / item["log"]).read_bytes()
        assert hashlib.sha256(packed).hexdigest() == item["gzip_sha256"]
        assert hashlib.sha256(gzip.decompress(packed)).hexdigest() == item["raw_sha256"]
local = json.loads((root / "i1-local.json").read_text())
for name, expected in local["functional_source_sha256"].items():
    source = subprocess.check_output(["git", "show", ci["source_sha"] + ":" + name])
    assert hashlib.sha256(source).hexdigest() == expected
    assert hashlib.sha256(Path(name).read_bytes()).hexdigest() == expected
print("I-1 sealed source and success/failure evidence verified")
```
