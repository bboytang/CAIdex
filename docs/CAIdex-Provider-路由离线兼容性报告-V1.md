# CAIdex Provider 路由离线兼容性报告 V1

报告修订1；CompatibilityReport schemaVersion=1；Runtime固定Codex0.160.1/d27764b82f7118f674371e6d6e76271d9d606edb。此索引覆盖八Adapter实际固定Runtime正例的16个公共路由/方言，profile配置以[Harness](../runtime/bridge/tests/real_runtime.rs)对应模式和Provider验收为准。它没有新增路由，不自动挂生产Registry。公共slug只选择固定Runtime协议，native模型和回复均为loopback合成fixture；`testedModelVersion`表示本报告的fixture/profile修订，**不是商业模型的服务版本**。单元/HTTP中的临时alias仅作为各Provider协议分支证据，不独立授予模型兼容性。

| Provider | 公共路由ID | 原生fixture模型 | 方言 | testedModelVersion | 精确profile入口与限制 |
| --- | --- | --- | --- | --- | --- |
| custom | `gpt-5.5` | `gpt-5.5` | Classic | `custom-runtime-classic-fixture-v1` | `gateway-classic`；[限制/验收](CAIdex-Model-Gateway-设计与验收.md) |
| custom | `gpt-6.1-sol` | `gpt-6.1-sol` | Lite | `custom-runtime-lite-fixture-v1` | `gateway-lite`；[限制/验收](CAIdex-Model-Gateway-设计与验收.md) |
| openai | `gpt-5.5` | `gpt-5.5` | Classic | `openai-runtime-classic-fixture-v1` | `gateway-openai-classic`；[限制/验收](CAIdex-OpenAI-Provider-设计与验收.md) |
| openai | `gpt-6.1-sol` | `gpt-6.1-sol` | Lite | `openai-runtime-lite-fixture-v1` | `gateway-openai-lite`；[限制/验收](CAIdex-OpenAI-Provider-设计与验收.md) |
| anthropic | `gpt-5.5` | `native-fixture` | Classic | `anthropic-runtime-classic-fixture-v1` | `gateway-anthropic-discovery-classic`；[限制/验收](CAIdex-Anthropic-Provider-设计与验收.md) |
| anthropic | `gpt-6.1-sol` | `native-fixture` | Lite | `anthropic-runtime-lite-fixture-v1` | `gateway-anthropic-tools-lite`；[限制/验收](CAIdex-Anthropic-Provider-设计与验收.md) |
| google | `caidex-google-classic-fixture` | `models/native-fixture` | Classic | `google-runtime-classic-fixture-v1` | `gateway-google-mcp-classic`；[限制/验收](CAIdex-Gemini-Provider-设计与验收.md) |
| google | `caidex-google-lite-fixture` | `models/native-fixture` | Lite | `google-runtime-lite-fixture-v1` | `gateway-google-tools-lite`；[限制/验收](CAIdex-Gemini-Provider-设计与验收.md) |
| ollama | `gpt-5.5` | `native-fixture` | Classic | `ollama-runtime-classic-fixture-v1` | `gateway-ollama-discovery-classic`；[限制/验收](CAIdex-Ollama-Provider-设计与验收.md) |
| ollama | `gpt-6.1-sol` | `native-fixture` | Lite | `ollama-runtime-lite-fixture-v1` | `gateway-ollama-tools-lite`；[限制/验收](CAIdex-Ollama-Provider-设计与验收.md) |
| deepseek | `caidex-deepseek-classic-fixture` | `native-fixture` | Classic | `deepseek-runtime-classic-fixture-v1` | `gateway-deepseek-tools-classic`；[限制/验收](CAIdex-DeepSeek-Provider-设计与验收.md) |
| deepseek | `caidex-deepseek-lite-fixture` | `native-fixture` | Lite | `deepseek-runtime-lite-fixture-v1` | `gateway-deepseek-tools-lite`；[限制/验收](CAIdex-DeepSeek-Provider-设计与验收.md) |
| qwen | `caidex-qwen-classic-fixture` | `native-fixture` | Classic | `qwen-runtime-classic-fixture-v1` | `gateway-qwen-tools-classic`；[限制/验收](CAIdex-Qwen-Provider-设计与验收.md) |
| qwen | `caidex-qwen-lite-fixture` | `native-fixture` | Lite | `qwen-runtime-lite-fixture-v1` | `gateway-qwen-tools-lite`；[限制/验收](CAIdex-Qwen-Provider-设计与验收.md) |
| openrouter | `caidex-openrouter-classic-fixture` | `native-fixture` | Classic | `openrouter-runtime-classic-fixture-v1` | `gateway-openrouter-tools-classic`；[限制/验收](CAIdex-OpenRouter-Provider-设计与验收.md) |
| openrouter | `caidex-openrouter-lite-fixture` | `native-fixture` | Lite | `openrouter-runtime-lite-fixture-v1` | `gateway-openrouter-tools-lite`；[限制/验收](CAIdex-OpenRouter-Provider-设计与验收.md) |

