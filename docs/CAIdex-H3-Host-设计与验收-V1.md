# H-3 多端审批、Diff 与纵向集成设计与验收 V1

2026-10-11用户确认H-2独立审计通过并批准H-3；用户告知的审计结果不由本实施会话自授。起点main/HEAD/origin/main/GitHub main均为0151b488127f7357df3523176e5e2c24be63bdd8，工作区干净，构建前磁盘2.4GiB可用。H-2功能a6c22072b622a6dc2a77e06951c5b72c0554ca20与原精确CI38102729830证据保留。

## 目标、有限范围、依赖和停止

依据[V3 H-3](CAIdex-实施计划-V3.md)、[CLI Host/审批契约第10/11节](CAIdex-CLI-完整交互与验收规范-V1.md)，复用H-1/2单写者Host、SQLite journal、seq/snapshot、固定Codex0.160.1与原Facade。实现持久审批有效期/撤销/首次有效竞争/迟到拒绝、独立Host授权撤销、真实Runtime输出与Diff/Review数据恢复及有界失败，提供内部双客户端纵向真实进程演示。

非目标：I及后续阶段、正式GUI/完整CLI、SSH/Relay与官方账户、商业API/真实Key/生产部署、另造Agent/工具执行器/审批引擎。两个隔离Custom Responses profile继续只用于本机合成模型，不授F/G-Live通过。生产平台/安装包不由Rust CI代验。

有限计划：①核实锁定协议/现有审批与授权/存储，记录最小设计；②先补针对性回归，增加持久有效性和恢复投影，保持原权限上限；③真实Runtime隔离文件审批、双端竞争/迟到/撤销/Review/Diff/流式输出与重启恢复演示；④fmt/Clippy/workspace、固定Runtime离线/schema/doctor、H-1/2/3演示及最终功能源码三平台精确CI；⑤封存SHA、实际计数/日志哈希、可复现步骤/失败/限制后停止等待独立审计。重大架构冲突停止报告。

通过条件：两个独立获权端看到同一持久Request/Host/Thread/Turn与真实availableDecisions；首个有效操作只提交一次，竞争输家与迟到/过期/撤销不能写Runtime；发送结果未知不重发；授权撤销同时阻止已有连接/重连/排队动作，不借Account授权；无客户端/断线不自动批准；原生文件执行与终态/Diff一致，Review和工具结果保留明确归属/未知字段；seq缺口恢复可信快照；容量/存储/Runtime失败安全拒绝，不降低H-1/2边界。最终SHA三平台CI成功后立即停止，不自行授独立审计通过或开展I。

## 已实现的最小设计与安全边界

