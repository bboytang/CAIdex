# CAIdex DeepSeek Provider：协议基线与验收

2026-10-08核对官方文档；当前仅完成接入前协议核对，尚无DeepSeek Adapter、协议测试或真实模型验收。继续V3的共享Custom transport/Broker与固定Runtime，不运行官方安装脚本或改写用户Codex配置。

## 已核对的原生契约

- [Models API](https://api-docs.deepseek.com/api/list-models/)的`object=list`、model `id/object/owned_by`为声明字段；没有要求`created`。扩展包含name、上下文/输出上限、媒体、effort与分协议能力。现有Custom/OpenAI目录解析器要求created，不能直接使用或为此放松其校验；DeepSeek目录需按自身契约验证、保留未知字段和原始数字，目录不授予Live/Full。
- [Responses API](https://api-docs.deepseek.com/api/create-response/)与[兼容表](https://api-docs.deepseek.com/guides/responses_api/)说明API无服务端会话，多轮由客户端提交完整input。developer按user解释；未知input及内置工具可能被忽略。仅支持指定apply_patch custom，其他custom名字不可直接透传。须明确编译或拒绝，不能通过HTTP成功认领Runtime工具语义等价。
- reasoning使用明文content；summary/encrypted_content不能原样回放。effort有原生等级及兼容别名，别名不证明精确刻度等价。parallel_tool_calls被忽略，原生始终允许并行；verbosity也无效果。现有Lite单调用、原始历史及本地verbosity契约不能靠直接透传成立。
- 官方[Codex接入说明](https://api-docs.deepseek.com/quick_start/agent_integrations/codex/)另提供catalog/config；此说明不证明与本项目固定0.160.1所有经典/Lite能力兼容。实际wire、工具、审批、取消和磁盘恢复须独立验证。

## 实施恢复点

先接原生目录及六方法的经典基础JSON/SSE文本范围，复用已有TLS/认证/有界传输/取消/安全错误，不新增HTTP栈。以模型配置和真实目录交集列出可用路由；Unsupported仍在Key/POST前拒绝，Unknown不升级Supported。目录缺created为有效样本，重复/坏ID与owner、未知字段/大数需回归；原OpenAI目录测试须保持通过。

随后按真实固定Runtime请求逐项接工具/namespace、developer/context/verbosity、明文reasoning和历史、Lite custom Code Mode/单调用策略。对原生忽略但用户依赖的字段明确拒绝或以显式策略编译，源声明与编译历史继续绑定；真实Runtime/Live证据分别记录。Qwen/OpenRouter随后按各自原生契约核对，不从“OpenAI-compatible”标签推定相同能力。

## 验证状态

已只读核对官方Models/Responses/Codex文档，并用公开兼容表核对现有目录解析器。未新增依赖、源码、测试或商业调用；基础JSON/SSE、Gateway、实际Runtime、三平台和Live均待实现/验证。
