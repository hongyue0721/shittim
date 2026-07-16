# Shittim API 文档

## 当前状态

已有首批 JSON Schema 源、manifest、Rust 生成类型与校验/哈希 API、纯领域 `domain-task` 状态机、Freedom-first `domain-policy` matcher、文件型 `kernel-sqlite` 持久化基座和 Task create/get repository，以及不可连接的 `kernel-kcp` 三方法 typed handler。当前**没有** raw KCP preflight/dispatcher、可连接 `agentd`、稳定网络 endpoint 或 TypeScript 客户端包。

本目录是中文导航，不是新的事实源。字段、状态机、错误和兼容规则以 `specs/` 及 `schemas/source` 为准。

## 文档

- [Schema 生成与契约类型](schema-generation.md)
- [domain-task 内部 Rust API](domain-task.md)（非 KCP 外部 API）
- [domain-policy 内部 Rust API](domain-policy.md)（非 KCP 外部 API）
- [kernel-sqlite 内部 Rust API](kernel-sqlite.md)（文件 migration、Audit、Outbox、rate limit、Task create/get；非 KCP 外部 API）
- [kernel-kcp typed application handler](kernel-kcp.md)（`system.ping`、`task.create`、`task.get`；不可连接、非 SDK）
- [Task repository 创建与读取契约](task-repository-contract.md)（create/get 与三方法 typed handler 已实现；list/raw/server 未实现）
- [AuditRecord v1](audit-record.md)（本地不可变审计事实；非公开 Event）
- [Kernel Control Protocol](kernel-control-protocol.md)
- [Event Catalog](event-catalog.md)
- [Error Catalog](error-catalog.md)

## 权威来源

- KCP、对象和 Schema：[`../../specs/IMPLEMENTATION_CONTRACTS.md`](../../specs/IMPLEMENTATION_CONTRACTS.md)
- Event/Outbox / Task·Action 状态机：[`../../specs/CORE_ARCHITECTURE.md`](../../specs/CORE_ARCHITECTURE.md)
- Policy 与错误安全语义：[`../../specs/SECURITY_PRIVILEGE.md`](../../specs/SECURITY_PRIVILEGE.md)
- 自动化锚点：[`../../specs/CONFORMANCE.md`](../../specs/CONFORMANCE.md)

## 版本原则

KCP Envelope 使用 `protocol_version`；payload、Event payload 和持久对象使用 `schema_version`。第一版 KCP protocol 为 `1.0`。正式 Schema 使用 JSON Schema 2020-12，并通过 RFC 8785 canonical JSON 支撑稳定哈希与幂等等价比较。

`domain-task` 只产出领域转换结果与事件**意图**；`domain-policy` 只产出非持久 decision draft / canonical input，并显式区分 Stop Fence/Recovery invariant。`kernel-sqlite` 已拥有文件 migration、AuditRecord JSON、Event Outbox、事务绑定 rate-limit 消费和 Task create/get repository。`kernel-kcp` 已实现三个 typed handler、稳定 response/error 构造、post-commit intent 与 SQLite adapter，但不接受 raw JSON、不提供 dispatcher 或 server；Task list/update、Action 与 PermissionDecision repository仍未实现。
