# H-2 持久任务与原生执行链设计与验收 V1

2026-10-11：用户明确确认H-1独立审计通过，并批准H-2。H-1不可变功能源码9df99f71eb04381260d66a0ab0db95fa6344a438及原证据保留；本轮起点main/HEAD/origin/main/GitHub main均为34ba5a636a5773ff6b7114c87fcc9fea935e5146，工作区干净，磁盘3.6GiB可用。

## 目标、范围和停止

依据[V3 H-2](CAIdex-实施计划-V3.md)及[CLI Host契约第10/11节](CAIdex-CLI-完整交互与验收规范-V1.md)：复用固定Codex0.160.1与Facade，增加Host持久任务提交、operation幂等/hash与状态查询、显式取消、实际policy/reviewer/sandbox、原生审批转交、真实执行/终态与未知结果、轮次边界与持久父子关联。事件/snapshot原子提交后才广播和调用Runtime；回应丢失查询原operation，不自动重试未知动作。

非目标：H-3多端审批竞争/完整Diff纵向集成、I、正式GUI/完整P CLI、生产部署、商业模型调用/真实Key、重写Agent/工具执行器/审批引擎。内部客户端不是正式产品客户端。F/G-Live保持待验。

有限计划：①核对现有契约并记录设计；②最小Host任务状态/存储升级及Facade执行转交；③幂等/丢回应/取消/审批/边界/故障重启定向回归，合成模型驱动真实固定Runtime独立进程演示；④fmt/clippy/workspace/Runtime/schema/doctor及最终源码Linux/Windows/macOS精确CI；⑤封存SHA、日志、计数、复现步骤和限制，停止等待独立审计。

通过条件：内部客户端可提交/查原operation/查询/取消；submit仅持久受理；原生审批未经显式决策不执行；实际终态才完成；Host重启保留未知且零自动重发；断线/丢回应恢复无重复执行；模型变更只在可信终态边界，跨Provider用独立线程并保存来源，不迁移opaque历史；H-1 seq/replay/snapshot与凭据隔离不回归；最终功能SHA三平台CI成功。重大架构冲突停止报告。

## 最小设计

- journal schema v2在现有events/snapshot内增加任务与operation投影；v1只能经校验后事务迁移，inspect保持只读；未知新版本拒绝。保留Host身份/seq/原事件。
- 单写者Host负责operation冲突检查、先持久化意图再调用现有Facade。非终态在重启/Runtime失联变Unknown，不重发；已确认终态不变。
- 每任务关联真实Thread/Turn，保存实际Runtime返回配置。一个执行链串行推进，任务受理与执行回应分离；取消RPC受理不能冒充Cancelled。
- 原生command/file审批按原Request ID/availableDecisions转交；不创建审批判定器。重启无效请求只能显示未知，不能批准。H-3的跨设备竞争/策略范围另验。
- H-2运行演示只使用明确指定的数字loopback Responses测试端点、私有Runtime home和隔离项目；不导入全局配置/账户/凭据。生产Provider配置和商业实测不在本轮启用。

## 验证与证据

本地开发验证已通过：fmt、全仓Clippy、完整workspace、固定Runtime81项离线测试、schema、doctor、H-1/H-2真实独立进程演示、Linux原生合成凭据附加测试。最终功能SHA及精确三平台CI将在提交/验证后封存；当前尚不认领CI成功或独立审计通过。

## 已实现契约

任务接口沿H-1 loopback JSONL/独立Host Token授权：`task/submit`、`task/status`、`task/list`、`task/cancel`、`task/approval`；Host固定project与policy，未知客户端字段拒绝。提交含`operation_id`及`submission`（prompt/model/provider，可选parent_task_id/continue_thread）。操作ID为1–128字符ASCII字母/数字/`-_.`，SHA256绑定序列化后的完整已校验Submission或动作载荷；同ID/同动作/hash返回原对象，同ID改载荷拒绝。原operation可在提交回应丢失时只读查询。

`submitted`仅表示Host已持久受理；首次受理可能尚无Thread/Turn及实际配置。Runtime thread/start回复确认后保存真实ID和actual policy/reviewer/sandbox/cwd及线程模型配置，检查on-request/user/readOnly及Provider相符才启动Turn；工具item完成不是任务终态。只有匹配当前stream/Thread/Turn的原生turn/completed投影为completed/failed/cancelled；明确RPC拒绝为failed/rejected，通信结果未知为unknown并停止Runtime连接，不重试。未知上游终态保留原始通知并保守标unknown。

