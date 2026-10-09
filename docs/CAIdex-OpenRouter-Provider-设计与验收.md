# CAIdex OpenRouter Provider：设计与验收

阶段F/G；2026-10-09。源码`model/providers/openrouter`，复用ModelProvider、CustomResponses传输、执行端Credential Broker和Gateway。当前基础Classic文本Adapter已实现，14项定向离线测试通过；完整本地已验、精确源码三平台CI待提交后核验。工具/推理回放/Lite/实际固定Runtime接线与商业Live/Full尚未验收，不修改Codex唯一执行/审批真源或V3阶段顺序。

## 官方契约与本步范围

已核对[Responses概览](https://openrouter.ai/docs/api_reference/responses/overview)、[基础文本与多轮](https://openrouter.ai/docs/api_reference/responses/basic-usage)、[请求参考](https://openrouter.ai/docs/api/api-reference/responses/create-responses)、[目录](https://openrouter.ai/docs/api/api-reference/models/list-all-models-and-their-properties)及[提供商路由](https://openrouter.ai/docs/guides/routing/provider-selection)。API及模型后端会变化，后续接线仍须重新核对，不以OpenAI兼容说明授Full。

- `OpenRouterConfig::new`使用官方`https://openrouter.ai/api/v1/`，仅接受执行端`provider=openrouter, kind=api-key`引用；显式`with_base_url`复用HTTPS/字面loopback安全策略，拒绝userinfo/query/fragment。固定派生models/responses端点，不读取环境Key，不接受模型提供URL，不新增代理或TLS关闭选项。
- 原生目录是`data`数组，不要求OpenAI的object=list/model、owned_by或created。仅验证对象/有效唯一ID并排序，完整原始条目（价格/architecture/supported_parameters/null/未知扩展/大数）保留和序列化；这些字段未转换为已验能力。list_models返回执行端配置与目录交集，source=ProviderCatalog，配置Unknown及兼容等级保持，不提供失败后的缓存或名称猜测。
- 六方法沿用共享接口。入口仅Classic，基础接受文本或system/developer/user/assistant文本消息数组、字符串instructions、有效max_output_tokens/temperature/top_p；assistant typed message可保留有效ID/completed状态和空annotations/logprobs。未知参数、工具/媒体/结构输出/推理输入、Runtime attribution/cache元数据、Lite及调用方路由覆盖在Key/POST前拒绝。
- OpenRouter Responses没有服务端会话状态。每次携带完整已支持文本历史，强制store=false；true/畸形store、background=true或非空previous_response_id拒绝。完整原生输出、usage和opaque reasoning可保留，但当前不接受reasoning输入或声明其绑定/回放兼容性。HTTP200的failed/incomplete不表示成功生成。
- 编译固定provider.require_parameters=true和allow_fallbacks=false，拒绝caller provider/models/plugins等绕过。共享传输无自动重试；这些政策不固定初始后端、不证明不同轮次选择同一供应商，也不保证其数据留存政策。绑定推理历史前须另验明确后端作用域，不能用model slug冒充稳定原生会话。
- 基础context headers全部拒绝，Runtime内部身份不转发native；原生turn-state拒绝。不提供Decline/执行工具之类新引擎。原生未请求function/custom/MCP/tool-search调用及参数事件拒绝，终态store=true拒绝；坏流不会作为成功交付。正文未知非工具扩展保留。
- 源预算先于本地编译，编译后请求及响应/frame预算、TLS、禁止重定向、单次认证、并发slot、deadline/取消/Drop与实际socket关闭均复用Custom；错误是静态安全分类，不附原生错误正文、Key、请求或URL。同步SecretStore开始后仍不能强停，取消后不POST。

## 离线验收

测试`model/providers/openrouter/tests/provider.rs`复用既有原生Adapter的隔离HTTP fixture模式，只用合成Key，无用户配置/商业API/模型下载。14项覆盖：

| 范围 | 验证 |
| --- | --- |
| 配置、目录、六方法 | Key引用/端点/Classic门控，完整目录JSON及配置交集，无能力/Full升级，坏ID/重复/空目录 |
| 文本请求与SSE | 两轮完整文本前缀，developer/instructions保留，native model替换，router政策/store=false，JSON/SSE未知字段/大数/opaque输出及三种终态 |
| 请求信任边界 | 状态/路由覆盖/未知控制/工具/媒体/推理/坏标量/消息门控，Key与POST为0，源与编译预算 |
| 原生输出 | 未请求调用、存储与turn-state拒绝，不交付成功或可执行调用 |
| 传输与凭据 | 缺Key/跨owner/预取消/到期，错误正文脱敏、重定向/content-type/大小拒绝，header/body timeout与cancel实际断连，Drop后slot复用 |
| Gateway | 无/坏监听token拒绝，内部context Key前拒绝，监听token与模型Bearer分离，不发送cookie/API-Key/组织等调用方header |

初次可执行定向13通过/1失败来自custom/tool-search测试夹具缺少共享Core要求的身份字段；补齐合法输入后最终14/0/0。早期编译错误仅新文件函数放置/测试SecretKind与reqwest既有功能调用已修正；不新增依赖功能或修改共享实现。日志`/tmp/caidex-openrouter-basic/{check,focused-first,focused-final}.log`。完整本地workspace524/0/59、既有固定Runtime57/0/0、全workspace/all-targets Clippy -D warnings、fmt/diff及逐名边界检查通过，workspace旧510+14、既有Runtime通过名完全保持。无新外部package/version，仅Cargo新增内部crate；共享Core/Custom/Gateway/Runtime/Broker及其他Provider源码/workflow未改。精确源码三平台CI待本次提交后核验，不能借Qwen旧CI代验；既有Runtime回归不是实际OpenRouter接线。

## 后续顺序

1. 完成本步完整本地检查、差异审查、提交推送及精确源码三平台CI，更新HANDOFF。
2. 继续OpenRouter显式Runtime上下文/正文与推理控制、原生工具及后端绑定历史；官方推理/工具/路由契约先核对，不复制Qwen summary或DeepSeek明文规则。
3. 再验Lite与实际固定Classic/Lite Runtime审批/执行/取消/磁盘恢复；商业模型兼容性另需授权。
4. 保持V3 F/G→H/I→Windows/SSH/iOS/CLI/Relay/R；生产Host、GUI、账户/记忆及完整CLI均仍待实现。
