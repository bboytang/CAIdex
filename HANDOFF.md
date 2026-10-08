# CAIdex 项目交接

更新：2026-10-08 22:04 UTC。每次先读本文件、AGENTS.md及Git状态；按V3续接，不重新规划。历史证据与协议细节见各Provider验收文档。正式项目仅`/root/projects/CAIdex-v1.0`，不改废弃`/root/projects/CAIdex`。

## 当前任务

账户/长期记忆/云同步文档任务已完成；独立架构提交81debdb已创建（V3/UI/Credentials/新Account设计及README对应部分），功能未实现。续接先核对HEAD/旧CI/文档检查与Git恢复点，未发现丢失或未完成的旧源码；随后完成F/G的DeepSeek Classic函数/namespace映射、明文reasoning与执行端/实际前缀/源声明绑定历史的本地子阶段。新增9项/共25项，最终workspace406/0/45、旧固定Runtime43/0/0、Clippy/fmt/diff通过。检查发现的多native reasoning流摘要索引风险已收紧为拒绝并覆盖回归。当前函数/历史范围本地通过，账户架构已独立提交81debdb，用户已明确授权此次及以后本地检查通过后直接commit/push和三平台CI，不再重复询问；正在提交此范围，精确新head/CI待记录。下一项正在核对/实现执行端显式effort映射；custom apply_patch的grammar语义仍需固定Runtime/原生契约证据，不认领已支持。未读取用户Key/调用商业模型/部署，不跳I。