取消使用独立operation：已知活动Turn调用原生turn/interrupt，RPC回复仅requested，等原生终态才confirmed；Thread创建期间取消会阻止后续Turn启动。取消、终态、重启使待请求unavailable。审批仍由Facade/Runtime校验原Request ID与availableDecisions；Host在写往Runtime前持久保存sending，返回sent仅表示传输写完成，不表示动作已完成。各回复完成只更新对应ID；所有其他pending/sending请求仍阻塞。跨重启的未知回应不重发。

H-2有限资源：同时一个活动执行链，最多1024任务/4096操作/每任务32原生审批记录，list每页1–100，原H-1帧/事件/replay上限保留。每任务受理后300秒到期；秒级检查发出幂等原生interrupt，不自动批准。按Host系统UTC钟判断，时钟跳变可能影响截止；尚未验生产长任务、资源调度或每请求独立expiry。上述是有限任务等待约束，非H-3完整多端审批协议。

同Provider继续仅接受已确认终态且同live stream、仍loaded的父线程，在下一turn/start指定新model，保存父Task关联；不以活动thread/resume的旧model配置假称轮次模型已变。actual.model为实际观察到的线程配置，actual.turn_model_request为Runtime已回应的轮次请求，后者不等于独立测量模型身份；演示以本机HTTP trace实际请求model验证轮次切换。新Provider profile必须独立新线程、保存parent_task_id；只发送显式prompt，不复制原opaque history、工具历史或自动拼接原文。重启旧stream不可继续，须显式新线程；这是安全拒绝，不冒充自动恢复执行。

本轮`--offline-responses http://127.0.0.1:<非零端口>/v1`仅启用两个隔离Custom Responses profile（caidex_h2_a/b、gpt-5.5/gpt-5.4），不读真实Key。两profile共用合成端点用于Host归属/新线程契约，**不代真实跨商业Provider能力验证**，既有F/G路由证据复用。未指定参数时保留H-1离线probe配置且拒绝task提交；现有配置不同拒绝，不覆盖。

