# I-4.1 本地 Memory Engine：设计与验收 V1

2026-10-11：用户确认 I-3 独立审计通过，并明确授权下一阶段 I-4。用户确认自动记忆首次默认关闭。实施会话不认领独立审计。用户随后确认拆分为 I-4.1 本地核心与 I-4.2 Host 接入：先封存并独立审计 I-4.1，再另行批准 I-4.2；不改变 V3 的正式 I 阶段或顺序。

目标：在 I-3 本地账户分区内实现个人/项目记忆、来源与不可变 revision、纠错与冲突、授权基础/语义检索；复用 ModelProvider/Router/Adapter/Broker，实现独立整合与 Embedding 角色、精确空间版本及 Provider 外发许可。本内部里程碑验证客户端执行端；Host 推理执行/独立授权恢复由 I-4.2 提供实际证据，不用来源读取 callback 或 Runtime 回归冒充 Host 记忆执行。

有限任务：版本化 SQLite 迁移；手工创建、查看、纠错、候选批准、删除及导出；来源新鲜度校验；CAS 冲突保留双方；删除来源墓碑防自动复活；自动开关与本地任务等待/取消/重启未知终态；按账户/项目/隐私授权检索；Embedding 契约和显式兼容端点；普通 Chat 有界记忆注入；合成模型/实际本地来源的复现及三平台精确 CI。

非目标：I-4.2 Host 记忆推理 RPC/独立 Memory 权限与远端执行（本轮未授权）、I-5 Chat 同步、I-6 Memory 同步、I-7 生产运维、正式 GUI/CLI、生产邮件/部署、商业模型与真实用户 Key、iOS 安装验收。项目本地授权不代云 ACL；Host 离线不能隐式转用客户端 Key 或云执行。本轮不新增 Host RPC 或权限，不修改固定 Codex 0.160.1 的工具/审批链。

依赖：I-1 数据定义与 RLS、I-2 身份、I-3 namespace/cache；Account/Memory/Cloud §§6–7 与 MEM-01～10；V3 I-4；固定 Facade/ModelProvider/Registry/Router/Gateway/Broker。旧 PostgreSQL 迁移与 I-3 schema 1 不修改，新本地 schema 2 事务迁移保留 Chat。

通过条件：无模型可手工保存/检索，整合与 Embedding 任务明确等待；来源与 AI 推断类型可见；原始 Chat/文件/Git/执行证据保持区分，新鲜度失效不能检索为事实；纠错 CAS 不最后写入覆盖；账户/项目撤权在读取和异步提交前生效；自动关闭停止提取/注入但手工 CRUD 可用；删除清理正文/向量/独占来源，旧来源不能自动复活；不同整合/Embedding/Chat 模型独立，精确空间版本与 revision 不混用；Key 归属和外发许可各自检查，已知凭据不入记忆/导出/模型正文；撤销/删除/切换/取消拒迟到结果，未知 POST 不重播；相关测试及精确 CI 通过，原始证据与限制可独立复现。

证据计划：定向 Rust 边界测试、合成 HTTP Embedding/整合及 Chat 注入复现；GitHub 三平台 workspace/fmt/Clippy、固定 Runtime/Host 及数据库认证回归。只本地静态/格式/差异检查，构建前检查磁盘，不清理有效缓存。只读独立审计前封存精确源码 SHA、CI 原始日志和试用步骤。

停止条件：达到上述条件后封存并停止功能开发，等待独立 I-4.1 审计及用户确认；I-4.2 须另行明确批准。发现当前条件内阻断则最小修复复验；不得启动 I-5。

当前状态：I-4.1 开发验证与证据封存完成，已停止新增功能，等待独立审计及用户确认。I-4.2 未批准；整个 I-4 与 MEM-10 未完成。

## 已确认的 Host 接口缺口与拆分

