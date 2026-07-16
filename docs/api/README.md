# Shittim API 文档

## 当前状态

已有首批 JSON Schema 源、manifest、Rust 生成类型与校验/哈希 API；**没有**可连接的 `agentd`、没有稳定网络 endpoint、没有 TypeScript 客户端包。

本目录是中文导航，不是新的事实源。字段、状态机、错误和兼容规则以 `specs/` 及 `schemas/source` 为准。

## 文档

- [Schema 生成与契约类型](schema-generation.md)
- [Kernel Control Protocol](kernel-control-protocol.md)
- [Event Catalog](event-catalog.md)
- [Error Catalog](error-catalog.md)

## 权威来源

- KCP、对象和 Schema：[`../../specs/IMPLEMENTATION_CONTRACTS.md`](../../specs/IMPLEMENTATION_CONTRACTS.md)
- Event/Outbox：[`../../specs/CORE_ARCHITECTURE.md`](../../specs/CORE_ARCHITECTURE.md)
- Policy 与错误安全语义：[`../../specs/SECURITY_PRIVILEGE.md`](../../specs/SECURITY_PRIVILEGE.md)
- 自动化锚点：[`../../specs/CONFORMANCE.md`](../../specs/CONFORMANCE.md)

## 版本原则

KCP Envelope 使用 `protocol_version`；payload、Event payload 和持久对象使用 `schema_version`。第一版 KCP protocol 为 `1.0`。正式 Schema 使用 JSON Schema 2020-12，并通过 RFC 8785 canonical JSON 支撑稳定哈希与幂等等价比较。
