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

最终功能源码[`3a63c0f932477387fd8ccdb62ca401917e6a2637`](https://github.com/bboytang/CAIdex/commit/3a63c0f932477387fd8ccdb62ca401917e6a2637)已直接推送main；[精确CI38107842905](https://github.com/bboytang/CAIdex/actions/runs/38107842905)三平台全部成功。逐job原始checkout均为该完整SHA，fmt、Clippy、workspace、固定schema/doctor、H-1/2/3真实进程演示与固定Runtime离线测试步骤均成功。实际计数如下，顺序为通过/失败/忽略；Host计入workspace，ignored不当通过，固定Runtime另外显式运行原ignored测试。

| 平台及原job | workspace | Host | 固定Runtime | H-3 claim/聚合Diff事件/重启HTTP增量 |
| --- | --- | --- | --- | --- |
| [Linux job114377025918](https://github.com/bboytang/CAIdex/actions/runs/38107842905/job/114377025918) | 682/0/83 | 42/0/0 | 81/0/0 | 1/0/0 |
| [Windows job114377025755](https://github.com/bboytang/CAIdex/actions/runs/38107842905/job/114377025755) | 676/0/81 | 41/0/0 | 80/0/0 | 1/0/0 |
| [macOS job114377025910](https://github.com/bboytang/CAIdex/actions/runs/38107842905/job/114377025910) | 681/0/81 | 42/0/0 | 80/0/0 | 1/3/0 |

Linux原生凭据附加1/0/0、Windows额外Host lib4/0/0另记，不混入workspace。本地fmt、Clippy workspace/all-targets/locked/-D warnings通过；workspace682/0/83（Host42/0/0）、固定Runtime81/0/0、Linux凭据附加1/0/0、stable/experimental schema及doctor（0.160.1、d27764b82f7118f674371e6d6e76271d9d606edb、无模型Turn）、H-1/2/3真实演示全通过。H-3本地实测claim1、HTTP9、patch offer3、重启增量0；实际文件CAIDEX_H3_PATCH与原生逐文件Diff一致，Review入口/退出、双端竞争/重查/迟到、请求撤销、授权撤销/重连、超过128事件snapshot补缺口、未答请求强杀重启unknown均通过。最终源码重新构建的非root流程也通过（聚合Diff0），不是root结果替代非root验证。

[最终三平台逐job/步骤/计数/原始日志及哈希](evidence/h3-ci.json)、[本地逐检查日志与源码指纹](evidence/h3-local.json)、[Host42实际测试名](evidence/h3-host-tests.log)、[HTTP trace](evidence/h3-local-http-trace.json.gz)、[停止后schema v3快照](evidence/h3-local-snapshot.json.gz)均已封存。原日志及压缩文件分别保存SHA256；文件内容逐项对照最终提交，不依赖本会话/tmp。后续封存提交仅文档与证据，功能源码、测试、依赖、脚本及workflow与上述功能SHA一致；封存HEAD用git rev-parse HEAD查询，不把文档提交当功能CI SHA。

新增6项service测试覆盖双端首次竞争/不重复写、scope与已有连接撤销及外部host/access伪造、显式请求撤销/迟到、审批SQLite提交失败不写Runtime且重启unknown、授权提交失败不返回令牌、原生resolved/线程关闭屏障。新增3项journal测试覆盖原始Diff/Review/工具事件及未知字段/ID/旧stream隔离、64项/合计1MiB超限无部分提交、v2只读inspect/事务升级/失败回滚。Diff回归含合法同Thread/Turn终态后到达及恢复；原超时测试增强UTC回拨、单调请求/整任务上限和原生interrupt而无approval/reply证据，另覆盖取消RPC已确认却无终态、15秒截止、重复取消不延长及无自动重发。已有H-1/2环境隔离、外部事件命名空间、认证、seq/原子性/提交后广播/未知不重发仍通过。

失败记录：[开发原始失败及最小修正](evidence/h3-local-failed.json)包含旧H-2缺少授权入口的失败回归、撤销reason被Cancel覆盖的实际缺陷，以及编译/Clippy/测试或演示假设错误，均修复后复验。[首轮CI38105001809](evidence/h3-failed-ci.json)和[第二轮38105455389](evidence/h3-second-failed-ci.json)在Linux/Windows误设必有聚合Diff而失败（macOS成功）；加30秒等待仍失败，时序假设被否定。原始非rootjournal聚合通知0、原生fileChange Diff完整，固定Runtime记录sandbox写入失败并保守失效不精确累计delta，见[原始负例/正例、原生日志、上游指纹和最终非root复验](evidence/h3-runtime-diff-boundary.json)。只修正演示为强制核对真实逐文件路径/内容/恢复，聚合事件单独实测，有发出时仍强制核对，不改Runtime或权限、不自行计算Diff。计数0不称聚合成功。

中间源码dc38cfbce4827c87bd87ec2972f533db07368e89的[成功CI38106599664](evidence/h3-before-cancel-ci.json)随后被取消确认期限缺口取代，不能代最终修复验证。新[失败/修复后回归](evidence/h3-cancel-bound-regression.json)实际0/1/0→1/0/0：旧cancel-requested永久跳过期限，现持久unknown并通过原服务失败路径停止Runtime，不伪造终态。此次最小修复后完整本地检查/非root演示及本表新SHA三平台全部重新完成。

开发验证与证据封存完成，开发会话当时停止等待独立审计。2026-10-11用户另行确认H-3独立审计通过并批准下一步，按V3顺序开展I-1；这是用户告知的审计结论，不是本实施会话自授。封存源码与上述CI证据保持。

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
- 等待请求最多120秒且受300秒整任务执行预算约束；取消终态另有15秒确认截止，重复取消不延长；300秒执行预算后最多再给15秒确认窗口，截止后unknown并停止Runtime。Host按1秒tick检查，实际停止还受调度及SQLite提交耗时影响，不承诺硬实时。显式请求撤销/到期取消整个原生Turn。已被首次claim的动作不再由请求撤销回滚；已发送结果未知保留unknown，不保证外部副作用exactly-once。断线或无客户端不自动批准。
- 委托授权是同OS用户、loopback Host的有限scope能力；observe能读该Host任务/事件全域，approve能处理该Host任务审批，未提供按项目/任务ACL或官方账户身份。已发送数据/动作不能撤回；owner令牌由原启动环境管理。64个授权ID（含撤销）不能复用，需以后明确迁移/治理才能扩容，不擅自裁剪记录。
- snapshot保存最新artifact，完整已提交wire/流式delta在journal；每任务64项/合计1MiB、最多1024任务/4096操作及既有帧上限，是安全边界而非生产吞吐或无限历史承诺。没有journal裁剪、高负载/备份容灾验证，Runtime未捕获事件无法补造真源。
- 固定Runtime聚合Diff是有条件能力：[`apply-patch`](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/apply-patch/src/lib.rs)失败写入将delta标为不精确，[工具Runtime](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/tools/runtimes/apply_patch.rs)跨sandbox尝试累积delta，[tracker](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/turn_diff_tracker.rs)失效后不再输出有效聚合Diff，此前已有聚合数据时可能发空diff清除。[事件映射](https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/tools/events.rs)仍发原生fileChange完成数据。非root实测与该源码行为一致，不直接观测Runtime内存delta。演示原先误设每次编辑必有聚合事件，现强制核对原始`fileChange.changes[].diff`的路径/内容和恢复，聚合事件有发出时仍强制核对落盘/恢复，计数0不称为聚合成功。Host转交原生数据，不自行重算或把逐文件内容伪装成完整unified Diff；如果后续产品要求无条件全量聚合，需另行确认设计，当前不改变固定Runtime。
- Review只支持新线程inline uncommittedChanges；演示Review文字来自本机合成模型，验证原生链与恢复，不授模型判断质量或独立审计结论。保留现有command/file审批支持；未支持的原生互动继续失败安全拒绝。正式GUI/CLI、远程授权、商业兼容、iOS/安装包与生产Host未验。
- 报告里的SQLite/HTTP计数来自实测，文件/事件/恢复来自代码断言；不读真实Key/不调商业模型是离线场景声明，不是独立全局网络或凭据遥测。F/G-Live保持待验。

## 封存证据只读复核

以下在仓库根目录执行，只读核对所有H-3压缩证据、最终源码指纹、实际checkout和分步骤实测计数；逐条安全断言/失败场景请结合源码与原日志审查，哈希核对本身不等于独立审计。

```bash
python3 - <<'PYCODE'
import gzip, hashlib, json, re, subprocess
from pathlib import Path
root = Path('docs/evidence')
source = '3a63c0f932477387fd8ccdb62ca401917e6a2637'
local = json.loads((root / 'h3-local.json').read_text())
ci = json.loads((root / 'h3-ci.json').read_text())
assert local['source_sha'] == ci['source_sha'] == source
assert ci['run_id'] == 38107842905 and ci['conclusion'] == 'success'
checked = set()
def check(value):
    if isinstance(value, dict):
        if 'artifact' in value and 'gzip_sha256' in value:
            name = value['artifact']
            data = (root / name).read_bytes()
            assert hashlib.sha256(data).hexdigest() == value['gzip_sha256'], name
            assert hashlib.sha256(gzip.decompress(data)).hexdigest() == value['raw_sha256'], name
            checked.add(name)
        for item in value.values():
            check(item)
    elif isinstance(value, list):
        for item in value:
            check(item)
for path in root.glob('h3-*.json'):
    check(json.loads(path.read_text()))
assert checked == {path.name for path in root.glob('h3-*.gz')}, 'unreferenced archive'
for name, expected in local['source_fingerprints'].items():
    data = subprocess.check_output(['git', 'show', source + ':' + name])
    assert hashlib.sha256(data).hexdigest() == expected, name
host = (root / local['host_tests']['artifact']).read_bytes()
assert hashlib.sha256(host).hexdigest() == local['host_tests']['sha256']
names = set(re.findall(r'\btest (\S+) \.\.\. ok', host.decode()))
assert len(names) == 42 and len(ci['jobs']) == 3
for job in ci['jobs']:
    assert job['checkout_sha'] == source and job['conclusion'] == 'success'
    raw = gzip.decompress((root / job['logs']['raw']['artifact']).read_bytes()).decode()
    assert re.search(r'log -1 --format=%H\r?\n[^\n]*' + source, raw)
    stage = gzip.decompress((root / job['logs']['steps']['artifact']).read_bytes()).decode()
    for label, step in [('workspace', 'Protocol tests'), ('fixed_runtime', 'Real Runtime with local Responses fixtures')]:
        lines = [line.split('\t', 2)[2] for line in stage.splitlines() if len(line.split('\t', 2)) == 3 and line.split('\t', 2)[1] == step]
        counts = [tuple(map(int, match)) for match in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored', '\n'.join(lines))]
        assert counts and dict(zip(['passed', 'failed', 'ignored'], map(sum, zip(*counts)))) == job[label]
        assert job[label]['failed'] == 0
    assert set(job['host_tests']['names']) | set(job['host_tests']['excluded_names']) == names
    assert job['host_tests']['failed'] == job['host_tests']['ignored'] == 0
    h3 = job['h3_demo']
    assert job['h1_demo']['restart_resubmissions'] == job['h2_demo']['restart_model_requests'] == h3['restart_model_requests'] == 0
    assert h3['approval_claims'] == 1 and h3['native_patch_contents'] == 'CAIDEX_H3_PATCH'
    assert h3['native_diff_restored'] and h3['native_review_restored'] and h3['revoked_client_rejected'] and h3['revoked_request_not_executed'] and h3['snapshot_gap_recovered']
    assert h3['unanswered_task'] == 'unknown' and h3['native_diff_source'] == 'item/fileChange.changes[].diff'
    assert h3['aggregate_diff_events'] >= 0 and not h3['authorization_header_seen']
    assert all(step['conclusion'] in ('success', 'skipped') for step in job['steps'])
    print(job['platform'], job['workspace'], job['host_tests']['passed'], job['fixed_runtime'])
print('final source, actual checkout, measured counts and', len(checked), 'archive hashes verified')
PYCODE
```