Runtime原始通知继续保留未知字段及host/*中立记录；新增host/task同样不可被外部通知伪造。原生account/rateLimits/updated只记录无载荷收据runtime/rateLimitsObserved，其他account通知/认证交互继续拒绝，账户载荷不入journal。固定版本检查、最小OS环境白名单、Host Token/模型Key/Account隔离、私有目录/锁与先提交再广播保持。

## 独立复现

从最终功能SHA的干净checkout运行；Rust1.99.0、Node22、Python3。Linux需要loopback权限，Windows需PowerShell，macOS需原生Runtime允许的本地执行环境。安装项目固定Runtime（不更新全局CLI）：

```bash
npm install --prefix .tools/codex --no-audit --no-fund --ignore-scripts @openai/codex@0.160.1
export CAIDEX_CODEX_BIN="$(node scripts/codex-binary.mjs)"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
node scripts/verify-codex-schema.mjs
cargo run -p caidex-cli --locked -- doctor
cargo build -p caidex-host --locked
python3 scripts/h1-demo.py
python3 scripts/h2-demo.py --directory /absolute/new-h2-evidence-directory
cargo test -p caidex-runtime --test real_runtime --locked -- --ignored
```

PowerShell设置`$env:CAIDEX_CODEX_BIN = node scripts/codex-binary.mjs`，Python命令用`python`。演示--directory要求新目录，不替换现有资料；省略时脚本仅清理自己创建的TemporaryDirectory。保存目录含Host journal、私有Runtime home/隔离项目、trace.json与h2-result-held信号，可offline inspect（Host退出后）及SQLite只读查询；不要把用户Token传给模型端点。

预期H-2报告status=ok、submitted_tasks=5、native_marker_writes=2、model_requests=7、native_tool_offers=3、restart_model_requests=0、executed_unconfirmed_task=unknown、authorization_header_seen=false。实际7次本机请求是测量值；商业调用/真实Key不读取是场景约束，不宣称独立全局遥测。内部submitter进程在读提交回复前退出；另一端按原operation找回blocked任务；批准后标记实际写入。两端从同cursor恢复；同Provider轮次切换、新profile新线程关联、取消待审批任务；最后已执行工具而终态被合成端点持有时强杀Host，重启查回Unknown，原operation重试仍无HTTP请求/标记增量，已完成任务保留终态。

## 失败场景与限制

已加入针对性的v1只读inspect/保身份seq事务升级/迁移失败回滚、持久提交失败零Runtime调用、Host/Runtime回应丢失、配置不符拒绝、原ID/决策边界、明确RPC拒绝、取消中断受理与终态区别、早取消不启动Turn、外部host/task伪造、账户载荷省略/认证拒绝及任务截止回归。断线replay/一致snapshot、旧stream请求不可批准、结果未知不重发与已有H-1安全回归继续执行。

开发中第一次本地workspace在/tmp/.git触发既有凭据文件保护，未改保护/删除.git，改用新建Git外/var/tmp私有目录重新完整验证。Linux原生凭据附加测试初次因本机缺gnome-keyring-daemon退出；按既有CI依赖安装测试服务后重跑，不改凭据源码。最初原生演示被H-1全account通知拒绝、以及loaded thread/resume返回旧配置挡住；按固定schema加入无载荷rateLimits收据、采用原生turn/start边界，未降低认证/策略检查。

剩余限制：仅本地同OS用户/Host Token授权；没有生产Provider配置/真实商业调用、H-3跨设备身份与审批竞争/请求级expiry/Diff、完整客户端、生产吞吐/裁剪/备份或外部exactly-once承诺。工具副作用无法回滚；未知结果只能安全显示/查询/外部核实，不自动重放。Runtime关掉后的子工具终止效果未对任意工具承诺。新版本拒绝降级；v2迁移不修复旧v1未标来源的历史命名冲突。journal全snapshot随任务累计扩大，达到有限容量拒绝新操作；生产调度/保留策略尚未实现。


## 最终源码与证据（精确CI运行中）

功能源码[f674f4b22e0653a6a2b85a8d0001f810f05c207f](https://github.com/bboytang/CAIdex/commit/f674f4b22e0653a6a2b85a8d0001f810f05c207f)已提交推送main；[CI38100999321](https://github.com/bboytang/CAIdex/actions/runs/38100999321)正在该SHA运行Linux/Windows/macOS，未使用H-1旧CI认领本次结果。

[本地摘要/源码指纹/完整日志哈希](evidence/h2-local.json)：workspace672通过/0失败/83忽略，Host32/0/0（新增12项）、固定Runtime81/0/0（1 filtered），Linux原生凭据另1/0/0。fmt、全仓Clippy、构建、schema、doctor、两阶段真实进程演示全部通过；[Host实际测试名](evidence/h2-host-tests.log)、[HTTP原始合成请求证据](evidence/h2-local-http-trace.json.gz)、[开发失败/环境修正](evidence/h2-local-failed.json)。Runtime完整回归早于最后Host内部Request ID收敛修正；Facade/Runtime/fixture源码未再改变，后续完整workspace及Host真实演示已重跑，最终CI仍完整复验。

封存前会独立核对三job实际checkout SHA、各步骤、workspace/Host/Runtime实测计数及两阶段演示；源码无新改动时仅追加文档/证据提交，不把文档HEAD冒充功能CI源码。当前状态：本地开发验证完成，精确CI待完成，H-2独立审计未执行。


首次精确CI38100999321的Windows job114356728434失败：deadline定向测试替身写出turn/interrupt\r\n，而断言应为turn/interrupt\n。根因是新增Python标记写入缺少显式newline，H-1原标记已有LF约定。最小修复仅给新增替身所有标记写入指定newline="\n"，保留严格单次调用断言，不改生产Runtime或测试门槛。原日志将归档；新功能SHA必须重新完整三平台CI。


同轮最小边界复核补充：未关联线程任务的thread_id/turn_id为None；缺失ID的原始turn通知不得以None相等推进状态。投影要求两个显式ID，审批交互也要求明确ID；缺失/异线程/异Turn通知仍保留原文而不改变任务，新定向回归验证匹配真实终态才能完成。修正后本地完整workspace673/0/83、Host33/0/0（H-2新增13项），fmt/全仓Clippy及H-1/H-2真实进程演示通过。第一轮Linux/macOS成功保留历史，不能代新最终SHA的三平台精确CI。
