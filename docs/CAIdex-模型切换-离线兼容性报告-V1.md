# CAIdex 模型切换离线兼容性报告 V1

报告修订：1；CompatibilityReport schemaVersion：1。执行器固定为 Codex 0.160.1 / d27764b82f7118f674371e6d6e76271d9d606edb。

本报告仅覆盖下表的合成 OpenAI Responses 路由组合。公共模型 slug 用来选择固定 Runtime 的协议配置；请求实际发送到 loopback fixture 的 native-switch-0/1，没有调用这些商业模型。证据等级为 ProtocolFixture / Experimental，不能标为 LiveProvider、LiveRuntime、Compatible 或 Full。

| 公共路由 ID | 原生 fixture 模型 | 方言 | testedModelVersion | 本次验收范围 |
| --- | --- | --- | --- | --- |
| gpt-5.5 | native-switch-0 | Classic | openai-router-classic-fixture-v1 | 与下行轮次边界切换、磁盘恢复、fork 父子隔离、未知模型拒绝/显式恢复 |
| gpt-5.1-codex | native-switch-1 | Classic | openai-router-classic-fixture-v1 | 同上，仅该明确组合 |
| gpt-6.1-sol | native-switch-0 | Lite | openai-router-lite-fixture-v1 | 与下行切换、磁盘恢复、fork 隔离、Lite header/工具前缀、未知模型拒绝/显式恢复 |
| gpt-6-sol | native-switch-1 | Lite | openai-router-lite-fixture-v1 | 同上，仅该明确组合 |

Classic 精确源码430305d/[CI38069700504](https://github.com/bboytang/CAIdex/actions/runs/38069700504)三平台已验。Lite 新增2项完整本地通过：workspace619/0/73、固定Runtime71/0/0（精确旧69+2）、Clippy/fmt/diff。精确源码`3a57bcbfb8f4aa942019c081404b6833daa57642`/[CI38072098372](https://github.com/bboytang/CAIdex/actions/runs/38072098372)三平台已验：Linux/Windows/macOS workspace619/614/618、固定Runtime71/70/70全部通过，各17steps成功或条件跳过；完整通过名精确旧CI38071015251+2，无遗漏/重复。实际证据更新见本报告及[交接](../HANDOFF.md)。

报告引用使用既有 CompatibilityReport 契约；每行的 testedModelVersion 取上表相应值，其余字段如下：

```json
{
  "schemaVersion": 1,
  "level": "experimental",
  "source": "protocolFixture",
  "reference": "docs/CAIdex-模型切换-离线兼容性报告-V1.md",
  "testedModelVersion": "openai-router-lite-fixture-v1",
  "limitations": [
    "Synthetic loopback inference only; no commercial model validation.",
    "Only the explicitly tested same-provider and same-dialect route pair.",
    "No production Host switching policy or persistent thread association."
  ]
}
```

这是版本化证据索引，尚未自动挂载到生产 Registry。路由注册仍保留原配置的 Unknown 能力和空报告，测试结果不自动赋予能力、模型限额或商业兼容性。Core 对 fixture 冒称 Full 的守门与 Router 对报告版本漂移的拒绝已另有[测试](../model/core/tests/router.rs)。报告引用由可信执行端提供，Registry 校验结构及来源范围，不认证报告真实性。

实际 Runtime 用例位于[Classic 切换测试](../runtime/bridge/tests/switching/mod.rs)和[Lite 切换测试](../runtime/bridge/tests/switching/lite.rs)。只在上一轮完成后发起切换；未证明活动轮次切换安全，也未实现生产 Host 的边界约束。无推理 fork 与父子历史隔离不等于跨 Provider 原始历史可迁移。

跨 Provider 另见[独立边界用例](../runtime/bridge/tests/switching/cross_provider.rs)：仅显式选中的可见文本进入独立新线程，来源 ID 为可见标注；源磁盘不透明 reasoning 在目标 Gateway 被拒绝。生产持久关联、一般历史适配仍待实现。实验 thread/resume.history 是云专用禁止使用入口，本项目不以它替代这些缺口。

商业版本变化、其他 Provider/模型组合、Classic↔Lite 互切、完整工具状态跨模型切换、媒体/限额/性能、多端 Host 竞争与审批边界均未由本报告验证。后续模型报告必须提供对应版本、方言、配置、真实测试和限制，不能按名称或目录结果推断兼容。
