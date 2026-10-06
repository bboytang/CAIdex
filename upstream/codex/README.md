# Codex 上游边界

本阶段使用固定版本的官方 CLI 二进制启动 app-server；不把整个上游源码复制入当前工程。源码基准为 [release commit](https://github.com/openai/codex/tree/d27764b82f7118f674371e6d6e76271d9d606edb)，版本与来源记录在 `lock.json`。

`schemas/` 是 `codex app-server generate-json-schema` 在 0.160.1 上生成的少量协议基准原件，遵循旁边的上游 Apache-2.0 许可证。完整 schema bundle 的 SHA-256 已记录；需要完整方法类型时从相同 CLI 生成，不读取 main 分支的变化替代锁定版本。

CAIdex 代码只在 `runtime/bridge` 对接 JSONL 信封；未知方法、参数、扩展字段保持原样。后续在该边界之外映射应用协议，避免 UI 直接依赖上游内部 Rust 结构。

升级必须同步修改锁定文件、schema 基准和版本校验，并执行协议/Runtime 回归。当前没有上游补丁，也未验证从源码重建二进制。