`runtime/host/src/service.rs:33` 的 Request 只包含观察/Runtime task/审批/客户端授权；`access.rs:13` 只有 Observe/Execute/Approve。普通记忆推理不应获得 Runtime 工具执行权限。用户已选择 I-4.1 / I-4.2 分别审计与批准；本轮不复用 Execute 偷授 Memory，不新增 Agent，不读/复制 Host Key。I-4.2 需提供独立 Memory 授权、Host 自有 Broker/Key、在线/离线/恢复/撤权、真实 Host RPC 与精确 CI。此次拆分保留 V3 I-4 的完整功能范围，I-4.1 完成不等于整个 I-4 完成。

## 本地实现与接口

`chat/core/src/memory` 为共享 Rust Memory Engine，借用现有 Chat 的账户分库、锁和 SQLite，Memory 表/作业/outbox 与 Chat 逻辑独立，与 Host journal 分开。`MemoryEngine` 从 `Chat::memory()` 获取；游客、可信 issuer/user_id/测试域隔离，归属由当前 Namespace 派生，模型不能指定另一账户。本地项目必须显式 `grant_project`；`revoke_project` 在读取/提交前生效，清对应外发许可、取消任务。此接口是应用本地权限边界，不提供云成员身份或服务端 ACL。

I-3 `schema.sql` 不变，新 `memory/schema2.sql` 在同一事务升级 schema 1→2，保留原 Chat。未知未来 schema 继续拒绝。新库自动记忆关闭；`set_automatic` 持久化本地设置，关闭取消进行中的记忆调用并停止自动排队/Chat 注入，手工 CRUD/基础搜索仍可用。启动将遗留 running 作业封为 unknown；不重发已提交模型 POST。schema 2 不能直接由旧 schema 1 二进制打开，不提供数据降级。

手工 `create/get/list/correct/approve/delete/export` 使用稳定 ID 与不可变 revision：纠错携带 base head，陈旧 base 追加冲突 revision、保留双方并置 disputed，搜索不把冲突当当前事实。用户明确选择 revision 后恢复 active；历史 parent/conflict 留痕，旧正文不当当前版本。AI 整合只能输出严格 `{text,tags}` 候选，kind 固定 ai_inference，用户批准后仍保留推断类型和来源；它不覆盖已批准的重要决定、不编造来源或提升权限。

来源分别为 User/Chat/File/Git/Execution/External。Chat 核对同账户 conversation/project、实际完成消息与正文 hash；自动采集只处理完成轮次的有界文本，附件解析未实现。其他来源由可信应用 `EvidenceResolver` 每次读取当前已授权证据，核对完整 bytes 的 SHA256 和真实摘录，不能传入“已验证”布尔值代替验证。文件/Git 来源测试实际改文件及创建/修改 Git 提交；Execution 来源测试使用合成证据文件，**不宣称本轮已连接真实 Host 记忆 RPC**。来源变化/不可用、到期、候选、争议、已删除及旧 head 不进入有效检索；详情 `source_current` 表达当前核验结果。

删除在事务内使记忆不可检索，清全部 revision 正文、摘录、向量及匹配来源/实体的作业引用，包括已结束作业；保留最少 revision 元数据、删除状态及 scope+来源指纹墓碑。旧来源不得再次自动提取，用户明确手工创建新记忆仍可操作。原始 Chat/文件/仓库/Host journal 不随记忆删除。`secure_delete=ON` 清 SQLite 已释放页中的载荷，不承诺 OS/备份/第三方收到的副本物理抹除。后来被 guard 识别为凭据的旧条目拒绝读取/导出，授权用户仍可按 ID 删除。

`search` 提供关键词、标签、时间及有效状态筛选；`semantic_search` 在 SQL 内按 scope 限定候选，再核对有效来源、外发许可及当前精确空间向量。个人/其他项目的向量不参与项目距离计算。空间 ID 包含真实端点/native model、model_revision、维度、preprocessing_version 与距离度量；同维度不同空间不混算，旧 revision 不返回。无模型/无当前空间向量/无外发许可时不发查询 POST；模型不可用时基础检索仍可用。客户端使用有界精确计算，云 pgvector 数据通道、ANN/大规模召回率验收不属于本轮。

