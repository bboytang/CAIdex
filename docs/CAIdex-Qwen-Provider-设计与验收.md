# CAIdex Qwen Provider：设计与验收

阶段 F/G；2026-10-09。代码 `model/providers/qwen`。本步为原生目录及基础 Responses 接入，沿用既定 ModelProvider/Custom transport/Credential Broker/Gateway；不另造 HTTP 框架、Agent 或工具执行器。完整 Qwen Codex 兼容性、商业 Live/Full 和生产 Host **尚未验收**，不能因目录或文本请求成功升级兼容等级。

## 官方契约与执行端配置

只读核对：[原生模型目录](https://www.alibabacloud.com/help/en/model-studio/list-models)、[Responses](https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-openai-responses)、[地域与计划端点](https://www.alibabacloud.com/help/en/model-studio/base-url)。实际服务会更新，后续工具/推理接入需重新核对；固定 Codex 仍为 0.160.1 / d27764b，不以供应商接入教程授予 Full。

- `QwenConfig::new(base, CredentialRef)` 要求执行端明确选择地域/工作区/计费计划对应的基址和 `provider=qwen, kind=api-key`。生产基址是官方域名根，例如 `https://<workspace>.cn-beijing.maas.aliyuncs.com/`；不能传 SDK 的 `/compatible-mode/v1` 前缀。显式可信代理可带统一路径前缀。
- 从同一基址派生 `api/v1/models` 与 `compatible-mode/v1/responses`，不自动换地区、工作区、旧路径或 Provider，不探测环境 Key。不自动选择计费计划；工作区/地域 Key 和计划可用性由执行端配置及将来的显式连接测试核对。
- 沿用 HTTPS/TLS 验证、无重定向/自动 POST 重试、HTTP 仅 literal loopback、URL 禁 userinfo/query/fragment；当前 Key 只由执行端 Broker 解析。Debug 不显示端点/配置/原生 wire。
- 目录 GET 与推理共用限流、取消、绝对期限、响应字节限制及安全错误。共享 Custom 新增 `get_json_with_query`，只为执行端非秘密目录参数编码，不放宽 Endpoint 校验，不接收模型输入选址；旧 `get_json/post_json` 保持原行为。

## 原生目录与六方法

目录是 `success/output.total/page_no/page_size/models`，模型主标识为 `model`，不是 OpenAI `object/data/id/created/owned_by`。官方样例缺部分可选声明并允许 null，未消费的属性、价格、时间、大数和未知扩展原样保留在 NativeModel.wire，序列化仍是原生模型对象。

分页固定每页20项，按总数取完整目录；最多256页/5120项及聚合响应字节预算，全过程共享一个总期限。页号/页大小/数量不符、重复ID、分页过程中总数变化、坏success/模型ID或超限均拒绝，不返回部分清单或缓存成功，不无限重新扫描。该上限是客户端资源界限，不是官方总模型容量承诺。

ModelProvider 六方法全部接入：

- `discover_models(context)` 返回完整原生目录；`list_models()`只返回目录与已配置路由的交集，source为ProviderCatalog。
- `metadata/capabilities/credential_requirements` 是执行端配置的只读查询，不读秘密或发请求。未知模型拒绝，未知能力保留Unknown；没有兼容报告时不编造。
- `create_response/stream_response` 共用已配置模型别名→native_model路由、Broker认证、Custom JSON/SSE 和取消/背压。仅Classic路由；声明不支持文本或streaming时在Key前拒绝。

## 基础请求与输出边界

本步支持文本字符串或user/assistant/system/developer消息、instructions、stream、max_output_tokens及合法temperature/top_p。结构化文本按角色校验，developer保持原角色和顺序，不沿用DeepSeek的降级规则；完整assistant输出消息要求id/status/completed/content数组，半成品历史拒绝。

服务会忽略未列控制且默认store=true，因此本步采用允许字段校验并显式写store=false。背景false只表示本地前台语义，在发送前消费；store=true/background=true、服务端conversation/previous_response_id、tools/choice/parallel、reasoning控制、text/verbosity/结构输出、include、client/cache元数据、媒体及未知字段均在Key前拒绝，不能静默变成普通聊天。源请求与补充store后的请求各自受字节预算约束；原生max_output_tokens最低16。nullable普通可选参数保持原值，store null仍编译false。

原生message/reasoning summary、usage和未知扩展按完整JSON/SSE保留。未声明function/custom/MCP/内置工具调用及相关事件拒绝，流关闭真实socket/释放slot，不交付可执行工具；不把reasoning summary当OpenAI加密载体。原生turn-state头拒绝，未绑定reasoning输入不能用于继续线程。本步不建立签名、历史来源认证或原生历史回放。

共享核心仍区分completed/incomplete/failed；非流式HTTP200返回failed/incomplete也不表示生成成功，调用方须检查真实终态。SSE缺完成、坏协议/[DONE]不作为成功；错误诊断脱敏，原生正文不做破坏性替换。Gateway token与模型Bearer独立，不能转发给供应商。

**已知原生限制：Responses会自动截断超出其输入预算的上下文，约为模型context window的80%。** 当前基础字节预算不是token预算，也不能证明服务未截断。后续真实Runtime/长上下文接线须按模型/版本核对输入预算、token计算与压缩/拒绝策略；未验证之前不能承诺长历史无损或授Full。目录的context/max_input字段不直接成为Codex实测证据。

## 本步验证与证据

夹具全部是隔离loopback与合成Qwen Key，无真实API/用户秘密/付费模型。测试覆盖：

| 范围 | 断言 |
| --- | --- |
| 目录与六方法 | 两页完整scan、精确GET路径/query/Bearer、null/未知大数/原生序列化、配置交集/Unknown/无Full |
| 异常目录 | OpenAI格式、坏success/分页/ID、重复与总数变化、空目录、页数/聚合预算拒绝 |
| JSON/SSE | developer优先级、消息历史、原生summary/正文/usage/扩展、精确native模型/store=false、无Lite头 |
| Key前边界 | ignored/unimplemented字段、媒体/未绑定reasoning、坏文本/半成品消息、能力不支持、错误provider/owner/Key缺失、源与编译后超限、取消/过期 |
| 传输生命周期 | 分页不续总期限、query编码及URL保护、取消/Drop真实断连、占槽与释放、HTTP/Retry-After/无重定向、未知工具拒绝 |
| Gateway | 独立监听token、执行端Key只去native、流终态及无秘密回显 |

首次46479未编译成功（夹具误用现有Gateway API和reqwest helper），不算有效RED；第二轮38347为8通过/3夹具期望失败（空Registry、非法Lite字符串、坏SSE同chunk不保证先交付created），按既有契约修正。35171定向13/0/0通过；随后补充能力门控/完整assistant消息用例，最终workspace54851 exit0：452通过/0失败/52ignored，Qwen14每名一次、DeepSeek57及Custom7保持；全workspace/all-targets Clippy-D warnings86201 exit0，fmt/diff通过。实际Runtime源码未改，本机不重复执行旧实际Runtime全套；本步独立源码三平台CI已按下列精确提交验收，旧CI不代验。日志 `/tmp/caidex-qwen-basic-{first,green,boundaries,workspace,clippy}.log`；检查和CI恢复点见[HANDOFF](../HANDOFF.md)。尚未完成的检查不得按上轮结果冒称通过。

精确源码`3638d13863b339e230aa884820a8ff4b840526e6`/[CI37888877980](https://github.com/bboytang/CAIdex/actions/runs/37888877980)已completed/success。Linux113685036842、Windows113685037198、macOS113685037053各17steps成功或条件跳过，完整raw2059/1745/1756行；workspace452/447/451（0failed，ignored52/50/50）、既有固定Runtime50/49/49（0failed/ignored）、Qwen14每名每平台一次，DeepSeek57保持。全部通过名503/496/500等于旧精确37886226991集合加14新名，无遗漏/重复；watch84155、状态/完整日志下载及逐名checker均exit0。检查脚本`/tmp/caidex-qwen-basic-ci-check.py`，结果同前缀ci-result.json，日志`/tmp/caidex-ci-37888877980-{linux,windows,macos}-raw.log`及status.json/watch.log。固定Runtime回归不是实际Qwen工具/历史接线验收，商业Live/Full仍未验。

## 后续实施顺序

1. 本步14项、workspace/Clippy及3638d13精确三平台CI已完成；保留证据，不重复已验基础适配，不借此授Full。
2. 按实际Qwen wire接入显式Runtime attribution/正文控制和逐模型推理参数；随后原生function/namespace/custom、tool choice与成对结果、完整summary历史绑定/重放，验证终态后交付工具。不盲复制DeepSeek明文content或Gemini签名契约。
3. 单独核对Lite/Code Mode、并行与本地交付策略及native能力边界，再接固定真实Codex Classic/Lite审批/执行/取消/磁盘恢复测试；实验/网页/模型服务端工具不得冒充Runtime工具。
4. Qwen之后OpenRouter，再按V3推进H/I/Windows/SSH/iOS/CLI/Relay/R；生产Host审批竞争/持久化在H，GUI/账户仍按既定阶段。真实商业模型测试需明确授权，本步不读用户Key/下载模型或部署。
