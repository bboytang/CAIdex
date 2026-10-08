# CAIdex DeepSeek Provider：协议基线与验收

2026-10-08核对官方文档；经典基础Adapter和10项离线回归已三平台通过；新增上下文适配及6项回归完整本地通过，精确三平台待验，实际DeepSeek Runtime/真实模型未验。继续V3的共享Custom transport/Broker与固定Runtime，不运行官方安装脚本或改写用户Codex配置。

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

基础三平台已收尾；先完成新增上下文适配检查/提交/精确三平台，再按真实固定Runtime请求逐项接工具/namespace、明文reasoning和绑定历史、Lite custom Code Mode/单调用策略。对原生忽略但用户依赖的字段明确拒绝或以显式策略编译，源声明与编译历史继续绑定；实际Runtime/Live证据分别记录。Qwen/OpenRouter随后按各自原生契约核对，不从“OpenAI-compatible”标签推定相同能力。

## 验证状态

上述经典基础目录/JSON/SSE/Gateway范围三平台通过；新增上下文适配完整本地通过、三平台待验。工具/其他高级参数/媒体/推理控制与history/Lite/实际DeepSeek Runtime/Live/Full仍待。整体F/G与H–R未完成，测试fixture不证明真实模型能力。

基础源码`2967f56a7788ee90375f1e970e4500567a61cfd3`/[CI37824041219](https://github.com/bboytang/CAIdex/actions/runs/37824041219)整体completed/success，各3job/17steps成功或条件跳过。完整raw Linux1962/Windows1649/macOS1660行，workspace391/386/390（0failed，ignored45/43/43）、旧固定Runtime43/42/42（0failed/ignored），Linux native credentials1；DeepSeek10/Ollama69/OpenAI11/Custom7/Google90及全部Runtime每名每平台一次。watch77797、下载98157及normalize/check/available均exit0；原始/标注日志 `/tmp/caidex-ci-37824041219-{linux,windows,macos}-raw.log`及同前缀.log/status.json/watch.log，校验脚本 `/tmp/caidex-deepseek-basic-ci-{normalize,check,available}.py`在精确2967f56归档执行。这不验证后续上下文适配或DeepSeek实际Runtime/Live。

## 当前上下文适配（完整本地通过，三平台待验）

沿用现有Provider显式构造策略：`with_runtime_context`仅允许3个本地归属头session_id/x-client-request-id/x-codex-turn-metadata并清空后发起原生GET/POST，取消/deadline保持；client_metadata仅字符串值，prompt_cache_key非空无控制字符，两者只在本地消费，不承诺原生缓存/归属持久化。只将对话前developer消息编译为system；对话开始后developer拒绝，不接受原生降级user。

`with_verbosity_instruction`由执行端为low/medium/high配置非空指令，重复映射拒绝；请求的verbosity必须已映射，移除原生无效果字段并追加到原instructions，原文/中文文本/已有system不改。null选项仅无意义值允许。原始body与扩展后body均检查预算。请求和响应的未绑定x-codex-turn-state分别400/502拒绝，SSE关闭真实socket并释放slot。默认构造仍拒绝这些Runtime参数；tools/reasoning输入/summary/context/include/Lite仍拒绝，不能据此认领完整固定Runtime。

新增6项离线回归：JSON/SSE优先指令与verbosity/nullable精确native wire，本地控制畸形/后置developer/未实现history拒绝且零Key/POST，执行端映射校验，原始及扩展预算，目录归属/取消/deadline及native turn-state断连/slot复用，实际Gateway原instructions/本地token隔离。共16项定向通过，日志 `/tmp/caidex-deepseek-context-green.log`；有效RED临时禁用本地编译，测试编译成功并运行400失败，正确源码finally恢复（同前缀red.log）。正确源码finally恢复后完整workspace397/0/45（DeepSeek16每名一次）、Clippy全workspace/all-targets-D warnings/fmt/diff通过，日志同前缀{workspace,clippy}.log；新精确提交三平台待验。无新依赖/共享生产源码/Runtime/workflow修改。
