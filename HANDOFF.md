# CAIdex 项目交接

更新：2026-10-08 18:47 UTC。每次先读本文件、AGENTS.md及Git状态；按V3续接，不重新规划。历史证据与协议细节见各Provider验收文档。

## 当前任务

F/G：DeepSeek基础及新增显式Runtime上下文/verbosity适配均已提交/push并三平台完整逐名验收。最新源码 `6742b151b225bb695c673331ead808ecbdbc61e9`/[CI37825726756](https://github.com/bboytang/CAIdex/actions/runs/37825726756)整体completed/success，DeepSeek16、workspace397/392/396、旧Runtime43/42/42均通过。下一步DeepSeek工具/namespace与明文reasoning绑定历史；当前无未完成源码。上下文适配不等于实际DeepSeek Runtime接线，tools/Lite/Live/Full及完整F/G、H–R未完成。

## 已完成 / 验证

- A–E当前范围：V3/UI/品牌/固定上游、Rust workspace/三平台CI、真实app-server facade、审批/Queue/Steer/Goal/MCP/PTY离线回归；执行端Broker及系统/受保护文件/env凭据与脱敏。
- F/G基础：canonical经典/Lite、Provider六方法/Registry证据门槛、共享Custom transport、本地Gateway独立token/取消/背压/TLS/安全错误；OpenAI原生。Anthropic/Gemini原生适配及固定Classic/Lite离线Runtime已三平台验；具体未支持能力和债务见对应docs。
- Ollama当前离线范围：原生目录/show/thinking/媒体/结构/绑定v1v2历史/namespace/custom/deferred/Lite；固定Classic MCP与Lite Code Mode审批、隔离执行、取消、磁盘恢复不重跑。精确f25179bf6e5446735b2323fc5169007ecfea947a/[CI37818503514](https://github.com/bboytang/CAIdex/actions/runs/37818503514)完整3job/17steps及逐名通过：workspace381/376/380，Runtime43/42/42。真实daemon/模型/Live未验；不重造Adapter或执行器。
- DeepSeek基础 `2967f56a7788ee90375f1e970e4500567a61cfd3`/[CI37824041219](https://github.com/bboytang/CAIdex/actions/runs/37824041219)：原生Models不要求created，完整未知字段/大数、六方法/配置交集、经典文本JSON/SSE/Gateway与独立deepseek Bearer。3job/17steps及完整raw逐名通过，workspace391/386/390（0failed，ignored45/43/43），旧Runtime43/42/42（0failed/ignored）、Linux native credentials1。job Linux113472119918/Windows113472119538/macOS113472119894，raw1962/1649/1660行；DeepSeek10/Ollama69/OpenAI11/Custom7/Google90每名一次。watch77797、下载98157及normalize/check/available均exit0，校验在精确源码归档执行。
- DeepSeek新增上下文 `6742b15`：with_runtime_context只消费本地3头/client_metadata/cache，前置developer→system保留优先级；with_verbosity_instruction用执行端配置追加原instructions，未映射/重复映射拒绝。请求/响应未绑定turn-state拒绝，SSE实际断连/slot复用；原始及编译后预算、取消/deadline保持。新增6项/共16项、完整workspace397/0/45、Clippy全workspace/all-targets-D warnings、fmt/diff本地通过。有效RED禁用本地编译后编译成功、运行400失败；finally恢复正确源码后完整GREEN。无新依赖/共享生产源码/Runtime/workflow改动；此范围精确6742b15三平台已验（见当前任务及下方证据）。

## 下一步顺序

1. DeepSeek工具/namespace、明文reasoning与绑定历史：先检查现有Ollama/Gemini/Anthropic codec和原生契约，再增加有效回归并最小实现。原生忽略/降级字段明确编译或拒绝，原始声明与native编译结果一起绑定。函数名仅ASCII字母/数字/_/-、最多128字符且唯一，不能直接复用Ollama点号namespace；effort别名/采样钳制见docs/DeepSeek新段。不得丢弃历史或用Adapter fixture代替实际Runtime。
2. 随后Lite custom Code Mode/本地单调用交付、固定实际Runtime审批/执行/取消/磁盘重启，再Qwen/OpenRouter。每一步定向/相关回归、精确源码CI及交接；无源码变化不重跑已验本地全套/旧CI，不派重复独立审查。未经另行授权不调用商业API或下载模型。
3. 按V3继续H Host/持久化、I Chat/同步 → Windows → SSH/iOS → CLI → Relay → R最终验收；iOS在GitHub建立真正simulator测试/无签名archive，Rust macOS CI不代表iOS。

## 重要架构决定

- Windows11x64：Tauri2+React/TypeScript/Rust；Linuxx86_64 Host/CLI；iOS17+/Swift5 SwiftUI+UniFFI。GUI中文/英文，CLI英文；assets/brand原件，UI尽量1:1参考官方，模型/API Key入口在设置。
- 固定Codex0.160.1/d27764b82f7118f674371e6d6e76271d9d606edb是唯一Runtime/工具执行器；Gateway仅适配Responses，共享Custom transport/Broker，不另造Agent/HTTP栈。未知协议信息保留，实验能力显式opt-in，经典/Lite分别验收。
- Native wire为回放权威；载体绑定执行端/profile/端点/model/compiled前缀，工具另绑定原始声明及native声明/策略，SSE raw chunks重建核对。Gemini v1/v2、Anthropic v3/v4、Ollama v1/v2不自动升级/松绑；JSON载体不是加密或来源认证。Unknown/Configured/ProviderCatalog/ProtocolFixture不授予LiveRuntime/Full。
- DeepSeek原生无服务端会话；developer降级user、unknown input/内置tools忽略、parallel flag忽略、summary/encrypted_content/verbosity不等价。当前default仍拒绝高级字段/Lite/tools/reasoning输入，opt-in上下文不允许伪造reasoning回放；后置developer拒绝。完整原生契约与后续恢复点见docs/DeepSeek。
- Chat独立无Shell/Git/项目写权限；Remote使用Host Key，手机不读取Host已存Key，同步不含凭据。模型轮次边界切换，跨Provider关联分支/新线程。
- Host后台；SQLite journal先落盘再广播、快照补缺口、请求幂等/审批首次有效。不盲重跑未知结果、不承诺外部exactly-once；活动线程不迁移Host。Remote先SSH后Noise/Snow Relay。Chat自托管HTTPS+SQLite/outbox/cursor/分支/tombstone，服务器是信任边界，无历史E2EE承诺；unsigned archive不是可安装IPA。

## 问题 / 阻塞

- 当前无审批/实现阻塞。完整项目/文档公开、离线合成fixture及临时marker实际执行已授权，不重复询问；不读取用户模型Key/调用商业API/运行官方安装脚本。
- 商业Key/真实模型/签名真实性/Full、生产Host权限/UI/iOS/真机/Windows UAC/签名未验。同步SecretStore开始后不可强停，仅保证取消后不POST；comment-only native chunk与实际Runtime下游idle单独未验。
- 既有Minor保留在Provider文档：Gemini thought-call/无tools/未opt-in none覆盖、prefix-only整组互换、projection/满槽取消、ProtoJSON整数/空ID、部分usage/thought-only phase；Anthropic重启第三轮/完整Lite custom结果覆盖。不派重复独立审查，不把暂缓项改标为修复。
- jsonschema固定0.58.6，仅arbitrary-precision与Offline retriever，无HTTP/file解析；同步求值虽有字节/Regex限制但无硬CPU抢占，留H Host隔离；不自动重试/修补坏回答。依赖历史细节见docs/Ollama与Cargo.lock。

## Git / 环境 / 相关文件

- branch main；最新源码6742b151b225bb695c673331ead808ecbdbc61e9已提交/push，后续提交只同步HANDOFF、README及docs/DeepSeek/Model-Gateway验收与原生约束。无未完成源码；本轮文档随收尾提交，恢复时以git status确认精确HEAD/工作区。基础新增path crate仅使用既有固定依赖，上下文阶段无依赖变化。
- .git普通沙箱只读，提交/push需授权环境。gh bboytang；push：`git -c credential.helper= -c 'credential.helper=!gh auth git-credential' push origin main`，不改全局配置/输出凭据。完整测试loopback用授权环境+TMPDIR=/var/tmp，保留/tmp/.git。全局codex0.160.1可用，本机无CI专用.tools/codex；Windows/Xcode/native Linux服务缺项由CI验。磁盘约10G可用，修改前先df，不清源码/凭据/保护目录。
- 当前代码：model/providers/deepseek/src/{lib,config,catalog,request}.rs、tests/provider.rs；共享model/providers/custom、model/core、model/gateway、credentials/core；实际Runtime在runtime/bridge/tests/real_runtime.rs及fixtures。基准docs/CAIdex-实施计划-V3.md、CAIdex-UI-规范-V1.md；原V2仅需求背景。
- 本地日志：/tmp/caidex-deepseek-context-{red,green,workspace,clippy}.log；正确request备份/tmp/caidex-deepseek-context-request-green.rs。有效RED1834、定向55892、完整3561均exit0；测试计数不是编译失败或初次空日志检查。
- CI证据：/tmp/caidex-ci-37824041219-{status.json,watch.log,linux-raw.log,windows-raw.log,macos-raw.log}及标注日志；旧基础校验归档/tmp/caidex-deepseek-basic-ci-source。新增CI状态/tmp/caidex-ci-37825726756-status.json；校验脚本/tmp/caidex-deepseek-context-ci-{normalize,check,available}.py，在精确6742b15归档/tmp/caidex-deepseek-context-ci-source执行，不能用后续工作区新增测试代验旧head。跨机器以已提交docs和原CI为准。

- 最新CI收尾：6742b15/37825726756的3job/17steps均completed且成功或条件跳过，Linux113477961713/Windows113477962030/macOS113477961883完整raw1968/1655/1666行。workspace397/392/396（0failed，ignored45/43/43）、旧Runtime43/42/42（0failed/ignored）、Linux native credentials1；DeepSeek16/Ollama69/OpenAI11/Custom7/Google90及全部Runtime每平台每名一次。watch53780、最终下载13789及normalize/check/available均exit0，在精确6742b15归档核对；所有工具handle已结束，无待运行测试/CI。原始/标注日志/tmp/caidex-ci-37825726756-{linux,windows,macos}-raw.log及同名前缀.log/status.json/watch.log。实际DeepSeek Runtime/Live仍未验。
