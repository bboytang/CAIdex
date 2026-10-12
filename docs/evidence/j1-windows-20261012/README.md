# J-1 独立审计入口

功能与构建源码：`ea0cdda82f92c85f033471d48d961d0fa71fb810`，基线：`ae26c9bde0b679667964afe755ccdb0189514574`。本目录和验收文档是随后文档/证据封存，不代表以封存commit重新构建安装包。独立审计尚未执行；实施自查不授通过。

- [设计、实际下载与限制](../../CAIdex-J1-Windows-设计与验收-V1.md)
- [32项源码SHA256](source-fingerprints.json)：比对immutable源码；安装配置/测试/共享核心包含在内。
- [验证索引](verification-index.json)、[本机实际检查](local-verification.json)、[Windows两job](windows-run.json)。
- [原生20项](native/native.json)、[9后端与原生原日志](windows.log.gz)、[5前端原日志](frontend.log.gz)、[前端结构化结果](renderer/frontend.json)。GUI原日志解压hash见verification-index；两份checkout核对指定SHA。
- [Foundation四job索引](foundation-index.json)：四checkout均同源码SHA；原始回归/邮件日志仅保存本机Git忽略私有目录，不随公开证据导出，可使用精确GitHub job或获授权的本机只读审计。
- [安装清单](native/build-manifest.json)、[实际下载字节复核](download-verification.json)、[artifact有效期/摘要](artifacts.json)。NSIS实际安装/启动/卸载通过；MSI仅构建/hash核对。
- `renderer/`是模拟IPC状态截图；`native/`两图是真实release WebView状态，均只含合成测试内容。不能外推商业模型、Windows11人工交互或DPI/多屏/硬件IME。

只读审计应核对源码/能力白名单/CSP、Broker与SQLite边界、Key无读取IPC、会话与草稿/停止/unknown不重发、精确构建产物与NSIS安装证据；不得把其范围外的账户/设备/Host/Codex/云Memory/同步视作已接入。生产、已验邮件与账户源码和原Foundation工作流不变，根Cargo.lock不变。

已修复的开发失败：独立workspace/TS/选择器与Clippy问题、Anthropic/Gemini构造限制、bundler目标文件名、WebView预建目录继承ACL、连续主题语言保存竞争、合成Secret测试类型、安装包类型标记与release哈希不一致。最后一项在38199849370/38200566319失败；仅调打包顺序无效，依据锁定Tauri官方源码用no-binary-patching修复，38239803102完整通过。没有把失败或取消算作通过。

GUI文件下载/剪贴板/原生菜单点击、MSI安装、签名/SmartScreen/升级、商业模型与真实附件兼容性未验。草稿有400ms落盘窗口，未发送附件UI选择重启不恢复；详见验收文档。结束后等待独立审计与用户下一范围授权，不继续开发。