每行对应一个独立CompatibilityReport记录：reference为本文件路径，testedModelVersion逐行采用表中值，level均为experimental，source均为protocolFixture；limitations由以下共有限制和该Provider段落共同构成。记录不合并Classic/Lite方言或不同profile能力。既有四条OpenAI切换路由报告仍见[模型切换V1](CAIdex-模型切换-离线兼容性报告-V1.md)，不重复生成或升级。

```json
{
  "schemaVersion": 1,
  "level": "experimental",
  "source": "protocolFixture",
  "reference": "docs/CAIdex-Provider-路由离线兼容性报告-V1.md",
  "testedModelVersion": "google-runtime-lite-fixture-v1",
  "limitations": [
    "Synthetic loopback inference through Codex 0.160.1 only; no live model validation.",
    "Only the explicitly documented route, dialect and executor profile; capabilities and limits remain unverified.",
    "No production Host or generic cross-provider history migration.",
    "Google Lite requires explicit single-call opt-in, disabled web search and the fixed fixture catalog; thought-only native calls cannot be automatically replayed."
  ]
}
```

下列是每行必需的配置与限制，不是默认全功能声明：

- Custom：直接Responses透传、原生opaque reasoning两轮与中断；不证明商业端点或未知字段在Runtime类型化后全量保持。工具执行器仍为固定Runtime。
- OpenAI：Responses透传、明确org/project与合成CredentialRef；Classic/Lite各两轮/中断。远端v2压缩使用两个额外`gateway-openai-compact-*`显式loopback模式，独立证据见[Runtime能力对照](CAIdex-Runtime-能力对照.md)，不能推给其他Provider。
- Anthropic：expected_organization=org-fixture、thinking binding/error控制、本地Runtime context、AllTurns、auto→summarized、adaptive effort与verbosity提示映射。Classic正例须关闭web并启用inline工具发现；Lite单次Code Mode真实审批/临时执行。默认cached web拒绝，无签名真实性或完整grammar证明。
- Google：固定Google catalog、models/native-fixture、maxOutputTokens=4096、工具上限100、Runtime context、AllTurns、auto摘要及显式thinkingBudget/verbosity映射；Classic静态MCP，Lite显式单调用opt-in，web关闭。其他默认/缺opt-in、discovery/deferred/后置system拒绝；thought-only调用不自动回放；canonical整数/显式空ID限制保持。history模式的两轮/重启证据覆盖相同公共route，但不增加默认能力。
- Ollama：本地Runtime context、native history、显式ModelDetails/effort与verbosity指导；Classic启用deferred MCP发现，Lite使用显式Code Mode/single-call选项。实际daemon版本、下载、模型推理/性能未验；默认/多调用按既有门控拒绝。
- DeepSeek：Runtime context、低/中/高effort映射、verbosity指导；Classic tools模式显式native apply_patch，另有native tools分支证据；Lite显式options/native tools。默认无能力或多调用明确拒绝；通用custom/媒体/真实版本未开放或未验。
- Qwen：Runtime context、custom tool mapping（包含native tools/history）、逐route effort及verbosity映射；summary/context只限既有验收契约；Lite另需显式options。地域/计划/API版本与grammar硬约束未验；不能按slug推断支持。
- OpenRouter：逐route显式backend=fixture-backend/region、runtime/native/advanced tools、native history、auto summary/all_turns context、effort/verbosity映射；Lite显式options。默认或部分政策拒绝Key/POST前发送；实际后端身份/版本、商业Full未验。

基础证据：源码`9db1fe7edeca3ce1e262f6cbdbafdc6b2261609f`/[CI38081159725](https://github.com/bboytang/CAIdex/actions/runs/38081159725)八Adapter及固定Runtime三平台完整日志已核验，workspace621/616/620、Runtime71/70/70，0失败，旧通过名保持。Gemini边界增强源码`57451e9996aee201954ee5d404dd02e22dba8dd2`的精确CI尚在运行；compaction新证据尚在本地回归，本报告不会把它们记成三平台通过。

可信执行端可引用本索引，但生产Registry配置的Unknown能力、未知context/output限额和空报告保持，不用fixture反推模型智能、签名认证、服务限额或费用。LiveProvider/LiveRuntime/Compatible/Full仍未验；用户确认在全项目完成后自行进行真实模型验收。生产Host、活动轮次模型边界约束与持久跨Provider关联归H，客户端展示归J/P。