三个独立角色使用现有 `LocalModel + ModelProvider/Router/Adapter/Broker`：Chat、Integration、Embedding。九原生 Adapter 与两类 Custom 保留已有传输/Runtime wire；Memory 仅增加实际服务端点/native model 身份及只读凭据 guard。共享 Embedding 契约默认 unsupported；现有 Custom Responses 显式配置 OpenAI 兼容 Embedding 端点并复用同一凭据 profile，不猜端点、不新建传输。检查输入/索引/向量数量/维度/有限浮点值，异常不保存向量；不声称所有原生服务均支持 Embedding。

`allow/revoke/permissions` 按确切目标、用途、scope、kind、source_type 和 local-only 独立许可；新增许可必须匹配当前已配置的实际目标。模型别名相同而端点/native model 改变也不能继承许可。模型配置不授外发，外发不授云同步；Bearer/ApiKey owner 必须是本机 namespace，拒绝 Host/其他账户 owner。ConfirmedSession 的 access/refresh 只读 guard 自动接入，已配置 Broker guard 及 `protect_credentials` 阻止已知值/认证结构进入记忆、导出和模型正文。离线仅选择缓存身份时，应用必须传入其安全存储中已知凭据的 guard；这不是识别所有未知秘密的保证，不读取真实用户 Key。

作业只在本地保存有界来源/记忆 revision 引用。`queue_integration/queue_embedding/jobs/run/cancel` 是共享核心接口；缺模型/Key/许可或来源暂不可用时 waiting，已提交后 done/failed/cancelled/unknown 均不可重播。Future 丢弃或进程中断封存 unknown；删除、纠错、撤权、切账户、自动关闭、模型配置/guard 变化都取消并更新 generation/epoch，迟到结果重新检查来源/版本/权限才提交。运行状态写失败不发模型 POST；终态落盘失败仍由重启恢复未知，不能把未知结果当安全重试。正式 UI/CLI 或后台消费者以后负责调用 `run`，本轮没有新增常驻调度服务。模型绑定只在当前进程会话内，重开后应用须重新配置，再明确恢复 waiting 作业；数据/许可保留，未知 POST 仍不能恢复重播。

开启自动记忆且取得 Chat 用途许可后，普通 Chat 临时加入 ≤16KiB 的低可信 user reference，附类型/来源/ID/revision，永不作为系统/工具/审批指令，也不污染原始 Chat 消息。完成轮次为用户/assistant 文本排 waiting 作业，不自动开始模型调用；等待队列满时跳过新增采集，已完成 Chat 不因此失败。自动关闭停止提取/注入，保留现有资料管理；真实 Runtime 工具仍由固定 Codex 执行/审批，Memory 没有执行入口。

## 有界行为与本地试用

每 scope 非删除记忆 ≤4096、每记忆 revision ≤1024、正文/来源摘录 ≤16KiB、来源 ≤8、标签 ≤16（每项128字节）、单 draft ≤64KiB、核对完整来源 ≤1MiB；列表/作业页 1～100。导出覆盖当前 scope 全部非删除记录（最多4096及其版本），不包含其他账户/项目或已删除载荷。等待/运行作业 ≤1024、外发许可 ≤1024、整合/Embedding 启动时共享进行中记忆数须 <8，普通 Chat ≤16轮、合计外发并发 ≤24、deadline120秒；Embedding 输入 ≤32项/合计64KiB、维度1～16000。数据库沿用 I-3 约256MiB总上限，Chat/Memory 共用容量；Provider 自身可施加更严限制。SQLite 满库定向测试验证纠错 head/正文/outbox 原子回滚，不等于物理 VPS 满盘压测。

在有足够磁盘的独立检验环境/当前功能 SHA checkout 执行；本轮完整构建均在 GitHub：

```sh
cargo test -p caidex-chat --locked
python3 scripts/i4-memory.py --output i4-memory.json
```

脚本创建并清理自己的临时私有目录/loopback HTTP fixture，运行 `cargo run -p caidex-chat --example local_memory`，通过真实 Custom Provider+Router 观察：默认关闭、无模型/许可等待、整合候选与批准、Embedding/语义查询、空间变化、三 revision 冲突、临时 Chat 注入开/关、导出、删除重启。成功原输出为 milestone I-4.1、http_requests=5（Integration1/Embedding2/Chat2）、uploads=0、fixture_assertions=passed。所有请求合成、无 Authorization、无工具；真实商业模型质量/费用/兼容性仍 F/G-Live 待用户最终验收。内部例子不等于正式 CLI/GUI/iOS。