- schema v3仅扩展原snapshot：Host授权摘要及任务artifact投影；v1/v2验证后事务升级，保Host ID/seq/旧操作hash，inspect只读不迁移，新版本拒绝。旧stream请求重启一律无效。
- 原Host Token保留owner职责。owner签发最多64个独立256bit令牌，SQLite只保存SHA256摘要与client_id/observe/execute/approve范围；令牌仅在签发回应返回一次，不入事件/快照/日志。client_id不复用，丢签发回应需owner撤销并新建。execute允许提交/取消，approve允许答复/撤销请求，observe允许只读；委托端不能签发/撤销授权或shutdown。全部鉴权仍在同OS用户/loopback Host域，不新增账户身份或远程配对。
- actor处理动作时再次核对当前持久授权；提交授权撤销后唤醒已有连接，禁止新的请求/观察输出与重连。已开始传输的数据/已发送动作无法撤回，撤销不回滚执行副作用；owner令牌轮换仍由启动环境负责。
- 每原生审批保存原完整请求、stream/Thread/Turn/Request归属、到期时间（最迟task截止，最多120秒），同时使用进程内单调时间作等待上限，系统UTC回拨不能延长有效期、有效状态和首次决策client_id/operation_id/decision。先提交sending意图再调用原Facade，后续有效性按持久记录查询；不同操作竞争只允许首个claim。相同操作/hash重查原结果不发第二次。
- 取消终态确认最多15秒，UTC截止与进程内单调时钟同时约束；重复取消不延长期限。Runtime仅确认取消RPC却不发终态时，先持久记录unknown再停止Runtime，不伪造cancelled或重发未知动作。
- 请求过期或显式撤销先持久标记，再用既有原生turn/interrupt使回调失效，不伪造批准/拒绝，明确整Turn取消。合法serverRequest/resolved/Turn结束/线程关闭使请求失效；无对应显式ID/stream的事件不能修改别的任务。已claim请求的撤销报告handled，需要停止执行使用显式task/cancel。
- 真实turn/diff/updated、fileChange patch/item、command结果、agentMessage和entered/exitedReviewMode按明确Thread/Turn关联保存原始wire，先落盘再广播。每任务artifact最多64项/合计1MiB，超限拒绝当前提交并安全停Runtime，不默丢原文冒称完整。未知外部事件继续journal原始记录；host/*仍中立，不能伪造授权/内部状态。
- Review提交只启用固定原生review/start的inline uncommittedChanges，继续原Host cwd/policy及profile，不自行读取/计算项目Diff或执行patch。新Submission可选review=true，false省略序列化以保持旧H-2载荷hash；其它Review类型归后续产品入口。

## 验证与证据

中间功能源码dc38cfbce4827c87bd87ec2972f533db07368e89的[CI38106599664](https://github.com/bboytang/CAIdex/actions/runs/38106599664)三平台成功，但最终检查发现cancel-requested被期限筛选永久跳过。新增失败回归实际0通过/1失败，证明只收到取消RPC确认、没有终态会永久占用Host；最小修复增加15秒确认期限、重复取消不延长及UTC回拨的单调时间屏障，逾期unknown并停止Runtime。修复后针对性回归1/0/0，完整本地检查及演示再次通过；最终源码尚待提交及新的精确三平台CI，旧成功不代本次修复验证。[首轮CI38105001809](https://github.com/bboytang/CAIdex/actions/runs/38105001809)与[第二轮CI38105455389](https://github.com/bboytang/CAIdex/actions/runs/38105455389)均Linux/Windows H-3演示失败、macOS成功，不代最终取消期限修复的三平台验证，也不授独立审计通过。首轮立即读取聚合Diff KeyError；增加30秒等待后仍失败，时序假设不足。[首轮](evidence/h3-failed-ci.json)/[第二轮](evidence/h3-second-failed-ci.json)完整失败日志分别封存。非root原始journal证明聚合通知0而原生fileChange Diff完整，Runtime记录sandbox写失败；固定源码保守失效不精确累计delta（见下面能力边界）。最小修正只明确强制验证真实逐文件Diff路径/内容与恢复，并实测、验证实际聚合事件；非root完整流程已通过（聚合事件0）；root全仓及固定Runtime/H-1/2/3演示已完整重跑并通过（聚合事件3），新功能SHA三平台CI重新完整运行。本地fmt、Clippy workspace/all-targets/locked/-D warnings通过；workspace实际682通过/0失败/83忽略，含Host42/0/0；固定Runtime离线81/0/0；Linux原生凭据附加1/0/0；stable/experimental schema及doctor（0.160.1、d27764b82f7118f674371e6d6e76271d9d606edb、无模型Turn）通过。H-1/2演示复验通过。H-3真实独立Host/固定Runtime演示实测claim1、本机HTTP9、原生patch offer3、重启HTTP增量0，实际文件内容CAIDEX_H3_PATCH与原生Diff一致，原生Review入口/退出、双端竞争/重查/迟到、请求撤销、授权撤销/重连、超过128事件的snapshot恢复、未答请求强杀重启unknown均通过。

[本地逐检查日志与源码指纹](evidence/h3-local.json)、[Host42实际测试名及结果](evidence/h3-host-tests.log)、[HTTP trace](evidence/h3-local-http-trace.json.gz)、[关闭后schema v3快照](evidence/h3-local-snapshot.json.gz)、[开发失败原日志及修正原因](evidence/h3-local-failed.json)均保留完整原文及SHA256。[非root原始负例/正例、sandbox日志与上游指纹](evidence/h3-runtime-diff-boundary.json)明确记录聚合Diff0与原生逐文件Diff恢复；辅助featured-plugin无认证缓存请求401也记录，未将本机模型计数当全部网络流量。该证据于功能提交后生成，逐文件核对提交内容与验证时源码一致，后续封存不改功能源码。原始before回归在旧H-2源码上证明新授权入口缺失；新实现回归曾捕获撤销reason被Cancel覆盖，已最小修复；测试/演示假设错误与编译/Clippy错误单独记录，未降低门槛或把零测试当通过。

新增6项service测试覆盖双端首次竞争/不重复写、scope与已有连接撤销及外部host/access伪造、显式请求撤销/迟到、审批SQLite提交失败不写Runtime且重启unknown、授权提交失败不返回令牌、原生resolved/线程关闭屏障。新增3项journal测试覆盖原始Diff/Review/工具事件及未知字段/ID/旧stream隔离、64项及合计1MiB上限失败无部分提交、v2只读inspect与失败迁移回滚。Diff回归另覆盖合法同Thread/Turn的终态后到达及持久恢复。原超时测试增强UTC/单调请求期限及整任务期限，包含UTC回拨、不产生approval/reply的原生interrupt证据，以及取消RPC已确认却缺失终态时的15秒上限、重复取消不延长、逾期unknown且无自动重发。已有H-1/2隔离、认证、seq/原子性/提交后广播/未知不重发测试继续通过。

## 独立复现

需Rust 1.99.0、Node 22、Python 3、Git及安装的固定Codex；先检查磁盘。以下只在新建私有测试目录工作，演示保留自己的证据目录，不修改用户项目。当前仓库的有效构建/Runtime缓存可复用，无需删除全局Codex数据或旧缓存。

```bash
df -h . /tmp
npm install --prefix .tools/codex --no-audit --no-fund --ignore-scripts @openai/codex@0.160.1
export CAIDEX_CODEX_BIN="$(node scripts/codex-binary.mjs)"
# TEMP根必须在用户Git仓库外；不删除已有/tmp/.git。
export TMPDIR="$(mktemp -d /var/tmp/caidex-h3-review-XXXXXX)"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test -p caidex-host --locked
cargo test -p caidex-runtime --test real_runtime --locked -- --ignored
node scripts/verify-codex-schema.mjs
cargo run -p caidex-cli --locked -- doctor
cargo build -p caidex-host --locked
python3 scripts/h1-demo.py
python3 scripts/h2-demo.py
python3 scripts/h3-demo.py
```

Linux原生凭据附加验证按既有`scripts/test-linux-secret-service.sh`及CI依赖在隔离session执行；Windows/macOS命令与路径设置见`.github/workflows/ci.yml`，Windows使用`python`。H-3也可指定`--directory /新的私有目录`，目录必须不存在；输出JSON包含证据目录，内有journal.sqlite3、trace.json、隔离Runtime数据与实际fixture项目。只查询复制的SQLite或停止后的演示库，别在运行中的Host上修改原数据库。每个演示独立启动、终止自己创建的进程，不需要用户Key。

## 失败路径与限制

- SQLite审批claim失败保持最后已提交blocked且无赢家/无决策写出；离线inspect及重启派生unknown。授权提交失败不激活grant或返回secret。artifact/迁移失败回滚，原数据不部分更新；容量拒绝和未知Runtime/存储失败采取安全停止，不承诺自动修复/重试未知动作。
- 等待请求最多120秒且受300秒整任务执行预算约束；取消终态另有最多15秒确认窗口，重复取消不延长，最迟300秒预算后再等待15秒即unknown并停止Runtime。显式请求撤销/到期取消整个原生Turn。已被首次claim的动作不再由请求撤销回滚；已发送结果未知保留unknown，不保证外部副作用exactly-once。断线或无客户端不自动批准。
- 委托授权是同OS用户、loopback Host的有限scope能力；observe能读该Host任务/事件全域，approve能处理该Host任务审批，未提供按项目/任务ACL或官方账户身份。已发送数据/动作不能撤回；owner令牌由原启动环境管理。64个授权ID（含撤销）不能复用，需以后明确迁移/治理才能扩容，不擅自裁剪记录。
- snapshot保存最新artifact，完整已提交wire/流式delta在journal；每任务64项/合计1MiB、最多1024任务/4096操作及既有帧上限，是安全边界而非生产吞吐或无限历史承诺。没有journal裁剪、高负载/备份容灾验证，Runtime未捕获事件无法补造真源。
- 固定Runtime聚合Diff是有条件能力：[`apply-patch`](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/apply-patch/src/lib.rs)失败写入将delta标为不精确，[工具Runtime](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/tools/runtimes/apply_patch.rs)跨sandbox尝试累积delta，[tracker](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/turn_diff_tracker.rs)失效后不再输出有效聚合Diff，此前已有聚合数据时可能发空diff清除。[事件映射](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/tools/events.rs)仍发原生fileChange完成数据。非root实测与该源码行为一致，不直接观测Runtime内存delta。演示原先误设每次编辑必有聚合事件，现强制核对原始`fileChange.changes[].diff`的路径/内容和恢复，聚合事件有发出时仍强制核对落盘/恢复，计数0不称为聚合成功。Host转交原生数据，不自行重算或把逐文件内容伪装成完整unified Diff；如果后续产品要求无条件全量聚合，需另行确认设计，当前不改变固定Runtime。
- Review只支持新线程inline uncommittedChanges；演示Review文字来自本机合成模型，验证原生链与恢复，不授模型判断质量或独立审计结论。保留现有command/file审批支持；未支持的原生互动继续失败安全拒绝。正式GUI/CLI、远程授权、商业兼容、iOS/安装包与生产Host未验。
- 报告里的SQLite/HTTP计数来自实测，文件/事件/恢复来自代码断言；不读真实Key/不调商业模型是离线场景声明，不是独立全局网络或凭据遥测。F/G-Live保持待验。
