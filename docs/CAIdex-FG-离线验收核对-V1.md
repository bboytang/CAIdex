# CAIdex F/G 离线验收核对 V1

核对日期：2026-10-10；恢复基线 main / origin/main=`a9502451bbe392a5fd00bd39e073caef17c20960`，工作区干净；最新源码 `3a57bcbfb8f4aa942019c081404b6833daa57642`。依据[实施计划 V3](CAIdex-实施计划-V3.md)、[Gateway 验收](CAIdex-Model-Gateway-设计与验收.md)、各 Provider 验收、实际源码/测试及精确 CI。此文是剩余门槛索引，不是商业模型兼容性报告，也不宣布 F/G 完成。

## 基线证据

重新读取 [CI38072098372](https://github.com/bboytang/CAIdex/actions/runs/38072098372) 的精确 head、三个 job/全部17steps与完整日志；整体 completed/success，各步骤成功或条件跳过。Linux/Windows/macOS workspace 分别619/614/618通过（0失败、73/71/71忽略），固定 Runtime 71/70/70通过（0失败/忽略）。忽略项由独立 Runtime/native credential 步骤补验，不能算作 workspace 通过。下面八个 Adapter 的529个测试函数名在三平台日志分别匹配通过；这是既有回归证据，不是新增测试或真实模型验证。

| Adapter | 基线离线测试数 | 已实现/已验证范围入口 | 仍不能认领 |
| --- | --- | --- | --- |
| Custom | 7 | [独立传输测试](../model/providers/custom/tests/provider.rs)，Responses/TLS/六方法 | 未知兼容端点的真实能力 |
| OpenAI | 11 | [验收](CAIdex-OpenAI-Provider-设计与验收.md)，Models/Responses透传、固定Classic/Lite两轮及interrupt | 商业版本、Runtime未知扩展完整往返 |
| Anthropic | 123 | [验收](CAIdex-Anthropic-Provider-设计与验收.md)，原生签名/v1–v4、动态MCP、Lite真实审批执行 | 默认cached web、签名真实性、完整grammar |
| Gemini | 90 | [验收](CAIdex-Gemini-Provider-设计与验收.md)，JSON/SSE/v1–v2、工具/媒体/推理、Classic/Lite真实Runtime | discovery/web/deferred/后置system等明确拒绝项、商业Full |
| Ollama | 69 | [验收](CAIdex-Ollama-Provider-设计与验收.md)，history/工具/结构输出、Classic MCP与Lite执行/恢复 | 实际daemon版本、模型下载/推理及性能 |
| DeepSeek | 57 | [验收](CAIdex-DeepSeek-Provider-设计与验收.md)，逐route effort/history/custom/Lite、真实Runtime7项 | 实际模型、通用custom/媒体等未开放语义 |
| Qwen | 72 | [验收](CAIdex-Qwen-Provider-设计与验收.md)，namespace/custom映射/history/Lite、真实Runtime7项 | 真实地域/计划/API、截断/grammar硬约束 |
| OpenRouter | 100 | [验收](CAIdex-OpenRouter-Provider-设计与验收.md)，显式backend、原生namespace/custom/history/Lite、真实Runtime7项 | 实际后端/版本证明、商业Full |

上述范围须以对应文档最后的已验子阶段为准；较早段落描述当时状态，不能拿历史“尚未实现”重复开发。Fixture只验证协议/编译/传输与固定执行器，不验证模型智能、商业限额、真实签名、生产Host。

## V3 门槛与剩余工作

| 门槛 | 当前事实与证据 | 剩余分类 |
| --- | --- | --- |
| F Responses/取消/工具/opaque/错误 | Core、Gateway及各Adapter已有离线回归；Runtime执行与审批保持固定0.160.1/d27764b | 已验证具体范围；不是所有Provider×参数×时序的穷举证明 |
| G Registry/ModelRouter | [Registry](../model/core/src/registry.rs)、[Router测试7项](../model/core/tests/router.rs)、[Gateway Router测试2项](../model/gateway/tests/router/mod.rs)：显式ID、目录归属、漂移拒绝、未知模型/方言/streaming门控，无fallback | 已实现/离线已验；不新建Registry或HTTP栈 |
| Unknown/Unsupported展示与拒绝 | ModelMetadata/六方法公开三态及未知限额；目录不升级能力/报告。Router检查dialect/streaming，其余请求语义由Adapter门控；Provider测试含Key/POST前拒绝 | 核心数据与拒绝已验；最终GUI/CLI展示归J/P，不伪造当前产品UI |
| 每模型版本化报告 | CompatibilityReport schemaVersion=1、reference/testedModelVersion/source/level门控已有；[切换报告V1](CAIdex-模型切换-离线兼容性报告-V1.md)仅四条合成route | 各Provider文档是分阶段证据，尚非每个商业版本报告；其他fixture报告仍须逐route/配置索引，不批量赋Full或自动挂生产Registry |
| 模型切换 | Classic3、Lite2与跨Provider2项真实Runtime测试已验；同Provider只限两个明确同方言route组合；跨Provider只显式可见文本新线程及foreign reasoning拒绝 | 不重复既有测试；完整工具状态/其他组合未验，活动轮次约束/持久关联/通用历史适配归H，不能用云专用history入口替代 |
| Provider工具/推理边界 | 配置、签名载体和完整前缀绑定已有；Gemini单调用3分支已补专项；Anthropic恢复/完整结果本地精确断言已补，详下表 | 补已有契约的离线证据；未开放功能继续明确拒绝，不按slug猜支持 |
| F真实模型验收与G商业报告 | 当前没有授权的真实Key/API/daemon证据 | 用户确认全项目完成后自行验收；当前继续可离线任务，真实模型仍未验，不宣称商业Compatible/Full |

## 已确认缺口与顺序

| 项目 | 实际源码/测试核对结果 | 动作 |
| --- | --- | --- |
| Gemini单调用thought豁免/无tools/未opt-in none | [provider测试](../model/providers/google/tests/http/provider.rs)已有opt-in auto/none/多调用/MAX_TOKENS；缺上述三个专项。[请求编译器](../model/providers/google/src/request.rs)已有0/1/无限分支，[数量校验](../model/providers/google/src/provider.rs)排除thought调用 | 本次新增两项回归覆盖三个分支，JSON/SSE×Classic/Lite本地已验；实现未改动，精确新CI三平台已验 |
| Gemini重复/部分SSE usage | [history测试](../model/providers/google/tests/history.rs)已有专项更新与raw保留 | 旧缺口已覆盖，不重复补 |
| Gemini缺prompt的usage下界、thought-only call文本phase | normalized_usage提前返回null；outcome看到任意functionCall即ToolCall，投影phase沿outcome | 仍有代码边界，后续分别独立复现/定向修复；本次不夹带修改 |
| Gemini整组载体互换/满槽取消 | 现有组内编辑/错model/request拒绝；投影取消测试先消费到text，native满槽已有deadline测试 | 精确整组互换及满槽cancel/Drop仍未专项覆盖，后续补真实入口测试；不是断言已存在漏洞 |
| Gemini整数/空ID | catalog只接受canonical整数，content拒绝显式空ID | 已记录兼容表示限制，无实际端点新依据，不泛化codec |
| Anthropic重启第三轮/完整Lite结果 | [Runtime测试](../runtime/bridge/tests/real_runtime.rs)重启只比旧请求前缀；Lite结果只contains marker，没有完整canonical/disk→native结果逐值核对 | 本轮复用两个既有用例补齐：完整第三native回复、Lite落盘调用及完整结果逐值对照；定向各1/0/0，workspace621/0/73、固定Runtime71/0/0、Clippy/fmt/diff通过，待精确CI |
| Runtime远端opaque compaction/Lite | [能力对照](CAIdex-Runtime-能力对照.md)仍只有Classic手动摘要证据 | 尚未验，不用doctor/Classic摘要代验；选定后按固定wire独立复现 |

本表针对交接点名门槛和源码分支，不声称全仓穷举或全时序形式验证。后续按确认缺口逐项推进；商业/Host边界保持V3最终方案。

## 本次执行计划与验收

沿既定V3细化验收，不重新设计：

1. 在既有 `model/providers/google/tests/http/provider.rs` 增加thought调用不占可执行单调用额度的正例；JSON/SSE×Classic/Lite，完整native history往返，只有一个真实function_call交付。
2. 增加无tools/未opt-in none的零调用额度负例；JSON/SSE×Classic/Lite，负例502且不交付call/载体/成功终态，认证与原生POST各一次。
3. 定向→Google完整→workspace与固定Runtime完整→Clippy/fmt→最终diff；不修改生产实现/依赖/fixture/workflow，除非新测试实际暴露既有缺陷。
4. 本地通过后按已有授权commit/push main；核对精确源码三平台CI全部job/步骤/完整测试名，更新本报告、Provider文档及HANDOFF。本地已验：定向3/0/0（含旧单调用测试）、Google92/0/0、workspace621/0/73、固定Runtime71/0/0、Clippy workspace/all-targets-D warnings、fmt/diff、固定schema。workspace函数通过名精确旧CI+2，Runtime通过名保持，无遗漏/重复。精确源码`d9b0d1a49d2aa78822d4110d07a5dc91a22e0800`/[CI38079737190](https://github.com/bboytang/CAIdex/actions/runs/38079737190)三平台已验。首次沙箱拒绝loopback监听，允许本地socket的验证环境复跑通过；不是生产缺陷RED。

固定Runtime本会话首次缺少项目二进制，71项NotFound；按CI安装隔离0.160.1、版本/schema校验后重跑完整通过，未使用全局0.162.1。唯一独立只读审查无Critical/Important，文档中多余的“无调用正例”承诺已删除。225个其他tracked文件保持恢复基线；旧Provider/Runtime用例正文未改。本次只有本核对文档、Gemini验收、Google测试与HANDOFF变更。

## 本次完成证据与准确下一步

源码`d9b0d1a49d2aa78822d4110d07a5dc91a22e0800`/[CI38079737190](https://github.com/bboytang/CAIdex/actions/runs/38079737190)整体completed/success；三job各17steps成功或条件跳过，完整日志逐名核验通过。Linux/Windows/macOS workspace621/616/620（0失败、忽略73/71/71）、固定Runtime71/70/70（0失败/忽略）；Google92逐名每平台一次。全通过名693/686/690精确旧基线CI+2，无旧名遗漏/重复；raw日志2298/1984/1995行，watch与下载exit0。仅Google新增2测试，其余Adapter及固定Runtime保持。上述执行计划已经完成；纯文档收尾不重复Rust CI。

下一步：Anthropic精确断言本地已补、待精确CI；继续逐项处理表中Gemini usage下界/thought-only phase/整组互换/满槽cancel、各route版本化报告索引等缺口。商业真实模型与生产Host仍未验，F/G不标整体完成，不跳H。
