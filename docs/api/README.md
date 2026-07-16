# Shittim API 文档

## 当前状态

当前只有规范，代码尚未开始。没有可连接的 `agentd`、没有生成的 JSON Schema、没有稳定客户端库，也没有已部署 API endpoint。

本目录用于给实现者提供中文导航，不是新的事实源。字段、状态机、错误和兼容规则以规范及未来生成的 JSON Schema 为准。

## 文档

- [Kernel Control Protocol](kernel-control-protocol.md)
- [Event Catalog](event-catalog.md)
- [Error Catalog](error-catalog.md)

## 权威来源

- KCP、对象和 Schema：[`../../specs/IMPLEMENTATION_CONTRACTS.md`](../../specs/IMPLEMENTATION_CONTRACTS.md)
- Event/Outbox：[`../../specs/CORE_ARCHITECTURE.md`](../../specs/CORE_ARCHITECTURE.md)
- Policy 与错误安全语义：[`../../specs/SECURITY_PRIVILEGE.md`](../../specs/SECURITY_PRIVILEGE.md)
- 自动化锚点：[`../../specs/CONFORMANCE.md`](../../specs/CONFORMANCE.md)

## 版本原则

KCP Envelope 使用 `protocol_version`；payload、Event payload 和持久对象使用 `schema_version`。第一版 KCP protocol 为 `1.0`。正式 Schema 将使用 JSON Schema 2020-12，并通过 RFC 8785 canonical JSON 支撑稳定哈希与幂等等价比较。