已提交的F/G基线仍为DeepSeek基础与显式Runtime上下文/verbosity适配：最新生产提交`6742b151b225bb695c673331ead808ecbdbc61e9`/[CI37825726756](https://github.com/bboytang/CAIdex/actions/runs/37825726756)整体completed/success，DeepSeek16、workspace397/392/396、旧Runtime43/42/42均通过。该CI不证明本轮新增25项范围。custom/deferred/Lite/推理控制/实际DeepSeek Runtime/Live/Full及完整F/G、H–R未完成。

## 已完成 / 验证

- 本轮收尾检查：15份Markdown、20个本地链接/1个锚点、README路径、17实体/55项未来核心验收/16项未来UI验收、A–R原顺序及旧CI证据均通过；15个Git变更路径严格限定8文档+7个DeepSeek源码，无依赖/其他生产源码/workflow改变。临时脚本/tmp/caidex-deepseek-tools-final-check.py，已对最终workspace核对25项DeepSeek每名一次；fmt/diff通过。
- 本轮DeepSeek新增：with_native_history/with_native_tools显式启用，Classic函数/namespace安全alias与原身份恢复，源工具/choice/parallel及native编译结果同时绑定；完整JSON/SSE native wire、明文reasoning、实际前缀与执行端owner/profile/端点/model绑定回放。终态验证后才交付工具，坏流实际断连/slot复用、预算/取消/Drop、Gateway token隔离、序列化后三轮与重复call_id拒绝。默认严格行为及原16项保持；新增9项/共25项、有效RED→最终workspace406/0/45（DeepSeek25逐名）、旧Runtime43/0/0、Clippy/fmt/diff通过。源码只DeepSeek7文件，无依赖/共享生产源码/Runtime/workflow变化；多reasoning added明确拒绝，复杂流状态仍待。序列化测试不是Host磁盘恢复，旧43项不是DeepSeek接线。日志见下方；新范围CI尚未执行。
- 上轮架构文档：17项逻辑实体、55项核心验收及16项UI验收已列为待实现/未执行。最终diff/独立交叉审查、15份Markdown的19个本地链接及1个锚点、README工程路径、A–R阶段顺序、数据库/账户/开关/秘密/备份一致性、既有CI证据保留和git diff --check通过；核对仅6文档变更，生产源码/依赖/工作流/HEAD未变。临时检查脚本/tmp/caidex-account-memory-doc-check.py；这是文档检查，不是新功能通过。认证库/参数、Embedding接入/索引、共有项目开关影响、迁移/墓碑保留/备份恢复点、邮件网络送达/资源容量/公开运营条件留I及后续验证。
- A–E当前范围：V3/UI/品牌/固定上游、Rust workspace/三平台CI、真实app-server facade、审批/Queue/Steer/Goal/MCP/PTY离线回归；执行端Broker及系统/受保护文件/env凭据与脱敏。
- F/G基础：canonical经典/Lite、Provider六方法/Registry证据门槛、共享Custom transport、本地Gateway独立token/取消/背压/TLS/安全错误；OpenAI原生。Anthropic/Gemini原生适配及固定Classic/Lite离线Runtime已三平台验；具体未支持能力和债务见对应docs。
- Ollama当前离线范围：原生目录/show/thinking/媒体/结构/绑定v1v2历史/namespace/custom/deferred/Lite；固定Classic MCP与Lite Code Mode审批、隔离执行、取消、磁盘恢复不重跑。精确f25179bf6e5446735b2323fc5169007ecfea947a/[CI37818503514](https://github.com/bboytang/CAIdex/actions/runs/37818503514)完整3job/17steps及逐名通过：workspace381/376/380，Runtime43/42/42。真实daemon/模型/Live未验；不重造Adapter或执行器。
- DeepSeek基础 `2967f56a7788ee90375f1e970e4500567a61cfd3`/[CI37824041219](https://github.com/bboytang/CAIdex/actions/runs/37824041219)：原生Models不要求created，完整未知字段/大数、六方法/配置交集、经典文本JSON/SSE/Gateway与独立deepseek Bearer。3job/17steps及完整raw逐名通过，workspace391/386/390（0failed，ignored45/43/43），旧Runtime43/42/42（0failed/ignored）、Linux native credentials1。job Linux113472119918/Windows113472119538/macOS113472119894，raw1962/1649/1660行；DeepSeek10/Ollama69/OpenAI11/Custom7/Google90每名一次。watch77797、下载98157及normalize/check/available均exit0，校验在精确源码归档执行。
- DeepSeek新增上下文 `6742b15`：with_runtime_context只消费本地3头/client_metadata/cache，前置developer→system保留优先级；with_verbosity_instruction用执行端配置追加原instructions，未映射/重复映射拒绝。请求/响应未绑定turn-state拒绝，SSE实际断连/slot复用；原始及编译后预算、取消/deadline保持。新增6项/共16项、完整workspace397/0/45、Clippy全workspace/all-targets-D warnings、fmt/diff本地通过。有效RED禁用本地编译后编译成功、运行400失败；finally恢复正确源码后完整GREEN。无新依赖/共享生产源码/Runtime/workflow改动；此范围精确6742b15三平台已验（见当前任务及下方证据）。

## 下一步顺序

1. 先核对本轮DeepSeek7源码文件、8文档未提交状态和末次测试；用户已授权本地检查通过后直接commit/push，以精确新提交进行三平台CI并完整逐名核对，不能借6742b15代验新范围。Classic函数/namespace与明文reasoning绑定历史本地已验，不重写。随后按固定Runtime真实请求补custom apply_patch、effort/summary/context/include等控制，复杂reasoning流需明确索引映射再开放；每个新增范围有定向证据。原生忽略/降级字段明确编译或拒绝，不丢历史/擅降级。
2. 随后Lite custom Code Mode/本地单调用交付、固定实际DeepSeek Runtime审批/执行/取消/磁盘重启，再Qwen/OpenRouter。每一步定向/相关回归、精确源码CI及交接；无源码变化不重跑已验本地全套/旧CI，不派重复独立审查。未经另行授权不调用商业API或下载模型。
3. 按V3继续H Host/SQLite journal/安全证据契约 → I官方账户/PostgreSQL/独立Chat/Memory/同步/邮件/本机恢复 → Windows → SSH/iOS → CLI → Relay → R；新设计第12/13节为I内部顺序和全部待验矩阵。iOS在GitHub建立真正simulator测试/无签名archive，Rust macOS CI不代表iOS。文档任务结束不自动开始账户实现，已确认架构不重新询问/规划。

## 重要架构决定

- Windows11x64：Tauri2+React/TypeScript/Rust；Linuxx86_64 Host/CLI；iOS17+/Swift5 SwiftUI+UniFFI。GUI中文/英文，CLI英文；assets/brand原件，UI尽量1:1参考官方，模型/API Key入口在设置。
- 固定Codex0.160.1/d27764b82f7118f674371e6d6e76271d9d606edb是唯一Runtime/工具执行器；Gateway仅适配Responses，共享Custom transport/Broker，不另造Agent/HTTP栈。未知协议信息保留，实验能力显式opt-in，经典/Lite分别验收。
- Native wire为回放权威；载体绑定执行端/profile/端点/model/compiled前缀，工具另绑定原始声明及native声明/策略，SSE raw chunks重建核对。Gemini v1/v2、Anthropic v3/v4、Ollama v1/v2不自动升级/松绑；JSON载体不是加密或来源认证。Unknown/Configured/ProviderCatalog/ProtocolFixture不授予LiveRuntime/Full。
- DeepSeek原生无服务端会话；developer降级user、unknown input/内置tools忽略、parallel flag忽略、summary/encrypted_content/verbosity不等价。当前default仍拒绝高级字段/Lite/tools/reasoning输入；工具/历史有独立显式opt-in，只有完整绑定载体允许回放，原生parallel false/custom/deferred仍拒绝，不允许伪造reasoning；后置developer拒绝。完整原生契约与后续恢复点见docs/DeepSeek。
- Chat独立无Shell/Git/项目写权限；Remote使用Host Key，手机不读取Host已存Key，同步不含凭据。模型轮次边界切换，跨Provider关联分支/新线程。
- Host后台；SQLite journal先落盘再广播、快照补缺口、请求幂等/审批首次有效。不盲重跑未知结果、不承诺外部exactly-once；活动线程不迁移Host。Remote先SSH后Noise/Snow Relay；unsigned archive不是可安装IPA。
- 官方统一CAIdex Account，客户端注册/邮箱验证/密码/Passkey/恢复；不可变user_id，auth_sessions无强制绑定/旧设备审批/默认数量限制，独立撤销/refresh轮换。账户/Gateway token/执行端CredentialRef.owner/Host ACL各自独立；登录不授予Host执行/Key读取。正式云端账户/Chat/Memory用PostgreSQL+pgvector/RLS，客户端本地记忆/缓存和Host journal继续SQLite，替代旧自托管Chat云SQLite描述；HTTPS服务器是信任边界，无Chat/记忆E2EE承诺。
- 自动记忆、记忆同步、Chat同步、Provider发送权限独立；同步记忆默认关，登录不上传。首次开须范围确认，账户服务器权威version/epoch；离线关本机即停/待确认，确认后全账户拒绝新memory上传/下载及旧job提交。A保留云/B删云，幂等/冲突/墓碑/旧cursor/来源屏障防复活；游客/账户缓存/附件/队列隔离。关闭时本地导出可用，云管理不绕过关闭下载正文。
- Memory模型无关、来源/revision可纠错导出；个人/项目/原Chat/执行事件/临时上下文分开，Git/源码/真实测试优先，记忆不改Runtime审批。Chat/整合/Embedding角色独立，向量空间版本化/无模型基础检索；模型调用复用授权执行端ModelProvider，用户Key不上传云端，执行端离线等恢复。
- 官方云内测2核2GB50GB单实例，无GPU/强制Redis或微服务；EmailSender Brevo免费优先/Resend备用，额度/中国网络送达实测。只本机有限备份/完整性/恢复与用户导出，不异地备份，整VPS/磁盘失效可数据库+备份全损。技术选项/共有项目开关影响/旧备份缺删除账本恢复留I验证，不承诺容量/全恢复；新增矩阵和GUI均待验。

## 问题 / 阻塞

- 本轮新增DeepSeek范围本地已验，精确三平台CI尚缺。2026-10-08用户明确授权本次及后续本地检查通过后直接commit/push/三平台CI，覆盖上轮文档任务的暂不push限制；不再为同一范围重复询问。已使用获准的离线合成fixture和旧Runtime临时marker回归；不读取用户模型Key/调用商业API/运行官方安装脚本。
- 商业Key/真实模型/签名真实性/Full、生产Host权限/UI/iOS/真机/Windows UAC/签名未验。同步SecretStore开始后不可强停，仅保证取消后不POST；comment-only native chunk与实际Runtime下游idle单独未验。
- 既有Minor保留在Provider文档：Gemini thought-call/无tools/未opt-in none覆盖、prefix-only整组互换、projection/满槽取消、ProtoJSON整数/空ID、部分usage/thought-only phase；Anthropic重启第三轮/完整Lite custom结果覆盖。不派重复独立审查，不把暂缓项改标为修复。
- jsonschema固定0.58.6，仅arbitrary-precision与Offline retriever，无HTTP/file解析；同步求值虽有字节/Regex限制但无硬CPU抢占，留H Host隔离；不自动重试/修补坏回答。依赖历史细节见docs/Ollama与Cargo.lock。

## Git / 环境 / 相关文件

- branch main；本轮起点/origin-main `adb90e5c96bac678d249ce79219105ce44395b69`，当前HEAD81debdb，起点有6份未提交架构文档（非干净工作区），最新生产提交仍6742b151b225bb695c673331ead808ecbdbc61e9。架构5文档独立提交81debdb（README按改动范围暂存，工作文件未覆盖）。当前11路径未提交：HANDOFF/README的DeepSeek段、DeepSeek/Gateway验收共4文档；DeepSeek修改config/lib/request/tests-provider，新增tools/history/history_stream共7源码文件。本地已验，CI/后续高级能力未完成；准备分开提交已确认账户架构文档与DeepSeek函数/历史范围，不覆盖既有成果，Cargo.lock/依赖未变。
- .git普通沙箱只读，提交/push需授权环境。gh bboytang；push：`git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main`，不改全局配置/输出凭据。完整测试loopback用授权环境+TMPDIR=/var/tmp，保留/tmp/.git。全局codex0.160.1可用，本机无CI专用.tools/codex；Windows/Xcode/native Linux服务缺项由CI验。磁盘约9.4G可用，修改前先df，不清源码/凭据/保护目录。
- 当前代码：model/providers/deepseek/src/{lib,config,catalog,request,tools,history,history_stream}.rs、tests/provider.rs；共享model/providers/custom、model/core、model/gateway、credentials/core；实际Runtime在runtime/bridge/tests/real_runtime.rs及fixtures。基准docs/CAIdex-实施计划-V3.md、CAIdex-UI-规范-V1.md；原V2仅需求背景。
- 本轮日志：/tmp/caidex-deepseek-tools-{red,green,workspace,runtime-regression,clippy}.log；RED正确源码备份/tmp/caidex-deepseek-tools-tools-green.rs。最终workspace406/0/45、DeepSeek25逐名、旧Runtime43/0/0均exit0，Clippy/fmt/diff通过；loopback首次沙箱PermissionDenied不算有效RED。全部工具handle结束，无正在运行的测试。
- 旧上下文日志：/tmp/caidex-deepseek-context-{red,green,workspace,clippy}.log；正确request备份/tmp/caidex-deepseek-context-request-green.rs。有效RED1834、定向55892、完整3561均exit0；测试计数不是编译失败或初次空日志检查。
- CI证据：/tmp/caidex-ci-37824041219-{status.json,watch.log,linux-raw.log,windows-raw.log,macos-raw.log}及标注日志；旧基础校验归档/tmp/caidex-deepseek-basic-ci-source。新增CI状态/tmp/caidex-ci-37825726756-status.json；校验脚本/tmp/caidex-deepseek-context-ci-{normalize,check,available}.py，在精确6742b15归档/tmp/caidex-deepseek-context-ci-source执行，不能用后续工作区新增测试代验旧head。跨机器以已提交docs和原CI为准。

- 最新CI收尾：6742b15/37825726756的3job/17steps均completed且成功或条件跳过，Linux113477961713/Windows113477962030/macOS113477961883完整raw1968/1655/1666行。workspace397/392/396（0failed，ignored45/43/43）、旧Runtime43/42/42（0failed/ignored）、Linux native credentials1；DeepSeek16/Ollama69/OpenAI11/Custom7/Google90及全部Runtime每平台每名一次。watch53780、最终下载13789及normalize/check/available均exit0，在精确6742b15归档核对；所有工具handle已结束，无待运行测试/CI。原始/标注日志/tmp/caidex-ci-37825726756-{linux,windows,macos}-raw.log及同名前缀.log/status.json/watch.log。实际DeepSeek Runtime/Live仍未验。
