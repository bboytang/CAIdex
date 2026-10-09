# CAIdex 项目交接

更新：2026-10-09 00:02 UTC。每次先读本文件、AGENTS.md及Git状态；按V3续接，不重新规划。历史证据与协议细节见各Provider验收文档。正式项目仅`/root/projects/CAIdex-v1.0`，不改废弃`/root/projects/CAIdex`。

## 当前任务

当前续接（main/cc30145）：已实现DeepSeek多reasoning/content-part按原生顺序映射，偏移未知复用bounded raw chunk索引等待，终态补全；正文增量与工具终态校验保持。新增4项/共46项、旧42名保留，原shortcut负例改成重复index；有效RED→46/0/0、workspace427/0/45、全workspace Clippy/fmt/diff通过，精确三平台尚待commit/push。未放松原工具结果配对；只history/history_stream生产及tests/provider和4文档（7路径）。日志/tmp/caidex-deepseek-complex-{red,green,workspace,clippy}.log。

F/G：账户/长期记忆/云同步架构独立提交81debdb（功能未实现）；DeepSeek Classic函数/namespace与完整native推理历史dba1c90123b31e89f50c871e225afc817765ca7e/[CI37851276859](https://github.com/bboytang/CAIdex/actions/runs/37851276859)，后续显式effort映射32a9f3fdae8aed7519a867c1d32a8cf911f5ce09/[CI37851939704](https://github.com/bboytang/CAIdex/actions/runs/37851939704)，均已提交/push且精确三平台完整验收。此前effort范围DeepSeek29、workspace410/405/409、旧固定Runtime43/42/42通过；这不是实际DeepSeek Runtime接线或Live/Full。

当前custom apply_patch已提交/push df98a54bb247f947d9ce4014f7007c9d192056b1，精确[CI37854138352](https://github.com/bboytang/CAIdex/actions/runs/37854138352)三平台完整验收：DeepSeek37、workspace418/413/417及旧Runtime43/42/42均通过；当前summary/context/include本地编译新增5项/共42项通过，workspace423/0/45、Clippy/fmt/diff通过，已提交/push 3c1ea05a4bc5a00da503603aa4528d8fc06048fb，独立[CI37855947093](https://github.com/bboytang/CAIdex/actions/runs/37855947093)已精确三平台完整验收：workspace423/418/422、DeepSeek42及旧Runtime43/42/42通过；复杂流索引本地已通过待精确三平台，随后Lite和Classic/Lite真实固定Runtime审批/执行/取消/磁盘恢复，随后Qwen/OpenRouter；不重做已验Adapter、不跳H/I。用户已明确授权今后本地检查通过直接commit/push并执行三平台CI，逐步记录，不再额外等待。仍不读取用户Key/调用商业模型/部署。

## 已完成 / 验证

- summary/context/include收尾：有效RED编译成功400拒绝后最小lib/request实现，新增5项/共42项、旧37名保持；本地workspace423/0/45、全workspace/all-targets Clippy/fmt/diff通过。首次Clippy仅新增测试if风格，修正后42项/Clippy通过。日志/tmp/caidex-deepseek-history-controls-{red,green,workspace,clippy}.log。本次续接确认只有交接CI运行状态未提交，无遗失源码。
- 精确3c1ea05a4bc5a00da503603aa4528d8fc06048fb/CI37855947093 completed/success；Linux113580022986/Windows113580022609/macOS113580023018各17steps成功或条件跳过，完整raw1994/1681/1692行。workspace423/418/422（0failed，ignored45/43/43）、DeepSeek42逐名每平台一次、旧Runtime43/42/42（0failed/ignored）、Linux native credentials1；全workspace/credentials/Runtime/compile-fail doc-test通过名集合467/460/464等于6742b15基线加26新名，无遗漏/重复。完整日志下载及normalize/available/full-names检查均exit0，在/tmp/caidex-deepseek-history-controls-ci-source精确归档执行；watch23418暂停后句柄不存在，不冒称exit0，保存watch.log和GitHub终态确认成功，不重启CI。原始/标注日志/tmp/caidex-ci-37855947093-{linux,windows,macos}-raw.log及同前缀.log/status.json/watch.log。此证据不代验复杂流/Lite/实际DeepSeek Runtime/Live。
- 本次收尾仅README、DeepSeek/Gateway验收及HANDOFF四文档；检查源码/依赖/workflow与3c1ea05一致，16份Markdown/20本地链接/1锚点、17实体/55核心及16UI未来验收、A–R顺序与原CI证据保持。检查脚本/tmp/caidex-deepseek-history-controls-final-doc-check.py；不把文档检查当账户/GUI功能验收。

- apply_patch本地子阶段：独立with_native_apply_patch/Classic唯一custom名称、grammar仅指导、v2策略绑定（旧v1不升级），kind/结果配对及SSE delta/done/id/index/终态一致，验证后交付工具。新增8项、最终DeepSeek37逐名/旧29名保持，workspace418/0/45、Clippy全workspace/all-targets-D warnings/fmt/diff通过；有效RED新正例编译成功400拒绝。日志/tmp/caidex-deepseek-patch-{red,green,workspace,clippy}.log（green初轮36，最终37以workspace为准），local-check.py核对10 tracked/2 new路径、20本地链接及固定grammar字节。只DeepSeek5源码/测试/2fixture和4文档，无依赖/其他Provider/Runtime/workflow变化；固定d27764b源声明/handler只读/tmp/caidex-pinned-apply-patch-{spec,handler}.rs，fixture保留Apache-2.0来源。此Adapter不是实际DeepSeek Runtime、Host落盘恢复或Live。
- 本轮收尾文档检查：16份Markdown/20本地链接/1锚点、README工程路径、17实体/55核心待实施/16UI待实施、A–R原顺序及旧CI证据保留均通过；仅4文档diff，生产源码/依赖/workflow与df98a54一致，git diff --check通过。脚本/tmp/caidex-deepseek-patch-final-doc-check.py；不把文档检查当账户/GUI功能验收。
- apply_patch三平台收尾：精确源码df98a54bb247f947d9ce4014f7007c9d192056b1/[CI37854138352](https://github.com/bboytang/CAIdex/actions/runs/37854138352)整体completed/success，3job各17steps成功或条件跳过；Linux113574131063/Windows113574131960/macOS113574131216完整raw1989/1676/1687行。workspace418/413/417（0failed，ignored45/43/43）、旧固定Runtime43/42/42（0failed/ignored）、Linux native credentials1。DeepSeek37每名每平台一次；全workspace/credentials/Runtime/compile-fail doc-test通过名集合462/455/459，等于已验6742b15加21新名，无遗漏/重复。watch29915及完整日志下载/normalize/available/full-names均exit0，全部handle结束；checker /tmp/caidex-deepseek-patch-ci-{normalize,available}.py与/tmp/caidex-deepseek-ci-full-names.py在精确/tmp/caidex-deepseek-patch-ci-source归档执行，未借旧29项CI或后续文档HEAD代验。日志/tmp/caidex-ci-37854138352-{linux,windows,macos}-raw.log及同前缀.log/status.json/watch.log。

- 最新effort CI：精确32a9f3f/37851939704整体completed/success，3job各17steps成功或条件跳过；Linux113566712391/Windows113566712436/macOS113566712113完整raw1981/1668/1679行。workspace410/405/409（0failed，ignored45/43/43）、旧Runtime43/42/42（0failed/ignored）、Linux native credentials1。DeepSeek29每名逐平台一次；全workspace/credentials/Runtime/compile-fail doc-test完整通过名集合454/447/451，等于已验6742b15加13新名，无遗漏/重复。watch62556、最终下载/check85317、normalize/available/full-names均exit0，在/tmp/caidex-deepseek-effort-ci-source精确归档执行，全部handle结束。
- 函数/历史CI：精确dba1c90/37851276859整体completed/success，各3job/17steps；Linux113564495100/Windows113564495117/macOS113564494877完整raw1977/1664/1675行，workspace406/401/405、旧Runtime43/42/42均0failed，ignored45/43/43；native credentials1。DeepSeek25每名一次，全通过名集合450/443/447与旧6742b15加9新名一致；watch54073、最终下载/check79697、normalize/available/full-names均exit0，在/tmp/caidex-deepseek-tools-ci-source精确归档执行，不代验29项。两阶段checker /tmp/caidex-deepseek-{tools,effort}-ci-{normalize,available}.py及/tmp/caidex-deepseek-ci-full-names.py，原始/标注日志/tmp/caidex-ci-{37851276859,37851939704}-{linux,windows,macos}-raw.log及同前缀.log/status.json/watch.log。

- effort当前步骤：with_reasoning_effort_mapping由执行端显式配置source→native none/low/high/max，拒绝未知/重复/未配置映射和summary/context等未实现控制。只在首次源请求编译一次，native验证/历史展开不重映射；Unsupported reasoning启用前拒绝，none可用于关闭。新增配置/JSON-SSE及两轮native历史/坏控制零Key-POST/原始预算4项，定向29/0/0；RED日志/tmp/caidex-deepseek-effort-red.log（编译成功运行400失败），GREEN同前缀green.log。最终workspace410/0/45（DeepSeek29每名一次）、Clippy/fmt/diff通过，已独立提交/push32a9f3f，CI37851939704已精确三平台验收，不借25项旧head代验。
- 函数/历史本地收尾检查：15份Markdown、20个本地链接/1个锚点、README路径、17实体/55项未来核心验收/16项未来UI验收、A–R原顺序及旧CI证据均通过；15个Git变更路径严格限定8文档+7个DeepSeek源码，无依赖/其他生产源码/workflow改变。临时脚本/tmp/caidex-deepseek-tools-final-check.py，已对最终workspace核对25项DeepSeek每名一次；fmt/diff通过。
- 本轮DeepSeek新增：with_native_history/with_native_tools显式启用，Classic函数/namespace安全alias与原身份恢复，源工具/choice/parallel及native编译结果同时绑定；完整JSON/SSE native wire、明文reasoning、实际前缀与执行端owner/profile/端点/model绑定回放。终态验证后才交付工具，坏流实际断连/slot复用、预算/取消/Drop、Gateway token隔离、序列化后三轮与重复call_id拒绝。默认严格行为及原16项保持；新增9项/共25项、有效RED→最终workspace406/0/45（DeepSeek25逐名）、旧Runtime43/0/0、Clippy/fmt/diff通过。源码只DeepSeek7文件，无依赖/共享生产源码/Runtime/workflow变化；多reasoning added明确拒绝，复杂流状态仍待。序列化测试不是Host磁盘恢复，旧43项不是DeepSeek接线。日志见下方；此25项范围dba1c90三平台CI37851276859已完整验收，不代验后续29项。
- 上轮架构文档：17项逻辑实体、55项核心验收及16项UI验收已列为待实现/未执行。最终diff/独立交叉审查、15份Markdown的19个本地链接及1个锚点、README工程路径、A–R阶段顺序、数据库/账户/开关/秘密/备份一致性、既有CI证据保留和git diff --check通过；核对仅6文档变更，生产源码/依赖/工作流/HEAD未变。临时检查脚本/tmp/caidex-account-memory-doc-check.py；这是文档检查，不是新功能通过。认证库/参数、Embedding接入/索引、共有项目开关影响、迁移/墓碑保留/备份恢复点、邮件网络送达/资源容量/公开运营条件留I及后续验证。
- A–E当前范围：V3/UI/品牌/固定上游、Rust workspace/三平台CI、真实app-server facade、审批/Queue/Steer/Goal/MCP/PTY离线回归；执行端Broker及系统/受保护文件/env凭据与脱敏。
- F/G基础：canonical经典/Lite、Provider六方法/Registry证据门槛、共享Custom transport、本地Gateway独立token/取消/背压/TLS/安全错误；OpenAI原生。Anthropic/Gemini原生适配及固定Classic/Lite离线Runtime已三平台验；具体未支持能力和债务见对应docs。
- Ollama当前离线范围：原生目录/show/thinking/媒体/结构/绑定v1v2历史/namespace/custom/deferred/Lite；固定Classic MCP与Lite Code Mode审批、隔离执行、取消、磁盘恢复不重跑。精确f25179bf6e5446735b2323fc5169007ecfea947a/[CI37818503514](https://github.com/bboytang/CAIdex/actions/runs/37818503514)完整3job/17steps及逐名通过：workspace381/376/380，Runtime43/42/42。真实daemon/模型/Live未验；不重造Adapter或执行器。
- DeepSeek基础 `2967f56a7788ee90375f1e970e4500567a61cfd3`/[CI37824041219](https://github.com/bboytang/CAIdex/actions/runs/37824041219)：原生Models不要求created，完整未知字段/大数、六方法/配置交集、经典文本JSON/SSE/Gateway与独立deepseek Bearer。3job/17steps及完整raw逐名通过，workspace391/386/390（0failed，ignored45/43/43），旧Runtime43/42/42（0failed/ignored）、Linux native credentials1。job Linux113472119918/Windows113472119538/macOS113472119894，raw1962/1649/1660行；DeepSeek10/Ollama69/OpenAI11/Custom7/Google90每名一次。watch77797、下载98157及normalize/check/available均exit0，校验在精确源码归档执行。
- DeepSeek新增上下文 `6742b151b225bb695c673331ead808ecbdbc61e9`/[CI37825726756](https://github.com/bboytang/CAIdex/actions/runs/37825726756)：with_runtime_context只消费本地3头/client_metadata/cache，前置developer→system保留优先级；with_verbosity_instruction用执行端配置追加原instructions，未映射/重复映射拒绝。请求/响应未绑定turn-state拒绝，SSE实际断连/slot复用；原始及编译后预算、取消/deadline保持。新增6项/共16项、完整workspace397/0/45、Clippy全workspace/all-targets-D warnings、fmt/diff本地通过。有效RED禁用本地编译后编译成功、运行400失败；finally恢复正确源码后完整GREEN。无新依赖/共享生产源码/Runtime/workflow改动；此范围精确6742b15三平台已验（见当前任务及下方证据）。

## 下一步顺序

1. dba1c90/CI37851276859、32a9f3f/CI37851939704及df98a54/CI37854138352均已独立完整收尾，无源码变化不重跑。先核对当前Git/交接；本轮summary/context/include精确3c1ea05/CI37855947093已完整收尾，不重复运行；多reasoning/content-part本地46项已通过，先提交/push并精确三平台收尾，然后Lite custom Code Mode/单调用；不能用全量明文投影冒称原生精简summary/加密。固定Runtime include/summary请求参考runtime/bridge/tests/real_runtime.rs现有Gemini/Ollama接线与配置；DeepSeek尚未接线，不新增第二Agent/HTTP栈。每个新增范围定向验证/精确CI/交接，已验函数/namespace/native历史/effort/apply_patch不重写；原生忽略字段须显式编译或拒绝。


2. 随后Lite custom Code Mode/本地单调用交付、固定实际DeepSeek Runtime审批/执行/取消/磁盘重启，再Qwen/OpenRouter。每一步定向/相关回归、精确源码CI及交接；无源码变化不重跑已验本地全套/旧CI，不派重复独立审查。未经另行授权不调用商业API或下载模型。
3. 按V3继续H Host/SQLite journal/安全证据契约 → I官方账户/PostgreSQL/独立Chat/Memory/同步/邮件/本机恢复 → Windows → SSH/iOS → CLI → Relay → R；新设计第12/13节为I内部顺序和全部待验矩阵。iOS在GitHub建立真正simulator测试/无签名archive，Rust macOS CI不代表iOS。文档任务结束不自动开始账户实现，已确认架构不重新询问/规划。

## 重要架构决定

- Windows11x64：Tauri2+React/TypeScript/Rust；Linuxx86_64 Host/CLI；iOS17+/Swift5 SwiftUI+UniFFI。GUI中文/英文，CLI英文；assets/brand原件，UI尽量1:1参考官方，模型/API Key入口在设置。
- 固定Codex0.160.1/d27764b82f7118f674371e6d6e76271d9d606edb是唯一Runtime/工具执行器；Gateway仅适配Responses，共享Custom transport/Broker，不另造Agent/HTTP栈。未知协议信息保留，实验能力显式opt-in，经典/Lite分别验收。
- Native wire为回放权威；载体绑定执行端/profile/端点/model/compiled前缀，工具另绑定原始声明及native声明/策略，SSE raw chunks重建核对。Gemini v1/v2、Anthropic v3/v4、Ollama v1/v2不自动升级/松绑；JSON载体不是加密或来源认证。Unknown/Configured/ProviderCatalog/ProtocolFixture不授予LiveRuntime/Full。
- 新增显式Runtime+history组合允许本地summary auto（完整明文展示）、context all_turns（完整绑定展开）、唯一include carrier；不承诺摘要长短/加密/来源认证/服务端状态。source先预算再消费，只一次effort映射，无effort不编造；默认/单独policy仍拒绝，history v1/v2原样且Key前验证。
- DeepSeek原生无服务端会话；developer降级user、unknown input/内置tools忽略、parallel flag忽略、summary/encrypted_content/verbosity不等价。当前default仍拒绝高级字段/Lite/tools/reasoning输入；工具/历史有独立显式opt-in，只有完整绑定载体允许回放，单独with_native_tools仍拒绝custom；with_native_apply_patch新增v2显式策略，grammar仅指导、Runtime负责解析审批；parallel false/其他custom/deferred仍拒绝，不允许伪造reasoning；后置developer拒绝。完整原生契约与后续恢复点见docs/DeepSeek。
- Chat独立无Shell/Git/项目写权限；Remote使用Host Key，手机不读取Host已存Key，同步不含凭据。模型轮次边界切换，跨Provider关联分支/新线程。
- Host后台；SQLite journal先落盘再广播、快照补缺口、请求幂等/审批首次有效。不盲重跑未知结果、不承诺外部exactly-once；活动线程不迁移Host。Remote先SSH后Noise/Snow Relay；unsigned archive不是可安装IPA。
- 官方统一CAIdex Account，客户端注册/邮箱验证/密码/Passkey/恢复；不可变user_id，auth_sessions无强制绑定/旧设备审批/默认数量限制，独立撤销/refresh轮换。账户/Gateway token/执行端CredentialRef.owner/Host ACL各自独立；登录不授予Host执行/Key读取。正式云端账户/Chat/Memory用PostgreSQL+pgvector/RLS，客户端本地记忆/缓存和Host journal继续SQLite，替代旧自托管Chat云SQLite描述；HTTPS服务器是信任边界，无Chat/记忆E2EE承诺。
- 自动记忆、记忆同步、Chat同步、Provider发送权限独立；同步记忆默认关，登录不上传。首次开须范围确认，账户服务器权威version/epoch；离线关本机即停/待确认，确认后全账户拒绝新memory上传/下载及旧job提交。A保留云/B删云，幂等/冲突/墓碑/旧cursor/来源屏障防复活；游客/账户缓存/附件/队列隔离。关闭时本地导出可用，云管理不绕过关闭下载正文。
- Memory模型无关、来源/revision可纠错导出；个人/项目/原Chat/执行事件/临时上下文分开，Git/源码/真实测试优先，记忆不改Runtime审批。Chat/整合/Embedding角色独立，向量空间版本化/无模型基础检索；模型调用复用授权执行端ModelProvider，用户Key不上传云端，执行端离线等恢复。
- 官方云内测2核2GB50GB单实例，无GPU/强制Redis或微服务；EmailSender Brevo免费优先/Resend备用，额度/中国网络送达实测。只本机有限备份/完整性/恢复与用户导出，不异地备份，整VPS/磁盘失效可数据库+备份全损。技术选项/共有项目开关影响/旧备份缺删除账本恢复留I验证，不承诺容量/全恢复；新增矩阵和GUI均待验。

## 问题 / 阻塞

- 当前无审批/实现阻塞；apply_patch及summary/context/include三平台均完整收尾，复杂流本地通过待精确三平台；旧watch句柄缺失以GitHub终态为准。2026-10-08用户明确授权本次及后续本地检查通过直接commit/push/三平台CI，覆盖此前文档任务的暂不push限制，不重复询问。离线合成fixture及旧Runtime临时marker仍获准；不读取用户Key/调用商业API/运行官方安装脚本。

- 商业Key/真实模型/签名真实性/Full、生产Host权限/UI/iOS/真机/Windows UAC/签名未验。同步SecretStore开始后不可强停，仅保证取消后不POST；comment-only native chunk与实际Runtime下游idle单独未验。
- 既有Minor保留在Provider文档：Gemini thought-call/无tools/未opt-in none覆盖、prefix-only整组互换、projection/满槽取消、ProtoJSON整数/空ID、部分usage/thought-only phase；Anthropic重启第三轮/完整Lite custom结果覆盖。不派重复独立审查，不把暂缓项改标为修复。
- jsonschema固定0.58.6，仅arbitrary-precision与Offline retriever，无HTTP/file解析；同步求值虽有字节/Regex限制但无硬CPU抢占，留H Host隔离；不自动重试/修补坏回答。依赖历史细节见docs/Ollama与Cargo.lock。

## Git / 环境 / 相关文件

- branch main；本轮从已push cc30145续接，当前未提交仅DeepSeek history/history_stream/tests及README/DeepSeek/Gateway/HANDOFF4文档（7路径，无新文件）；46项及workspace427/0/45/Clippy/fmt/diff通过，下一步commit/push/精确三平台。旧3c1ea05/CI37855947093已完整收尾；其他旧成果保留，没有其他用户修改、依赖/共享生产源码/Runtime/workflow变化。以实际Git为准，不借旧42项CI代验46项。


- .git普通沙箱只读，提交/push需授权环境。gh bboytang；push：`git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main`，不改全局配置/输出凭据。完整测试loopback用授权环境+TMPDIR=/var/tmp，保留/tmp/.git。全局codex0.160.1可用，本机无CI专用.tools/codex；Windows/Xcode/native Linux服务缺项由CI验。磁盘约9.4G可用，修改前先df，不清源码/凭据/保护目录。
- 当前代码：model/providers/deepseek/src/{lib,config,catalog,request,tools,history,history_stream}.rs、tests/provider.rs；共享model/providers/custom、model/core、model/gateway、credentials/core；实际Runtime在runtime/bridge/tests/real_runtime.rs及fixtures。基准docs/CAIdex-实施计划-V3.md、CAIdex-UI-规范-V1.md；原V2仅需求背景。
- 函数/历史本地日志：/tmp/caidex-deepseek-tools-{red,green,workspace,runtime-regression,clippy}.log；RED正确源码备份/tmp/caidex-deepseek-tools-tools-green.rs。最终workspace406/0/45、DeepSeek25逐名、旧Runtime43/0/0均exit0，Clippy/fmt/diff通过；loopback首次沙箱PermissionDenied不算有效RED。全部工具handle结束，无正在运行的测试。
- effort本地日志：/tmp/caidex-deepseek-effort-{red,green,workspace,clippy}.log；RED编译成功运行400失败→29项GREEN及完整workspace410/0/45、Clippy/fmt/diff通过。文档检查脚本/tmp/caidex-deepseek-effort-final-check.py，CI收尾/tmp/caidex-deepseek-ci-final-doc-check.py；最终检查15份Markdown/20本地链接/1锚点、17实体/55核心待实施/16UI待实施、A–R/旧CI保留通过，账户/GUI/iOS新矩阵仍未执行。
- 旧上下文日志：/tmp/caidex-deepseek-context-{red,green,workspace,clippy}.log；正确request备份/tmp/caidex-deepseek-context-request-green.rs。有效RED1834、定向55892、完整3561均exit0；测试计数不是编译失败或初次空日志检查。
- CI证据：/tmp/caidex-ci-37824041219-{status.json,watch.log,linux-raw.log,windows-raw.log,macos-raw.log}及标注日志；旧基础校验归档/tmp/caidex-deepseek-basic-ci-source。新增CI状态/tmp/caidex-ci-37825726756-status.json；校验脚本/tmp/caidex-deepseek-context-ci-{normalize,check,available}.py，在精确6742b15归档/tmp/caidex-deepseek-context-ci-source执行，不能用后续工作区新增测试代验旧head。跨机器以已提交docs和原CI为准。

- 旧上下文CI收尾：6742b15/37825726756的3job/17steps均completed且成功或条件跳过，Linux113477961713/Windows113477962030/macOS113477961883完整raw1968/1655/1666行。workspace397/392/396（0failed，ignored45/43/43）、旧Runtime43/42/42（0failed/ignored）、Linux native credentials1；DeepSeek16/Ollama69/OpenAI11/Custom7/Google90及全部Runtime每平台每名一次。watch53780、最终下载13789及normalize/check/available均exit0，在精确6742b15归档核对；所有工具handle已结束，无待运行测试/CI。原始/标注日志/tmp/caidex-ci-37825726756-{linux,windows,macos}-raw.log及同名前缀.log/status.json/watch.log。实际DeepSeek Runtime/Live仍未验。