## 验收映射与明确未验范围

| 条件 | I-4.1 对应证据 | 保留边界 |
| --- | --- | --- |
| MEM-01/02 提取、无模型与用户纠错 | waiting/default-off、AI 候选/类型/来源、导出及 HTTP 整合 | 正式页面、常驻消费者以后接入 |
| MEM-03 决定取代/冲突 | CAS head、三 revision、争议排除、人工选分支、满库回滚 | 云多设备协议归 I-6 |
| MEM-04 来源/项目 | 项目先授权、文件/Git 实际变化排除、Execution 类型和当前摘要校验 | 实际 Host 来源授权/读取/记忆执行归 I-4.2 |
| MEM-05/06 跨模型与空间 | Chat A/B 使用同 revision、整合/Embedding 独立、同维不同空间无请求 | 合成模型不证明商业 Live 质量 |
| MEM-07 检索 | scope SQL、有效状态/来源、关键词/标签/时间、当前空间向量及离线基础回退 | 云 pgvector/ANN/生产容量未验 |
| MEM-08 注入/审批 | 恶意来源与工具输出拒绝、低可信 user 注入、原 Runtime/Host 审批回归 | 不冒充真实 Host 记忆攻击链已验 |
| MEM-09 管理/自动关闭 | 关闭停止提取/注入、CRUD/删除/重启/防旧来源复活、队列满 Chat 正常 | 云删除/同步墓碑归 I-6 |
| MEM-10 Host 离线/恢复 | **本轮未验、未完成** | 用户已拆至 I-4.2；不得使用 Runtime 回归代授 |
| SEC-05/07 本地隔离/秘密/外发 | 账户切换/迟到拒绝、ConfirmedSession guard、实际 Custom/Anthropic/Gemini HTTP 凭据前置阻断、精确目标许可 | 本地 grant 不是云 ACL；guard 不能识别所有未知敏感数据 |

SQLite 仍为 OS 权限保护的明文，不能防同 OS 用户/管理员。云同步 API、账户 enabled/settings_version/cloud_epoch 数据传输与 B 删除归 I-5/I-6；本轮 outbox 只是本地引用，不生成云上传。iOS Simulator/Archive/真机、GUI/正式 CLI、真实 Host Memory RPC、生产 VPS 磁盘增长/备份及性能不授通过。I-4.1 通过条件限定本地核心，整个 I-4 与 MEM-10 须 I-4.2 的独立实际证据。

## 最终精确验证与停止状态

