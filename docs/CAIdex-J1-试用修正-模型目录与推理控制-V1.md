# J-1 用户试用修正：发送、供应商目录与输入框控制

2026-10-12 用户真实安装后报告 DeepSeek 官方 `https://api.deepseek.com` / `deepseek-flash` 无回答，并授权供应商只配一次Key、自动获取模型列表、输入框选择模型/推理强度或速度。本范围是已交付J-1的明确新增修正，不视作J-1独立审计通过或下一阶段授权。基线850e12c；保留现有生产/邮件/账户流程与Codex0.160.1真源、既有ModelProvider/Registry/Router/Broker、两类Custom。

有限任务：复现空tools与none导致默认DeepSeek编译拒绝，修复桌面配置；安全错误分类可见且失败不自动重发；供应商Key记录不复制/回显，已有配置向后读取、目录缓存可刷新、从输入框选择模型及其受支持参数；生成新版可安装包与精确CI/源码/下载/限制证据。非目标：原生账户/设备/Host/Codex执行/云Memory/同步/生产/真实商业Key请求。

验证：真实loopback HTTP的DeepSeek配置、目录与多个模型复用Key、推理参数wire、HTTP认证/限流/地址/协议错误和旧配置恢复；前端自动目录/选择/控件与错误；真实Windows WebView/安装启动卸载、安装字节hash；共享Chat原契约回归。真实用户商业请求由用户新包试用验证，不用fixture宣称Live通过。

通过：不再因空工具配置本地拒绝DeepSeek请求；保存供应商无需逐个模型ID/Key；目录选择及参数进入实际请求，未知或不支持参数不伪造开关；Key不进SQLite/日志/导出；错误明确、不重POST；实际安装包/CI可复现。达到后停止新增功能，封存等待独立审计。不跨范围重排V3阶段。

当前确认的源码问题：Chat固定空tools与tool_choice=none，而DeepSeek默认native_tools=false时拒绝任意这些字段；Chat receive丢弃ProviderError、GUI仅有失败终态。最小复用现有显式工具编译（不启工具执行）和安全ProviderError结构；默认send接口保持原行为，新接口提供每次请求控制和诊断。

