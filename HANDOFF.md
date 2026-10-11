# CAIdex 开发交接

更新：2026-10-11。正式目录 `/root/projects/CAIdex-v1.0`；恢复依据为仓库、源码、验收文档及精确GitHub CI，不依赖旧聊天或/tmp日志。

## 当前批准工作：H-1 独立审计阻断修复

用户本轮仅授权修复AUD-001版本子进程环境继承、AUD-002Runtime通知污染Host命名空间；AUD-003仅最小证据标注。审计基线/main/HEAD/origin/main/GitHub main均已核实为`b516dbae41b252d365d8fad8848c197f2f6ccd51`，初始工作区干净；旧已验源码`dec3a374228a75b9a6600715b0d56bf5047b12b0`。构建前约3.1GB空闲，保留有效缓存及全部用户/全局Codex文件。

有限计划：①复现两阻断并明确共享边界；②版本与Runtime共用原OS白名单，外部host/*安全记录；③合成凭据/伪造事件回归、合法恢复/完整workspace/固定Runtime/schema/doctor/真实双端演示；④最小diff/提交push main/最终源码精确三平台CI/证据封存。非目标：H-2/H-3/I、商业调用/真实Key、全量journal重构或协议改版。通过条件：原路径不再泄漏敏感环境、不接受外部Host控制事件，原契约无回归，新增回归及最终源码三平台全部通过。停止条件：封存证据后停止，等待用户重新发起H-1独立审计；开发验证不授独立审计通过。

当前进度：原边界已由新增回归复现失败；最小修复后H-1共19项定向通过、fmt及全仓Clippy通过。Linux完整workspace659/0/83、固定Runtime81/0/0、stable/experimental schema、doctor及真实双端强杀重启演示全部通过；待提交本轮源码及最终精确三平台CI。使用fix-finding的只读边界调查作为开发验证，不冒充H-1独立验收；用户已提供审计发现，尚未重新审计修复结果。

## H-1 初次交付历史（以下证据不代本轮修复验证）

2026-10-10 用户确认 F/G-Offline 验收通过并正式关闭；F/G-Live 保持待验，不认领商业兼容性或生产Host。用户明确批准 H-1，实现/测试/演示/commit/push main/三平台精确CI，完成后停止；H-2/H-3/I 尚未授权。

实施前核对：main/HEAD/远端 main 均为 `b8dfec41bf06b2237b6c7ca2d7f91c91b7ec4322`，初始工作区干净；V3第4节H-1及CLI第10节为验收依据。磁盘约4GB空闲，target约17GB；复用缓存、限制并行，不删除用户/全局Codex资料。

有限计划：①核对Facade/生命周期边界并记录设计；②新增独立本地Host及V1 SQLite journal、Host/stream sequence、提交后广播、attach/detach、snapshot恢复；③双内部客户端/真实固定Runtime无模型初始化与线程探针、故障测试；④最终diff/提交/push/精确三平台CI/审计证据。非目标：H-2任务/幂等提交/执行链、H-3审批竞争/Diff、正式GUI/CLI、SSH/Relay、账户/Memory、商业调用/用户Key和生产部署。

通过条件：客户端退出不终止Host；两个测试客户端断线重连恢复提交事件或一致快照；重启journal保持Host身份/seq且旧Runtime状态未知，不重发未确认动作；SQLite版本/损坏/提交失败安全拒绝；实际演示、测试、Linux/Windows/macOS精确CI及可独立审计证据。停止条件：达到上述条件即停止新增功能，等待用户验收及下一授权；真实阻断须修复复验，资源不足不得擅自清理。

依赖：固定Codex0.160.1、现有Runtime Facade、Tokio/serde与新增必要SQLite绑定；隔离运行数据，不接收Key/Account Token，不改现有凭据边界。当前进度：H-1实现与验证完成，精确三平台CI全部成功，交付后停止；用户验收及独立审计未执行。

## H-1 当前进度与证据

已实现 `runtime/host` 独立本地Host、固定Runtime Facade复用、V1 SQLite journal/event+snapshot事务、Host/stream双seq、提交后广播、严格内部JSONL请求、双端attach/detach/连续replay或一致snapshot。线程探针先记录未知intent再发请求；重启保留未知intent、旧Runtime缓存失效，不重发。显式shutdown先记录stopping；Runtime断连/交互/存储失败安全停止；inspect只读、不建库/迁移。Windows owner-only ACL、Unix0700/owner/链接检查，独立Host token不进入Runtime/journal/日志。

最终功能源码提交：`dec3a374228a75b9a6600715b0d56bf5047b12b0`，已push；[精确CI38095166217](https://github.com/bboytang/CAIdex/actions/runs/38095166217)三平台完整成功，完整日志逐项核对最终源码SHA、测试名/计数、schema/doctor及真实双端演示。

- Linux job114339472553：workspace655/0/83，H-1含于其中15/0/0，固定Runtime81/0/0，另原生凭据service1/0/0。
- Windows job114339472633：workspace649/0/81，H-1含于其中14/0/0，固定Runtime80/0/0；生产ACL脚本及真实Rust子进程边界两项成功。Unix专属权限/硬链接测试不在Windows执行。
- macOS job114339472658：workspace654/0/81，H-1含于其中15/0/0，固定Runtime80/0/0。三平台fmt/全仓Clippy/schema/doctor均成功，ignored不计通过。

三平台实际独立Host进程演示通过：两客户端断线重连replay一致，第三观察客户端进程退出不终止Host；真实强杀重启同Host/journal，旧线程unknown，0探针重发/模型轮次/商业调用/用户Key读取。Linux CI seq6→9，Windows/macOS seq5→7，实际Runtime通知可推进seq，不固定事件数。最终Linux本地15项/Host Clippy/fmt/真实演示也通过。磁盘约3.1GB空闲，无用户/全局Codex清理。

[H-1设计/运行/失败与限制](docs/CAIdex-H1-Host-设计与验收-V1.md)记录3次失败CI与最小修复：Windows子进程PSModulePath/路径/LF边界、macOS参数journal锁析构晚于任务completion、危险交互先于夹具注入确认；均由最后精确CI复验。严格请求拒绝额外cwd/apiKey无副作用，提交失败无动作/广播，未知intent不重发。初始/tmp为Git工作树造成凭据测试拒绝，换新建Git外私有TMPDIR，不改变保护。

审计证据：[最终CI摘要及完整步骤/测试名/演示JSON/日志SHA256](docs/evidence/h1-ci.json)，同目录三平台完整原始日志gzip归档；[最终本地测试/文件指纹](docs/evidence/h1-final-local.json)、[最终本地日志](docs/evidence/h1-final-host-tests.log)。初版h1-local.json/h1-host-tests.log仅对应db8bf47及早期本地检查，不冒充最终源码。所有证据可从仓库/GitHub复核，不依赖实施会话/tmp日志。

功能源码停止新增；本次封存仅HANDOFF/验收文档/docs/evidence，源码/依赖/workflow与上述已验SHA一致。main沿用既有提交/push流程；恢复时读取Git核实最新文档封存HEAD/远端及实际未提交文件，不将文档HEAD误作Rust CI源码SHA。准确下一步为用户安排独立新会话只读审计及H-1验收；实施自查/CI不等于独立审计。H-2/H-3/I仍未授权，未经用户确认不得启动。

## F/G 封存状态

F/G-Offline已由用户正式确认关闭。九Adapter/18条route/4条切换的原证据复用[退出核对](docs/CAIdex-FG-离线验收核对-V1.md)，固定源码586199f/[CI38090495112](https://github.com/bboytang/CAIdex/actions/runs/38090495112)三平台全成功（workspace640/635/639，Runtime81/80/80）。本次未改Facade/Provider/凭据源码，最新功能变更仅H-1及必要workspace/依赖/workflow。

F/G-Live仍待真实模型版本/签名/费用、实际Ollama等用户最终实测；没有用户Key读取或商业调用，不授LiveRuntime/Full，不认领生产Host已通过。前次治理b8dfec4已结束，不是当前待办；独立审计未新增执行记录。

## 已确定的架构边界（不得擅自改动）

- 唯一Agent/工具执行器/审批真源：Codex **0.160.1 / d27764b82f7118f674371e6d6e76271d9d606edb**。复用Runtime facade、ModelProvider、Gateway、Credential Broker及Custom传输，不另造执行器、审批引擎或模型HTTP栈；未知wire保留，Classic/Lite分别验。
- Windows11x64 Tauri2+React/TypeScript/Rust；Linuxx86_64 Host/CLI；iOS17+ SwiftUI/Swift5+UniFFI。GUI默认简中、英文备选，CLI全英文。用户品牌原件为准，布局尽量参考同平台官方，Key入口在设置；不新增Android/Web/macOS原生GUI。
- Registry目录/配置/协议fixture不授LiveRuntime/Full；native载体绑定执行端/profile/endpoint/model/完整前缀/工具政策。不能自动升级、跨Provider复用签名或放宽历史绑定；JSON载体不是加密/真实性证明。
- Host管理线程/任务/真实审批；退出GUI/CLI/SSH只断连接，不等于取消。journal先落盘再广播，快照/seq补缺口，审批首次有效，不盲重跑未知结果、不承诺外部exactly-once。切Host不迁活动线程；模型轮次边界切换，跨Provider关联分支/新线程。
- 普通Chat独立，没有Shell/Git/项目写权限；API Key归实际执行端，Remote不读Host已存Key，不进Git/日志/Chat/Memory/普通同步。Account Token、Gateway token、CredentialRef.owner、SSH/Relay/Host ACL各自独立；登录账户不授执行/Key读取/代审批权。
- 官方统一Account，不可变user_id；Windows/iOS应用内注册/密码/Passkey/恢复。CLI只登录已有账户：浏览器Authorization Code+PKCE、VPS/SSH Device Grant；公开客户端无长期secret。auth_sessions独立撤销/refresh轮换，不强制设备绑定、旧设备批准或默认设备数量限制。
- 正式云端账户/Chat/Memory使用PostgreSQL+pgvector/RLS与版本化迁移；Windows/iOS/Linux CLI本地缓存/记忆、Host journal继续SQLite且逻辑分离。不退回用户自建身份或云SQLite临时路径。未登录/云故障不破坏已有本地权限能力。
- Memory模型无关，个人/项目/Chat原文/执行事件/临时上下文分开，来源/revision可纠错导出，Git/源码/实际测试优先；记忆和外部文本不能提升Runtime权限。Chat/整合/Embedding角色独立，空间版本化，无模型可基础检索；整合在授权执行端复用ModelProvider，不复制Key到云后台。
- 自动记忆、Memory Sync、Chat同步、Provider外发权限独立。新账户Memory Sync默认关；三平台登录继承同一服务端enabled/settings_version/cloud_epoch，Enabled不授权上传匿名/其他账户/未许可旧资料。首次范围确认；离线关本机立即停传/待服务器确认，确认后全账户拒绝新memory上传/下载/旧job提交；A保留云/B重认证删云，以epoch/tombstone/来源屏障防复活；账户切换隔离本地资料。
- 云内测2核2GB50GB单实例，无GPU/强制Redis/Kubernetes；可替换EmailSender（Brevo优先/Resend备用），实际额度/中国网络送达另验。只本机有限备份/恢复与用户导出，无异地自动备份；整VPS/磁盘损坏可能数据库+备份全损，本地缓存不是全量云备份，不承诺容量或全故障恢复。
- Remote先SSH，Q沿Noise/Snow E2EE配对/撤销；不与账户登录设备混用。exec报告实际解析的无头审批策略（通常Never、特定AutoReview例外），不代答人工请求；持久task允许独立Host授权的跨端审批，submit成功不等于任务成功。
- iOS测试/最终构建在GitHub macOS真正simulator/无签名archive；真机/签名另验，无签名archive不是可安装IPA。当前三平台Rust CI不代GUI或iOS验收。

## 未实现、未验证与风险

- H-1 journal/seq/snapshot/本地生命周期已实现；H-2持久任务/幂等/执行链、H-3审批竞争/Diff、活动轮次模型约束与跨Provider持久关联/完整生产Host仍未实现。稳定thread/start无原始history；实验thread/resume.history为云专用禁止入口，未使用。
- I官方账户/PostgreSQL/Memory/同步/Email及J–O客户端、L SSH、P完整英文CLI、Q加密Relay、R实际平台/安装升级/权限/恢复仍待。当前CLI仅doctor、credentials status/set/remove、version/help；CLI-01～34不因底层协议存在而完成。
- 各Provider限制以专属验收/profile为准；未知Runtime扩展不保证类型化持久化全量往返。native载体为JSON一致性门控而非密码学真实性，backend配置不是实际endpoint证明；终态缓冲受idle/预算限制，完整前缀可能二次增长。
- compaction只验明确Classic/Lite本地/远端成功与失败/取消、Total采样前自动阈值/磁盘恢复；其他scope/轮末/TokenBudget/无限历史不认领。同步SecretStore开始后不能强停，保证取消后不POST；跨Gateway keepalive未承诺。
- H-1无journal裁剪/高负载吞吐/备份恢复承诺；SQLite提交在服务任务同步执行、同OS用户授权域，Runtime丢事件无法凭已捕获日志重建完整真源。jsonschema0.58.6有字节/regex限制但无硬CPU抢占；后续H须分析生产隔离/有界失败，此限制不写成已解决。当前未发现有实际证据的新F/G阻断；后续若出现影响权限/隔离/数据完整性/后续正确性的明确缺陷，重新纳入阻断分析。

## 恢复入口

- [AGENTS规则](AGENTS.md)、[V3阶段/里程碑/交付](docs/CAIdex-实施计划-V3.md)、[UI规范](docs/CAIdex-UI-规范-V1.md)、[Account/Memory/Cloud设计](docs/CAIdex-Account-Memory-Cloud-设计与验收-V1.md)、[CLI-01～34](docs/CAIdex-CLI-完整交互与验收规范-V1.md)。
- [F/G退出核对](docs/CAIdex-FG-离线验收核对-V1.md)、[Gateway](docs/CAIdex-Model-Gateway-设计与验收.md)、[Runtime能力](docs/CAIdex-Runtime-能力对照.md)、[18条Provider路由报告](docs/CAIdex-Provider-路由离线兼容性报告-V1.md)、[4条切换报告](docs/CAIdex-模型切换-离线兼容性报告-V1.md)。各Provider专属验收由上述索引进入，历史CI留原文/GitHub。
- H-1源码/测试 `runtime/host/`、真实进程演示 `scripts/h1-demo.py`；既有源码 `runtime/bridge/src/`、`model/core/src/`、`model/gateway/src/`、`model/providers/`、`credentials/core/`；固定Runtime/切换/Provider集成测试位于 `runtime/bridge/tests/`。当前只有H-1本地Host探针，无完整生产任务Host/Windows/iOS工程；完成H-1后不自行开始H-2。
