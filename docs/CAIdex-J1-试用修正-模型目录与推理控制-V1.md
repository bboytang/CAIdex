# J-1 用户试用修正：发送、供应商目录与输入框控制

2026-10-12 用户真实安装后报告 DeepSeek 官方 `https://api.deepseek.com` / `deepseek-flash` 无回答，并授权供应商只配一次Key、自动获取模型列表、输入框选择模型/推理强度或速度。本范围是已交付J-1的明确新增修正，不视作J-1独立审计通过或下一阶段授权。基线850e12c；保留现有生产/邮件/账户流程与Codex0.160.1真源、既有ModelProvider/Registry/Router/Broker、两类Custom。

有限任务：复现空tools与none导致默认DeepSeek编译拒绝，修复桌面配置；安全错误分类可见且失败不自动重发；供应商Key记录不复制/回显，已有配置向后读取、目录缓存可刷新、从输入框选择模型及其受支持参数；生成新版可安装包与精确CI/源码/下载/限制证据。非目标：原生账户/设备/Host/Codex执行/云Memory/同步/生产/真实商业Key请求。

验证：真实loopback HTTP的DeepSeek配置、目录与多个模型复用Key、推理参数wire、HTTP认证/限流/地址/协议错误和旧配置恢复；前端自动目录/选择/控件与错误；真实Windows WebView/安装启动卸载、安装字节hash；共享Chat原契约回归。真实用户商业请求由用户新包试用验证，不用fixture宣称Live通过。

通过：不再因空工具配置本地拒绝DeepSeek请求；保存供应商无需逐个模型ID/Key；目录选择及参数进入实际请求，未知或不支持参数不伪造开关；Key不进SQLite/日志/导出；错误明确、不重POST；实际安装包/CI可复现。达到后停止新增功能，封存等待独立审计。不跨范围重排V3阶段。

当前确认的源码问题：Chat固定空tools与tool_choice=none，而DeepSeek默认native_tools=false时拒绝任意这些字段；Chat receive丢弃ProviderError、GUI仅有失败终态。最小复用现有显式工具编译（不启工具执行）和安全ProviderError结构；默认send接口保持原行为，新接口提供每次请求控制和诊断。

参数来源核对2026-10-12：[DeepSeek Responses/工具与流事件](https://api-docs.deepseek.com/guides/responses_api/)、[DeepSeek思考强度](https://api-docs.deepseek.com/guides/thinking_mode/)；[OpenAI GPT-5](https://developers.openai.com/api/docs/models/gpt-5)、[GPT-5.4](https://developers.openai.com/api/docs/models/gpt-5.4)、[GPT-6.1 Sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol)、[GPT-6 Astra](https://developers.openai.com/api/docs/models/gpt-6-astra)、[Fast mode的priority兼容参数](https://developers.openai.com/api/docs/guides/fast-mode)。使用明确协议/具体ID声明，不用名称前缀猜能力；未知模型仍可选默认参数，不伪造等级。DeepSeek为none/low/high/max，OpenAI指定ID按各自声明及目录返回的已知effort值；本轮没有新增ultrafast/推理模式等更广参数。其他供应商目录接入，未有安全映射的推理参数保持默认，不宣称可任意调整所有模型。较低强度通常更快而非固定速度，priority可能增加费用和降级。

升级保留供应商UUID、Key归属和既有历史；新目录是独立本地缓存表。旧配置可读取，新控制值写入后旧0.1.0不识别新增字段，降级读取不承诺，历史库/Key本身不删除。目录上限4096/供应商，大列表搜索展示；目录声明不等于商业推理能力验收。商业模型不由本会话验证，最终真实用户复试另列结果。