不可变功能源码 [`c17c5bde51b79c2ec7e1ca5df9c5c3b584847b17`](https://github.com/bboytang/CAIdex/commit/c17c5bde51b79c2ec7e1ca5df9c5c3b584847b17)，[精确 CI38146076798](https://github.com/bboytang/CAIdex/actions/runs/38146076798) 四 job 已完成且成功。每份原始日志的 checkout 完整 SHA 均为该功能 SHA。封存提交只追加文档/证据，不把封存 HEAD 当功能 CI 源码。

| 环境/job | workspace 通过/失败/ignored | Memory（已计入 workspace） | 固定 Runtime 额外 | 其他证据 |
| --- | --- | --- | --- | --- |
| Linux/114489598304 | 711/0/83 | 12 | 81/0 | 原 Chat7、Host42 均计入 workspace；原生凭据附加1；Chat/Memory/H-1/2/3 演示成功 |
| macOS/114489598375 | 710/0/81 | 12 | 80/0 | 原 Chat7、Host42 均计入 workspace；三平台全部 fmt/Clippy；Chat/Memory/H-1/2/3 演示成功 |
| Windows/114489598108 | 704/0/81 | 12 | 80/0 | 原 Chat6、Host41 均计入 workspace；额外 Host lib4/Windows ACL 边界；Chat/Memory/H-1/2/3 演示成功 |
| 数据库/114489598283 | 不计入 workspace | 无 | 无 | 真实 PG17.10/vector0.8.2；I-1 118/0、I-2 110具名组/0、共享认证5/0 |

每平台 Memory 为11项集成测试 +1项 SQLite 满库纠错单元测试；另有 Custom Embedding/秘密 HTTP、Anthropic Memory guard、Gemini Memory guard/精确流目标3项 Provider 测试。原 Chat、Host 与这些测试已计入 workspace，不能重复相加；ignored 不算通过。固定 Runtime 与平台附加检查另列。I-2 110是具名断言组，不是110个 Rust 测试。

三平台 Memory 演示均观察5次实际 loopback HTTP：整合1、Embedding2、Chat2，uploads=0，默认关闭/候选批准/空间隔离/冲突/注入开关/删除重启检查通过。原 Chat 演示4次 HTTP及 H-1/2/3/固定 Runtime 均在同一功能 SHA 回归成功，不能因此授真实 Host Memory/MEM-10 或商业 Live。

[最终四 job/步骤/源码 SHA256/原日志/演示](evidence/i41-ci.json)、[同提交真实认证回归](evidence/i41-auth-regression.json)、[七轮开发结果与28份原日志](evidence/i41-development.json)为证据索引。首轮 Clippy 嵌套条件、第二轮终态空输入错误分类已修；c32c23b/bc03cce 为成功中间提交，两次旧提交运行主动取消。39f6b57 的 Windows Memory 全通过，但未改动的 Host 服务测试在 service.rs:129 等待超时（20/21）；未定位根因、未放宽超时/断言。当前最终功能提交的 Windows 完整 Host/Runtime 回归已成功，该历史失败保留而不授通过。

32份最终/开发原日志均核对压缩与原文 SHA256，未见 ENOSPC/No space left on device。原190份证据逐字保留；Cargo.lock、PostgreSQL迁移、I-3 schema1、固定 Runtime/Host/upstream 不变，36个本轮源码/配置/脚本文件指纹可核对。原 Account/UI 仅更新用户已确认的自动记忆默认关闭。

本地仅 fmt/Python compile/diff/链接/原始证据与源码指纹检查，未本地完整构建，未清理用户/全局 Codex/有效缓存。盘39GiB/已用35GiB/可用约3.6GiB（91%）；SQLite max_page_count 定向故障不代物理 VPS ENOSPC 验收。

当前 I-4.1 开发通过条件已满足，停止新增功能，等待独立新会话/其他模型只读审计不可变源码与实际证据；实施会话不授独立审计。用户确认 I-4.1 审计通过且明确批准后，才可开始 I-4.2；不自动进入 I-5。

## 封存后的只读复验

独立新会话/其他模型以最终证据索引中的不可变源码为审计对象，读取完整调用路径、迁移及实际结果；开发会话自查和 CI 均不授独立审计通过。归档完成后，仓库根执行以下静态检查即可核对源码/四份最终原日志及所有开发失败/取消/中间原日志，不运行模型或读取 Key：

```sh
python3 - <<'PYVERIFY'
import gzip, hashlib, json, subprocess
from pathlib import Path
base = Path('docs/evidence')
report = json.loads((base / 'i41-ci.json').read_text())
assert report['milestone'] == 'I-4.1' and report['conclusion'] == 'success'
assert len(report['jobs']) == 4
for path, digest in report['source_sha256'].items():
    raw = subprocess.check_output(['git', 'show', report['source_sha'] + ':' + path])
    assert hashlib.sha256(raw).hexdigest() == digest, path
development = json.loads((base / 'i41-development.json').read_text())
all_jobs = report['jobs'] + [j for run in development['runs'] for j in run['jobs']]
for job in report['jobs']:
    assert job['conclusion'] == 'success' and job['checkout_sha'] == report['source_sha']
for job in all_jobs:
    packed = (base / job['log']).read_bytes()
    assert hashlib.sha256(packed).hexdigest() == job['gzip_sha256']
    assert hashlib.sha256(gzip.decompress(packed)).hexdigest() == job['raw_sha256']
print('I-4.1 immutable source and original job logs verified')
PYVERIFY
```
