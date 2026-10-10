# CAIdex H-1 本地Host设计与验收 V1

2026-10-10用户确认F/G-Offline关闭并批准H-1；F/G-Live仍待验。依据[V3 H-1](CAIdex-实施计划-V3.md)、[CLI第10节](CAIdex-CLI-完整交互与验收规范-V1.md#10-hostremote线程与任务)。本里程碑交付本地生命周期/journal与内部客户端，完成后停止，H-2/H-3/I未授权。独立审计尚未执行，实施会话自查不能替代。

## 有限范围与实现

新增独立`caidex-host`进程（[入口](../runtime/host/src/main.rs)、[服务](../runtime/host/src/service.rs)），持有固定Codex **0.160.1** 的app-server与现有Runtime Facade。启动检查版本，隔离子进程CODEX_HOME、清空继承环境，只保留基础OS路径变量；不导入用户Codex配置/登录、模型Key或Account Token。探针Provider指向本机未开放端口且不需要Key；唯一Runtime操作为固定参数的`thread/start`（ephemeral/read-only/never），没有模型轮次、工具请求或审批回应。无重新实现Agent/工具执行器/审批引擎，无Provider/Broker修改。

Host不读取stdin，不跟随测试客户端退出；退出客户端只是断连接/detach。只有独立Host授权下的显式`shutdown`关闭其Runtime；进程死亡不保证活动Runtime动作完成。本次不安装系统service、不做开机启动、任务/operation幂等提交、Queue/Steer/取消、审批竞争、Diff、正式GUI/CLI、SSH/Relay、账户/Memory或生产部署。

## Journal与恢复契约

[journal](../runtime/host/src/journal.rs)使用精确锁定rusqlite0.38.0及bundled SQLite；必要新增依赖全部锁于Cargo.lock，未升级既有依赖。数据库`journal.sqlite3`独立于Chat/Memory，application_id=`0x43414948`、user_version=1；V0仅迁移空白数据库，非空未版本化库/不同application/新版本拒绝，事务创建V1表。schema未来迁移须显式实现，不能自动降级/覆盖。

- 持久随机Host ID；每次成功接入Runtime后新stream；Host范围`seq`连续增长，stream内`stream_seq`从1增长。事件保留原始通知及未知字段；本次账户通知/服务端交互不持久化载荷且安全停止，绝不自动答复。
- 每次单一SQLite事务追加事件并更新物化snapshot，WAL + synchronous FULL（[SQLite官方语义](https://www.sqlite.org/pragma.html#pragma_synchronous)）；提交成功后才更新内存/广播/回复。提交失败终止Host，不再发Runtime请求或广播未提交数据。恢复校验SQLite完整性、schema/身份、事件与快照水位/尾stream；不自动修库。
- snapshot版本1包含Host/stream/seq、生命周期、线程缓存及未确认探针。`thread/started`数据源于实际Runtime；Host重启把旧线程`runtime_state`设为`unknown`，缓存wire不等于已加载线程。未确认探针保持`outcome=unknown`，不重发、不伪装失败/成功；不承诺外部exactly-once。
- attach使用`host_id`和最后已处理的`after`；跨Host、负数、未来seq拒绝。差值≤128返回连续replay及`through_seq`，差值>128或无cursor返回一致snapshot。客户端以返回水位替换恢复基线，随后只处理更大的seq；先订阅再同步读取水位，单writer中间没有await，捕获窗口没有订阅缺口。replay不是历史剪裁，全部journal仍保留。
- 广播ring128条，慢客户端会收到`gap/after/reconnect`并断开（或写超时直接断开）；用客户端最后实际处理的seq重新attach，不能以服务端发送数代替本地已处理游标。请求/事件相互穿插，客户端按响应id与event分别处理。detach丢弃订阅，不发interrupt/cancel/shutdown。
- Runtime断开/队列溢出/不支持交互标记`unavailable`并停止；未捕获事件可能丢失，不能用已捕获journal宣称完整Runtime历史。强杀/磁盘故障无法落盘新的不可用标记，离线inspect将旧running缓存显示为offline，重启仍使旧Runtime缓存失效。

## 本地权限与资源

只绑定127.0.0.1随机端口，协议v1，首帧独立256位Host授权令牌，常量时间比较。令牌由演示父进程生成，经Host环境和观察客户端stdin传递，不在argv、stdout、journal、Git或Runtime子进程环境中。不是Account/Gateway/模型Key；没有Key读取/导出入口。所有控制客户端在H-1属于同一OS用户授权域，本次不认领生产多用户ACL/撤销或Remote授权。

Host目录Unix验证当前owner/0700，Windows用系统PowerShell/.NET创建owner-only继承ACL并核验现存ACL；拒绝宽权限/目录链接及存储链接，不修复现存用户目录权限或覆盖文件。独占OS文件锁防止第二Host启动；SQLite保留独立短busy timeout。现存探针config内容不一致或有auth.json拒绝，不读取其秘密。Windows具体ACL与Rust实现由Windows runner验证；不等于Windows11桌面/UAC/安装验收。

连接最多64、请求帧64KiB、事件1MiB、广播/replay128、Runtime请求15秒、客户端写5秒；无自动重试。没有日志保留/裁剪、压缩/快照容量上限或生产吞吐承诺，磁盘不足安全失败。SQLite提交与snapshot序列化在Host服务任务同步执行；高负载隔离/持久任务有界资源及jsonschema CPU限制仍归后续H。未声称已解决系统崩溃/整盘损坏或备份恢复。

## 独立复现

取得仓库对应不可变源码，使用rust-toolchain.toml锁定工具链；磁盘先检查（Linux `df -h .`、Windows `Get-PSDrive`）。不清理用户资料或全局Codex。无固定Runtime时只在仓库`.tools`按既有方式安装，不升级全局CLI。

```bash
npm install --prefix .tools/codex --no-audit --no-fund --ignore-scripts @openai/codex@0.160.1
export CAIDEX_CODEX_BIN="$(node scripts/codex-binary.mjs)"
CARGO_BUILD_JOBS=2 cargo build -p caidex-host --locked
python3 scripts/h1-demo.py
```

Windows PowerShell：

```powershell
npm install --prefix .tools/codex --no-audit --no-fund --ignore-scripts @openai/codex@0.160.1
$env:CAIDEX_CODEX_BIN = node scripts/codex-binary.mjs
$env:CARGO_BUILD_JOBS = '2'
cargo build -p caidex-host --locked
python scripts/h1-demo.py
```

演示自动启动独立Host→两客户端attach→真实线程探针→独立观察客户端进程退出→两端断开/重连/replay一致→实际强杀Host→同journal重启→旧线程unknown、无探针重发→两端snapshot/replay→显式shutdown。预期单行JSON `status=ok`、`clients=2`、`observer_process_exit_detaches=true`、`replay_identical=true`、`snapshot_restored=true`、`restart_resubmissions=0`、`model_turns=0`。新Runtime通知可推进seq，不能假定重启只产生一条事件。

默认演示只清理自己独占创建的临时目录。保留独立证据用 `python3 scripts/h1-demo.py --directory /absolute/new/private-host`，必须是新路径，演示不删除此目录；停止后 `target/debug/caidex-host inspect /absolute/new/private-host`查看offline快照，或用SQLite只读查询`SELECT seq,stream,stream_seq,method FROM events ORDER BY seq;`。inspect在已有Host持锁时拒绝；不创建新的数据库，不等于连接活Host。

不手工回显/保存Host令牌。内部JSONL协议首帧`protocol/token`后，命令`id/method`仅支持snapshot、attach（host_id/after）、detach、probe、shutdown；不是正式CLI契约。`scripts/h1-demo.py`包含参考测试客户端，两个独立TCP连接及一个独立观察进程，不等于正式多设备客户端验收。

## 测试、失败场景与证据

```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 cargo test --workspace --locked
CAIDEX_CODEX_BIN="$(node scripts/codex-binary.mjs)" cargo test -p caidex-runtime --test real_runtime --locked -- --ignored
```

若临时目录本身处于Git工作树，现有凭据测试会按保护规则拒绝；选择独占且Git外的临时测试根通过TMPDIR传入，不删除.git或修改该边界。

| 门槛/失败场景 | 证据入口与通过判定 |
| --- | --- |
| SQLite满容量/事务后半段失败 | journal内部SQLITE_FULL测试、[journal集成](../runtime/host/tests/journal.rs)注入snapshot失败，event/snapshot/watermark全部不推进 |
| 提交失败不发动作/广播 | [service测试](../runtime/host/tests/service.rs)注入intent触发器失败，客户端无事件、Runtime marker不存在、Host安全退出 |
| 双端重连/一致replay/snapshot/提交后可读 | service测试、真实进程演示；独立SQLite连接在收到事件时已经能读取该seq |
| 未知回应/重启不重发 | delay夹具真实Facade收发后中止Host服务；journal未知intent保留、重启marker仅一次。仅线程探针，不代表真实工具/任务恢复已验 |
| Runtime退出/认证交互 | exit/auth夹具；Host不可用、未知intent不假失败、秘密载荷无journal记录 |
| 慢客户端/部分请求 | service内部ring溢出明确gap并以snapshot补缺口；service集成部分帧遭广播中断后仍正确解析 |
| 版本/损坏/身份/文件保护 | journal集成拒绝future schema/foreign application/nonempty V0/corrupt/坏snapshot/双owner/Unix链接宽权限；拒绝前后源库内容保持 |
| 真实固定Runtime与独立Host进程 | 三平台CI新增H-1无模型演示步骤；已有schema/doctor/真实Runtime回归继续运行 |

当前本地/精确CI结果及源码SHA在本文件交付记录和[HANDOFF](../HANDOFF.md)更新，不把尚未执行的检查写成通过。

## 交付记录与独立审计

最终功能源码：`dec3a374228a75b9a6600715b0d56bf5047b12b0`，已提交/push main；[精确CI38095166217](https://github.com/bboytang/CAIdex/actions/runs/38095166217)三平台完整成功，完整原始日志已核对测试名/计数、schema及演示JSON。封存文档提交不改变该源码、依赖或workflow。H-1已交付并停止，不开始H-2；用户验收与独立审计仍待执行。

| 实际runner / 完整job | workspace 通过/失败/忽略 | H-1（包含于workspace） | 固定Runtime 通过/失败/忽略 | 真实Host双端演示 |
| --- | --- | --- | --- | --- |
| [Linux ubuntu-24.04](https://github.com/bboytang/CAIdex/actions/runs/38095166217/job/114339472553) | 655/0/83 | 15/0/0 | 81/0/0 | 成功，seq6→9 |
| [Windows windows-2022](https://github.com/bboytang/CAIdex/actions/runs/38095166217/job/114339472633) | 649/0/81 | 14/0/0 | 80/0/0 | 成功，seq5→7 |
| [macOS macos-15](https://github.com/bboytang/CAIdex/actions/runs/38095166217/job/114339472658) | 654/0/81 | 15/0/0 | 80/0/0 | 成功，seq5→7 |

ignored不计入通过；固定Runtime的ignored用例在后续实际原生执行步骤单独运行。Windows不运行Unix权限/硬链接专属测试，另有直接生产ACL脚本与真实Rust子进程边界检查，均成功。Linux另行隔离原生凭据service测试1/0/0。三平台fmt、全仓Clippy、stable/experimental schema指纹、doctor及H-1实际演示均成功；不是Windows11桌面或iOS验收。三个演示均model_turns=0/commercial_calls=0/user_keys_read=false/restart_resubmissions=0。

[最终精确CI摘要](evidence/h1-ci.json)保存完整step状态、实际测试名/计数、演示JSON及原始日志SHA256；完整日志归档：[Linux](evidence/h1-ci-linux.log.gz)、[Windows](evidence/h1-ci-windows.log.gz)、[macOS](evidence/h1-ci-macos.log.gz)。解压后的原始字节SHA256须与摘要一致，压缩文件自身SHA256也已记录。

最终Linux定向H-1 **15/0/0**、Host Clippy/fmt及独立真实进程演示通过，见[最终本地测试日志](evidence/h1-final-host-tests.log)和[最终源码文件指纹/测试名/演示JSON](evidence/h1-final-local.json)。最终演示stream1 seq5→stream2 seq7、同Host、旧线程unknown、0重发、0模型轮次；CI上真实Runtime通知可产生不同seq，验证只要求连续提交/恢复、不固定事件数。构建前约4GB空闲，完成验证约3.1GB；复用缓存、限制构建并行，没有清理用户资料或全局Codex。

历史本地workspace655/0/83、固定Runtime81/0/0及stable/experimental schema有效记录在[初版本地摘要](evidence/h1-local.json)和[初版本地测试日志](evidence/h1-host-tests.log)。初版文件指纹对应db8bf47，不冒充最终版本；最终完整workspace/Runtime以精确CI为准。Linux本地默认沙箱不允许socket/原生执行，已在授权环境复验；/tmp自身为Git工作树，凭据保护测试改用新建Git外私有TMPDIR，没有删除.git或改变保护。最终CI仍使用既有三平台流程，不新增分支制度。

### 实际失败与修复记录

| 不可变源码 / CI | 实际结果与最小修复 |
| --- | --- |
| db8bf47 / [38093594306](https://github.com/bboytang/CAIdex/actions/runs/38093594306) | Linux/macOS成功；Windows私有目录ACL初始化失败。后继增加安全step诊断、SID值/强类型flags比较及直接生产脚本新建/复开/拒绝继承检查；未放宽owner-only权限。首次具体比较没有逐项重演，不冒充已证根因 |
| 8dfa3be / [38094110441](https://github.com/bboytang/CAIdex/actions/runs/38094110441) | Linux/macOS成功；Windows直接脚本成功，但Rust子进程step17 Set-Acl失败。仅在该PowerShell子进程移除继承PSModulePath，增加早期真实Rust边界检查；后继CI证实两项Windows权限检查均成功 |
| d62ee4f / [38094678640](https://github.com/bboytang/CAIdex/actions/runs/38094678640) | Linux成功；macOS测试发现任务completion先于参数journal锁析构；Windows认证交互测试中Host正确停止，却抢在夹具注入RPC回复之前。最终修复显式drop journal后返回，并将启动提交纳入同一清理；夹具先确认注入再发故障事件。既有测试复验，不加重试/睡眠掩盖 |

Windows PSModulePath跨版本继承问题见[微软官方说明](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_psmodulepath?view=powershell-7.6)。同步改用absolute路径，避免canonicalize生成Windows verbatim路径传给PowerShell（[Rust官方说明](https://doc.rust-lang.org/std/fs/fn.canonicalize.html)），夹具marker固定LF。实现中额外字段拒绝测试还暴露serde flatten忽略未知字段，已改严格带标签struct请求，参数拒绝无Runtime副作用/秘密无journal记录由最终测试覆盖。

### 独立只读审计输入与边界

输入为上述不可变源码、最终本地文件指纹/日志、精确CI摘要和三job完整日志归档、独立演示命令、保留的隔离journal，以及journal/service测试和故障夹具。GitHub job原始日志可用`gh api repos/bboytang/CAIdex/actions/jobs/<job_id>/logs`读取；当前运行证据不依赖实施会话/tmp临时文件。需要复跑时使用上节运行步骤，留存新建私有目录，不能覆盖现存用户数据。

重点核对：所有Host动作是否先提交intent；event/snapshot是否原子推进；提交后广播/回复；attach水位/部分帧/慢客户端缺口；旧Runtime状态失效与未知结果不重发；独占锁与文件/授权/凭据边界；范围是否止于H-1。

尚未执行独立新会话/其他模型审计，不将实施自查或CI成功冒充独立验收。F/G-Live、生产多用户/Remote ACL、持久任务/执行/审批、高负载/日志裁剪/备份恢复及Windows11桌面/iOS Simulator/Archive/真机/安装包仍未验或属于后续授权范围。
