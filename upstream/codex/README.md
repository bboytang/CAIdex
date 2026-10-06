# Codex 上游边界

本阶段使用固定版本的官方 CLI 二进制启动 app-server；不把整个上游源码复制入当前工程。源码基准为 [release commit](https://github.com/openai/codex/tree/d27764b82f7118f674371e6d6e76271d9d606edb)，版本与来源记录在 `lock.json`。

`schemas/` 包含 0.160.1 生成的完整常规与实验协议 bundle，以及初始信封/初始化原件，遵循旁边的上游 Apache-2.0 许可证。两份 bundle 的 SHA-256 已记录在 `lock.json`；CI 使用相同 CLI 重新生成并检查指纹。实验版需要 `generate-json-schema --experimental`，不能以默认输出代表全部方法。

CAIdex 代码只在 `runtime/bridge` 对接 JSONL 信封与 Runtime facade；未知事件、服务端请求及扩展字段保持原样。客户端方法由锁定 schema 生成清单，实验方法需明确 opt-in；方法存在不代表特定 Host/模型支持。功能对照和剩余验收见 `docs/CAIdex-Runtime-能力对照.md`，避免 UI 直接依赖上游内部 Rust 结构。

升级必须同步修改锁定文件、schema 基准和版本校验，并执行协议/Runtime 回归。当前没有上游补丁，也未验证从源码重建二进制。
