# J-1 0.1.1 试用修正：独立审计入口

不可变构建与测试源码`41db64dc66c31d2c29fb01f797a90ceb333a8a18`，修正基线`850e12c85c056a43700f17058cdeb6d8fc04f81e`。后续文档封存提交不是安装包构建提交；独立审计尚未执行，实施自查不授通过。

- [有限范围、下载、试用与限制](../../CAIdex-J1-试用修正-模型目录与推理控制-V1.md)
- [33项源码与配置指纹](source-fingerprints.json)：含共享Chat、九供应商配置、IPC、安装配置、测试入口、未改动的根锁及凭据红线。
- `renderer/`来自真实Chromium但模拟IPC，截图内容仅为合成测试；`native/`来自实际Windows release WebView与原生IPC及回环合成HTTP，不能外推商业接口通过。
- [本机后端原始12项结果](local-backend.log)：复现旧DeepSeek本地400，并验证新路径目录、两个模型/同Key、实际reasoning参数、HTTP错误、密钥移除期间旧目录拒写、priority参数、旧配置读取；不使用用户真实Key。
- [验证索引](verification-index.json)、[Windows成功状态](windows-run.json)、[7前端原日志](frontend.log.gz)、[14后端与原生构建原日志](windows.log.gz)、[原生24项结果](native/native.json)。两份GUI原日志checkout均核对同一源码SHA，解压hash见索引。
- [安装清单](native/build-manifest.json)、[实际下载复核](download-verification.json)、[产物摘要与有效期](artifacts.json)：NSIS安装启动卸载实际通过、字节一致，MSI安装未验；下载包与源码SHA匹配。
- [本机检查与锁一致性](local-verification.json)、[Foundation四job成功索引](foundation-index.json)：305条共享锁记录匹配，其中291条registry的source/checksum匹配；四份私有回归原日志checkout均同41db64d，不把之前或运行中的任务算作通过。

审计应同时检查错误分类是否无秘密/无自动重发、目录元数据在写库前的验证和Key过滤、替换/移除Key期间旧结果是否拒写、模型与推理/速度是否真正进入请求、供应商Key不按模型复制、旧配置读取，以及真实安装字节与release一致。默认Chat.send契约保留，桌面显式使用已有DeepSeek编译支持，不提供工具执行权限。

生产/cloud/auth/邮件/凭据核心/根Cargo.lock及Foundation工作流未改动。Foundation原日志仅存本机Git忽略的私有证据目录，不导出；索引只记录精确GitHub任务与hash。没有商业推理或生产请求；目录返回条目、能力显示与真实可调用性分开。详细限制见修正文档。
