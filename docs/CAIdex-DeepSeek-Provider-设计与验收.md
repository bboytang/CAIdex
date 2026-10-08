# CAIdex DeepSeek Provider：协议基线与验收

2026-10-08核对官方文档；经典基础Adapter和10项离线回归已三平台通过；新增上下文适配及6项回归亦已精确三平台通过。随后加入显式Classic函数/namespace与完整native历史，新增9项回归/共25项及完整workspace本地通过；该函数/历史范围已精确三平台通过；后续effort29项范围亦已精确三平台通过，实际DeepSeek Runtime/真实模型未验。继续V3的共享Custom transport/Broker与固定Runtime，不运行官方安装脚本或改写用户Codex配置。

## 已核对的原生契约

- [Models API](https://api-docs.deepseek.com/api/list-models/)的`object=list`、model `id/object/owned_by`为声明字段；没有要求`created`。扩展包含name、上下文/输出上限、媒体、effort与分协议能力。现有Custom/OpenAI目录解析器要求created，不能直接使用或为此放松其校验；DeepSeek目录需按自身契约验证、保留未知字段和原始数字，目录不授予Live/Full。
- [Responses API](https://api-docs.deepseek.com/api/create-response/)与[兼容表](https://api-docs.deepseek.com/guides/responses_api/)说明API无服务端会话，多轮由客户端提交完整input。developer按user解释；未知input及内置工具可能被忽略。仅支持指定apply_patch custom，其他custom名字不可直接透传。须明确编译或拒绝，不能通过HTTP成功认领Runtime工具语义等价。
- reasoning使用明文content；summary/encrypted_content不能原样回放。effort有原生等级及兼容别名，别名不证明精确刻度等价。parallel_tool_calls被忽略，原生始终允许并行；verbosity也无效果。现有Lite单调用、原始历史及本地verbosity契约不能靠直接透传成立。
- 官方[Codex接入说明](https://api-docs.deepseek.com/quick_start/agent_integrations/codex/)另提供catalog/config；此说明不证明与本项目固定0.160.1所有经典/Lite能力兼容。实际wire、工具、审批、取消和磁盘恢复须独立验证。

## 已验收基础实现（三平台通过）

`DeepSeekConfig::new`使用独立deepseek归属Bearer引用，`with_base_url`明确指定可信代理/loopback，复用Custom端点/TLS安全策略。默认官方根为`https://api.deepseek.com/`，代理路径保留后拼接models/responses；没有隐式环境Key、OpenAI组织scope或自动模型路由。`DeepSeekProvider::new/with_options`只接Classic metadata，六方法沿用共享Registry/transport/Broker；目录与已配置native模型取交集，来源仅ProviderCatalog，不升级Unknown能力或兼容性报告。

目录独立验证object/id/owner及重复ID，不读取或补造created；未消费的原生声明/未知字段/大数保持。原Custom/OpenAI严格目录解析器未改。基础请求允许字符串或user/assistant/system文本message、instructions、正整数或null max_output_tokens、纯text format及对应无意义null选项，原文保留；store统一false，background只接受false/null。原始body先检查预算，alias改为native ID后仍受共享request预算约束，Unsupported text/streaming在Key/POST前拒绝。

本阶段明确拒绝Lite路由、所有工具声明、reasoning控制/历史输入、developer消息、图片/附件、结构输出、sampling、归属头及未知控制字段；不把原生忽略的parallel/verbosity等当支持。JSON/SSE原生输出保留明文thinking、usage、未知item/大数及真实completed/incomplete/failed状态，错误诊断使用共享脱敏。意外function/custom/search调用的JSON、SSE item/工具delta/terminal在交付前安全拒绝；stream只报一次静态error并立即释放底层I/O/slot，没有新worker、HTTP栈或工具执行器。

10项离线回归覆盖：无created目录/六方法/交集与原文、畸形/重复目录及无fallback、JSON/SSE原生未知信息/文本history/nullable、坏控制零Key/POST、配置与Lite门控、预算/Unsupported/凭据归属/预取消、三种终态与诊断脱敏、意外工具流实际断连/slot复用、原生HTTP错误/无重定向/目录预算/取消/Drop、真实Gateway本地token与原生Key隔离及拒绝归属头。合成凭据和实际loopback，不涉及商业API。

有效RED使用既有严格OpenAI目录解析器，测试编译成功并在缺created样本处502失败；恢复独立契约后GREEN10/0/0。日志 `/tmp/caidex-deepseek-{catalog-red,green,focused-final}.log`。完整本地workspace391/0/45、既有固定Codex0.160.1 Runtime43/0/0、Clippy全targets-D warnings/fmt/diff通过，日志同前缀 `{workspace,runtime-regression,clippy-final}.log`。首轮fixture接口/局部命名/错误码与Clippy分支风格问题已修正，不计有效RED或成功；最终定向和Clippy重新通过。新增deepseek path crate只依赖已固定第三方版本，lock仅加入该package，无升级/新功能，workflow及共享生产源码/Runtime未改。现有43项Runtime只是旧回归，没有DeepSeek实际Runtime正例；基础范围随后以精确2967f56/CI37824041219完成三平台，见收尾证据。

## 下一步

基础与上下文适配三平台已收尾；Classic函数/namespace、明文reasoning与绑定历史已有本地实现（见末节）。该范围已精确三平台收尾；随后effort亦已精确三平台通过，再按真实固定Runtime请求补齐custom apply_patch、剩余推理控制、Lite custom Code Mode/单调用策略及实际Runtime接线。对原生忽略但用户依赖的字段明确拒绝或以显式策略编译，源声明与编译历史继续绑定；实际Runtime/Live证据分别记录。Qwen/OpenRouter随后按各自原生契约核对，不从“OpenAI-compatible”标签推定相同能力。

## 验证状态

上述经典基础目录/JSON/SSE/Gateway范围三平台通过；新增上下文适配亦三平台通过。Classic函数/namespace与history新范围见末节，不借旧CI认领新范围。custom工具/其他高级参数/媒体/剩余推理控制/Lite/实际DeepSeek Runtime/Live/Full仍待。整体F/G与H–R未完成，测试fixture不证明真实模型能力。

基础源码`2967f56a7788ee90375f1e970e4500567a61cfd3`/[CI37824041219](https://github.com/bboytang/CAIdex/actions/runs/37824041219)整体completed/success，各3job/17steps成功或条件跳过。完整raw Linux1962/Windows1649/macOS1660行，workspace391/386/390（0failed，ignored45/43/43）、旧固定Runtime43/42/42（0failed/ignored），Linux native credentials1；DeepSeek10/Ollama69/OpenAI11/Custom7/Google90及全部Runtime每名每平台一次。watch77797、下载98157及normalize/check/available均exit0；原始/标注日志 `/tmp/caidex-ci-37824041219-{linux,windows,macos}-raw.log`及同前缀.log/status.json/watch.log，校验脚本 `/tmp/caidex-deepseek-basic-ci-{normalize,check,available}.py`在精确2967f56归档执行。这不验证后续上下文适配或DeepSeek实际Runtime/Live。

## 已验收上下文适配（三平台通过）

沿用现有Provider显式构造策略：`with_runtime_context`仅允许3个本地归属头session_id/x-client-request-id/x-codex-turn-metadata并清空后发起原生GET/POST，取消/deadline保持；client_metadata仅字符串值，prompt_cache_key非空无控制字符，两者只在本地消费，不承诺原生缓存/归属持久化。只将对话前developer消息编译为system；对话开始后developer拒绝，不接受原生降级user。

`with_verbosity_instruction`由执行端为low/medium/high配置非空指令，重复映射拒绝；请求的verbosity必须已映射，移除原生无效果字段并追加到原instructions，原文/中文文本/已有system不改。null选项仅无意义值允许。原始body与扩展后body均检查预算。请求和响应的未绑定x-codex-turn-state分别400/502拒绝，SSE关闭真实socket并释放slot。默认构造仍拒绝这些Runtime参数；tools/reasoning输入/summary/context/include/Lite仍拒绝，不能据此认领完整固定Runtime。

新增6项离线回归：JSON/SSE优先指令与verbosity/nullable精确native wire，本地控制畸形/后置developer/未实现history拒绝且零Key/POST，执行端映射校验，原始及扩展预算，目录归属/取消/deadline及native turn-state断连/slot复用，实际Gateway原instructions/本地token隔离。共16项定向通过，日志 `/tmp/caidex-deepseek-context-green.log`；有效RED临时禁用本地编译，测试编译成功并运行400失败，正确源码finally恢复（同前缀red.log）。正确源码finally恢复后完整workspace397/0/45（DeepSeek16每名一次）、Clippy全workspace/all-targets-D warnings/fmt/diff通过，日志同前缀{workspace,clippy}.log；精确6742b15随后通过三平台，见下方完整日志证据。无新依赖/共享生产源码/Runtime/workflow修改。

上下文源码`6742b151b225bb695c673331ead808ecbdbc61e9`/[CI37825726756](https://github.com/bboytang/CAIdex/actions/runs/37825726756)整体completed/success，3job/17steps均成功或条件跳过。Linux113477961713/Windows113477962030/macOS113477961883完整raw1968/1655/1666行；workspace397/392/396（0failed，ignored45/43/43）、旧Runtime43/42/42（0failed/ignored）、Linux native credentials1。DeepSeek16/Ollama69/OpenAI11/Custom7/Google90及全部Runtime每平台每名一次；watch53780、最终下载13789及normalize/check/available均exit0。原始/标注日志 `/tmp/caidex-ci-37825726756-{linux,windows,macos}-raw.log`及同前缀.log/status.json/watch.log；脚本 `/tmp/caidex-deepseek-context-ci-{normalize,check,available}.py`在精确6742b15归档执行，未借基础CI代验。实际DeepSeek Runtime/Live及完整F/G仍未验。

## 已核对的接入边界（实现范围见末节）

2026-10-08再次核对官方[Responses请求定义](https://api-docs.deepseek.com/api/create-response/)、[Thinking说明](https://api-docs.deepseek.com/guides/thinking_mode/)和Models声明；不运行安装脚本、不访问模型服务。实现继续复用现有transport/Broker/codec模式，不增加另一Provider依赖或执行器。

- function name必须非空、最多128字符、`[a-zA-Z0-9_-]+`且全局唯一；因此不能直接照搬Ollama的点号namespace名称。原始namespace/function/custom身份与编译后的安全native名称必须可逆对应，并一起绑定请求/历史；拒绝冲突、未知声明和不支持的内置tools，不能自动去重或静默丢弃工具。custom仅apply_patch原生支持；Lite exec不能原样透传，须明确映射及绑定原始输入/actual结果。
- effort原生none/low/high/max；none关闭thinking。公开兼容别名minimal→low、medium/xhigh→high不证明精确等级等价；Thinking通用页另列ultra→max，但Responses参考未列，先不推定支持。映射应由执行端明确选择并记录，目录只提供声明、不能自动授予Live/Full。
- temperature在thinking开启时无效果；top_p只在thinking开启时有效且下限0.95，低值会被原生钳制，关闭thinking时固定1.0。不能因成功响应把忽略/钳制当作按原值执行；后续必须按明确原生thinking选择门控，拒绝不等价组合而不悄改用户值。top_logprobs原生0–20，具体输出及strict结构交付还须独立验证。
- reasoning input仅明文content进入原生上下文；summary/encrypted_content不支持。沿用已确定的完整native wire载体/前缀绑定：包含执行端profile/端点/model、编译请求、原响应及SSE chunks；canonical显示/工具组须与native输出一致，未知字段/大数/中文/原始arguments保持。不得把本地敏感JSON载体称作真实加密/来源认证，不删除真实推理以假装Runtime回放成功。

以上是接入约束；函数/namespace、绑定历史与effort实现范围见后节，其余不标为已实现/已验证。随后Classic/Lite实际固定Runtime审批、执行、取消及磁盘恢复；每一步独立测试、精确CI和交接。

## Classic函数与绑定native历史（三平台通过：dba1c90）

构造策略保持显式：`with_native_history()`启用原生历史载体，`with_native_tools()`同时启用Classic函数/namespace和历史。未启用时保留原16项测试验证的严格拒绝行为，不提升metadata/Registry的兼容性等级，不改变其他Provider、共享transport或固定Runtime。

- 函数名遵守原生ASCII/128字符/全局唯一约束；namespace扁平编译为确定的`caidex_ns_<声明索引>`，保留namespace描述并可逆恢复源name/namespace，拒绝别名碰撞及重复源身份。源声明（含strict/defer的false/null）、源choice/parallel与native声明同时绑定；不静默丢弃重要控制。支持auto/none/required及具namespace的指定function；strict/defer true、custom/内置工具/未知字段拒绝。parallel true表示允许并行，去掉原生忽略的flag；false拒绝，尚未宣称本地单调用策略。
- 输入函数调用/结果校验call_id、原始JSON-object arguments、完整结果配对与文本结果。响应工具只允许已声明alias、有唯一id/call_id且未重复使用历史call_id的completed调用，遵守choice；保留中文、原始arguments及未知输出字段/大数。
- `caidex.deepseek.native-history.v1:`载体保留完整编译native request、native response、源映射、执行端CredentialRef归属（不含Secret）、端点/model；流式同时保留解码后的原生SSE JSON事件。绑定调用方实际编译的前缀，不采信载体自行声明的前缀；验证整组显示/工具语义一致，再恢复原生明文reasoning及完整输出。允许Runtime省略/改变有效显示id/status，不放松语义/arguments/未知字段。任意调用方明文reasoning、外来载体、改前缀/源声明/summary/工具/执行端scope均Key/POST前拒绝。
- 流式文本与单native reasoning item的摘要增量交付；工具等完整terminal、声明/参数/状态校验和原生事件重建一致后才交付。SSE delta的item_id、终态内容及added/done身份一致性校验；多个reasoning added或未对应已added reasoning索引的delta明确失败，复杂reasoning/content-part序列还须单独映射与验证。取消/Drop/deadline/坏流关闭原I/O并释放slot；仍无新worker、工具执行器或模型HTTP框架。
- 原始/展开请求、原生历史、投影事件和pending队列均有预算；满预算报明确错误，不删除推理/工具历史。载体是敏感JSON，不是加密、来源认证或Host journal。完整前缀有增长上限（源码shortcut已记录），未来H持久化可去重；不能把序列化回放测试称为实际Host落盘/重启验收。

新增9项离线测试（总25）：JSON两轮namespace/原始大数arguments回放；坏声明/choice/未绑定reasoning零Key/POST；完整组/实际前缀/原始声明/端点/profile/model篡改拒绝；响应名称/参数/id/状态/choice及slot复用；原生SSE文本/摘要增量与chunks核对及回放；坏工具delta/未知工具/多reasoning流零工具交付、真实断连和slot复用；历史预算与实际取消/Drop；不同namespace同名函数/指定choice经真实Gateway与本地token隔离；序列化后三轮准确native前缀与重复call_id拒绝。均为合成Key/loopback，不访问商业API。

本轮本地定向25/0/0（`/tmp/caidex-deepseek-tools-green.log`）；有效RED临时破坏namespace别名，测试编译成功并运行失败，finally逐字节恢复tools源码（同前缀red.log及tools-green.rs）。最终源码完整workspace406/0/45（含25项DeepSeek每名一次），既有固定Codex0.160.1 Runtime43/0/0，Clippy全workspace/all-targets-D warnings、fmt及diff检查通过；日志同前缀{workspace,runtime-regression,clippy}.log。多reasoning added拒绝纳入既有坏流测试；初次沙箱禁止loopback的PermissionDenied不是代码RED，使用授权离线执行环境后通过。Runtime43仅旧回归，不是新DeepSeek实际Runtime正例。当前源码仅DeepSeek的config/lib/request、tools/history/history_stream和tests/provider；无新依赖、Cargo.lock、共享生产源码、其他Provider、Runtime或workflow变更。此前账户/记忆架构文档原样保留，不实现其功能；2026-10-08用户已授权本地检查通过后直接commit/push并三平台CI。旧6742b15的CI只验原16项，本范围已提交/push dba1c90123b31e89f50c871e225afc817765ca7e，[CI37851276859](https://github.com/bboytang/CAIdex/actions/runs/37851276859)completed/success，已三平台完整收尾。

尚未实现/验证：custom apply_patch与Lite exec、deferred/search、parallel false本地策略、reasoning summary/context/include控制（effort进度见后节）、媒体/结构输出/采样、固定DeepSeek Classic/Lite实际Runtime审批/执行/取消/磁盘恢复、真实模型Live/Full。保持V3顺序，继续本Provider后再Qwen/OpenRouter，不能用Adapter/Gateway fixture代替实际Runtime证据。

## 显式reasoning effort映射（三平台通过：32a9f3f）

`with_reasoning_effort_mapping(source, native)`由执行端配置请求级映射，native仅none/low/high/max；source允许none/minimal/low/medium/high/xhigh/max，不从模型名称/目录猜测映射、不透传服务端别名、不保证不同等级推理强度等价。重复/未知映射构造时拒绝；未配置的source、非对象/无effort/null/非字符串reasoning、summary/context等未实现字段Key/POST前拒绝。默认构造仍拒绝reasoning请求控制；已有native历史功能不被自动启用。

先检查原始请求预算，再映射一次；展开历史后的第二遍只验证native等级，不对已编译值再次应用source映射。启用thinking时遵守Unsupported reasoning门控，native none可作为显式关闭选择。当前轮effort允许独立改变；旧载体保存当时实际native request/response，回放恢复完整旧推理，不用新映射重写过去请求。取消/deadline、原生预算、alias/native路由和凭据边界沿用共享路径。

新增4项：构造级别/重复策略拒绝；4个native等级的JSON/SSE与两轮完整历史、原instructions/verbosity/优先developer保持及防二次映射；未映射/畸形/Unsupported/默认控制零Key/POST；映射缩短字段不能绕过原始预算。有效RED在原编译器上编译成功、运行400失败（`/tmp/caidex-deepseek-effort-red.log`）；实现后定向29/0/0、最终workspace410/0/45（29项DeepSeek逐名一次）、Clippy全workspace/all-targets-D warnings、fmt/diff通过，日志同前缀{green,workspace,clippy}.log。未改依赖/其他Provider/共享生产源码/Runtime/workflow；不重复运行与此独立Provider改动无关的旧本地Runtime全套。

此4项不是dba1c90/CI37851276859的25项范围，已以独立新提交32a9f3fdae8aed7519a867c1d32a8cf911f5ce09/[CI37851939704](https://github.com/bboytang/CAIdex/actions/runs/37851939704)完成三平台完整验收。summary/context/include、custom apply_patch（包括固定Runtime grammar处理）、媒体/结构/采样和实际DeepSeek Classic/Lite执行/审批/取消/磁盘恢复仍待；以上不授予Live/Full。

函数/历史三平台收尾：精确dba1c90123b31e89f50c871e225afc817765ca7e/CI37851276859，3job各17steps完成且成功或条件跳过；Linux113564495100/Windows113564495117/macOS113564494877完整raw1977/1664/1675行。workspace406/401/405（0failed，ignored45/43/43）、旧固定Runtime43/42/42（0failed/ignored）、Linux native credentials1。DeepSeek25及既有全部workspace/Runtime/credentials/compile-fail doc-test通过名逐平台核对，完整名集合450/443/447，等于已验6742b15基线加9新名，无遗漏或重复。watch54073、下载/check79697、normalize/full-names均exit0；原始和step标注日志/tmp/caidex-ci-37851276859-{linux,windows,macos}-raw.log及同前缀.log/status.json/watch.log。checker /tmp/caidex-deepseek-tools-ci-{normalize,available}.py及/tmp/caidex-deepseek-ci-full-names.py在精确源码归档/tmp/caidex-deepseek-tools-ci-source执行，不借后续effort源码代验。本CI不证明后续29项范围或实际DeepSeek Runtime/Live。

effort三平台收尾：精确32a9f3f/CI37851939704整体completed/success，3job各17steps成功或条件跳过；Linux113566712391/Windows113566712436/macOS113566712113完整raw1981/1668/1679行。workspace410/405/409（0failed，ignored45/43/43）、旧固定Runtime43/42/42（0failed/ignored）、Linux native credentials1。DeepSeek29及全workspace/credentials/Runtime/compile-fail doc-test通过名逐平台核对，完整集合454/447/451与已验6742b15加13新名一致，无遗漏/重复。watch62556、最终下载/check85317、normalize/available/full-names均exit0；原始/标注日志/tmp/caidex-ci-37851939704-{linux,windows,macos}-raw.log及同前缀.log/status.json/watch.log。checker /tmp/caidex-deepseek-effort-ci-{normalize,available}.py及/tmp/caidex-deepseek-ci-full-names.py在精确源码归档/tmp/caidex-deepseek-effort-ci-source执行。两个源码范围已独立收尾，不重复运行旧CI，不把43/42/42旧Runtime当DeepSeek正例；实际DeepSeek Runtime/Live与完整F/G、H–R仍未完成。