参数来源核对2026-10-12：[DeepSeek Responses/工具与流事件](https://api-docs.deepseek.com/guides/responses_api/)、[DeepSeek思考强度](https://api-docs.deepseek.com/guides/thinking_mode/)；[OpenAI GPT-5](https://developers.openai.com/api/docs/models/gpt-5)、[GPT-5.4](https://developers.openai.com/api/docs/models/gpt-5.4)、[GPT-6.1 Sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol)、[GPT-6 Astra](https://developers.openai.com/api/docs/models/gpt-6-astra)、[Fast mode的priority兼容参数](https://developers.openai.com/api/docs/guides/fast-mode)。使用明确协议/具体ID声明，不用名称前缀猜能力；未知模型仍可选默认参数，不伪造等级。DeepSeek为none/low/high/max，OpenAI指定ID按各自声明及目录返回的已知effort值；本轮没有新增ultrafast/推理模式等更广参数。其他供应商目录接入，未有安全映射的推理参数保持默认，不宣称可任意调整所有模型。较低强度通常更快而非固定速度，priority可能增加费用和降级。

升级保留供应商UUID、Key归属和既有历史；新目录是独立本地缓存表。旧配置可读取，新控制值写入后旧0.1.0不识别新增字段，降级读取不承诺，历史库/Key本身不删除。目录上限4096/供应商，大列表搜索展示；目录声明不等于商业推理能力验收。商业模型不由本会话验证，最终真实用户复试另列结果。

## 新版试用

1. 退出旧客户端后安装0.1.1，沿用既有应用目录、供应商UUID与Windows凭据，不删除旧数据或重新录入Key。
2. 设置→模型：已有DeepSeek保持官方地址`https://api.deepseek.com`，点击刷新模型；新供应商填类型、名称、API地址和Key，保存后自动获取目录。正常流程无需手填模型ID；高级手填只用于不提供目录的兼容服务。
3. 聊天输入框的模型菜单按供应商分组，可搜索完整缓存目录并直接选模型。选择实际目录返回的`deepseek-flash`，不把测试中的其他合成ID当成官方供应商承诺。
4. 在模型旁选择推理强度：默认省略参数，由API决定；关闭思考/低/高/最高使用明确映射。支持优先处理的指定OpenAI模型另外显示速度选项，可能增加费用，服务商也可能降级。
5. 新建一轮发送短消息。遇到失败，保存界面的完整错误分类、所选模型和无密钥API地址供定位。旧失败消息保留，不自动重发；本次没有读取用户Key或调用官方商业模型。

目录包括供应商实际返回条目，不保证每个条目都支持文字Chat或当前账户可调用；选择不兼容模型会报告错误。其他供应商没有已确认映射的推理/速度参数保持API默认。Windows11人工安装、SmartScreen、MSI安装及旧安装包升级尚未专项验收。

## 实际交付与验证

新版0.1.1构建/测试源码为[`41db64dc66c31d2c29fb01f797a90ceb333a8a18`](https://github.com/bboytang/CAIdex/commit/41db64dc66c31d2c29fb01f797a90ceb333a8a18)，不是后续文档封存SHA。[Windows精确CI38256864545](https://github.com/bboytang/CAIdex/actions/runs/38256864545)两个job均success：前端7项（模拟IPC）、Windows后端14项（实际SystemStore/共享Chat）、原生WebView24项（6次回环合成POST）、格式/Clippy/正式构建及NSIS安装启动卸载通过。真实安装后的EXE与release SHA256一致；MSI仅构建和hash核对。

[同源码Foundation38256864514](https://github.com/bboytang/CAIdex/actions/runs/38256864514)四job全部success：Linux/macOS/Windows共享Runtime与数据库/认证回归；四份原日志checkout均核对41db64d，原日志仅本机私有保存，不向公开仓库导出邮件/账户原始记录。[任务与hash索引](evidence/j1-windows-catalog-20261012/foundation-index.json)提供只读复核入口，无生产部署或真实邮件请求。

下载：[CAIdex Windows x64 0.1.1 安装包](https://github.com/bboytang/CAIdex/actions/runs/38256864545/artifacts/11751500543)。压缩包包含NSIS与MSI，建议使用`CAIdex_0.1.1_x64-setup.exe`；通常需要GitHub登录，保存至2026-11-11T13:46:06Z。两份已实际下载到本机并逐份匹配CI字节/hash：

| 文件 | 字节 | SHA256 |
| --- | ---: | --- |
| CAIdex_0.1.1_x64-setup.exe | 6634181 | `9e90c28843a805f88b49657c1ccc600884c8d0093a94c91b09e38800dfe3fd70` |
| CAIdex_0.1.1_x64_en-US.msi | 9539584 | `dbca0159a81db112f818795cf14a5af342f373bc4bcd8629e517742d34eeaba3` |

本机TypeScript/Vite正式构建、Rust cargo check、12项Linux后端、两workspace格式/Python语法/diff通过；Linux无Windows凭据测试，原生Windows与Chromium验证使用GitHub。盘余不足完整跨平台构建，本轮复用有效缓存，没有未经授权清理。证据索引与合成原日志见[独立审计入口](evidence/j1-windows-catalog-20261012/README.md)，33项源码指纹可核对不可变源码。已复验早期旧Key输出错误断言和原生脚本等待禁用控件的问题；失败/取消的旧运行不计通过。

完成项为发送本地拒绝修正、可见安全错误、供应商一次Key与目录缓存/刷新、输入框模型和受支持推理/速度选项、新版可安装包；未接入账户/设备/云Memory/同步/Host/Codex执行。共享Chat原默认send契约保持，生产/cloud/auth/邮件/凭据核心、根Cargo.lock和Foundation工作流未变。真实商业DeepSeek回复、所有目录模型可调用性、其他供应商参数能力未授通过。

交付后停止新增功能，进入独立只读审计；实施会话自查不等于独立审计，不授整体项目通过或下一里程碑授权。
