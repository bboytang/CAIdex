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

H-1已实现并本地验证，功能提交/精确CI收尾中。Linux workspace655/0/83（严格请求解码最终修复之前），最终H-1定向15/0/0（含参数拒绝无副作用）、全仓Clippy及最终Host Clippy/fmt、固定Runtime81/0/0、stable/experimental schema指纹均通过。实现中发现flatten忽略额外字段，由失败测试推动改为严格带标签请求；最终定向回归及真实演示已复验，精确CI再验最终源码全仓。

Linux独立真实进程演示：旧stream1 seq5→新stream2 seq7，seq6=host/started、seq7=真实remoteControl/status/changed；同Host、旧线程unknown、0重发、0模型轮次。完整本地H-1输出见[测试日志](evidence/h1-host-tests.log)，[初版本地证据摘要/测试名/文件指纹/演示结果](evidence/h1-local.json)可独立核对；该文件指纹对应db8bf47初版，不冒充Windows后续修复版本。构建前约4GB空闲，验证后约3.1GB；仅复用缓存及新建隔离证据，无用户/全局Codex清理。

初版`db8bf47a5b0e44d8efdc64ec72006adf594889b6`/[CI38093594306](https://github.com/bboytang/CAIdex/actions/runs/38093594306)整体failure：Linux/macOS完整成功，Windows在Host私有目录ACL初始化失败，不能认领三平台完成。按阻断修复为`8dfa3be7b74fef8d75a3077df3fe9e38bdf182e6`（SID值/强类型flags比较、只输出错误step），增加Windows早期边界检查直接执行生产脚本，覆盖新建/复开和拒绝不安全继承；权限门槛未放宽。Linux15项Host测试/Clippy相关复验通过，[新精确CI38094110441](https://github.com/bboytang/CAIdex/actions/runs/38094110441)中Windows早期边界检查成功，完整结果待核验。旧版具体失败于哪个比较没有逐项重演，不将推断写成已证根因。

独立审计输入：不可变提交、该提交完整CI三job步骤/日志、上述运行命令与隔离journal、journal/service测试及故障夹具。重点检查actor所有调用是否先持久提交、广播顺序、attach水位/部分帧/慢客户端缺口、旧Runtime失效与未知intent、文件/授权/凭据边界、范围是否止于H-1。尚未安排或执行独立审计；不据此自行启动H-2。
