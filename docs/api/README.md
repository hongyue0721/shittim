# Shittim API 文档

## 当前状态

已有首批v1 JSON Schema源、manifest、Rust生成类型与校验/哈希API、纯领域状态机、SQLite基座，以及不可连接的v1 KCP preflight/dispatcher/handlers。当前manifest共有53个Schema（41 retained + 12 component-native）；ADR-0006相关首批12项的source、manifest entries、generated Rust root types、Task creation纯library和三份official fixtures/harness已落地，但production MethodVersionBindings仍为空。ADR-0006已将active合同升级为root-only TaskCreate v2与Action-only Child Task，后续repository、handler、child materializer、method-aware preflight和cutover仍未完成；ADR-0007的Approval/PermissionDecision v2仍为contract-only。当前**没有**可连接`agentd`、稳定网络endpoint或TypeScript客户端包。

本目录是中文导航，不是新的事实源。字段、状态机、错误和兼容规则以 `specs/` 及 `schemas/source` 为准。Core API 不暴露预埋的 Computer Use 方法；未来 Profile 的方法必须在正式 Schema、Catalog 和 Extension SDK Base 组合契约确立后再出现。`desktop-client` 是桌面客户端，不是 Computer Use。

## 文档

- [Schema 生成与契约类型](schema-generation.md)
- [domain-task 内部 Rust API](domain-task.md)（非 KCP 外部 API）
- [domain-policy 内部 Rust API](domain-policy.md)（非 KCP 外部 API）
- [kernel-sqlite 内部 Rust API](kernel-sqlite.md)（文件 migration、Audit、Outbox、rate limit、Task create/get；非 KCP 外部 API）
- [KCP Value preflight 与注册式 dispatcher](kcp-preflight-dispatcher.md)（已实现、不可连接、非 SDK）
- [kernel-kcp typed application handler](kernel-kcp.md)（`system.ping`、`task.create`、`task.get`；不可连接、非 SDK）
- [Task创建、Child materialization与迁移合同](task-repository-contract.md)（ADR-0006首批Schema、纯library与official fixtures/harness已落地；repository/handler/child materializer/cutover未完成；legacy v1 create/get已实现）
- [Approval v2与PermissionDecision授权合同](approval-contract.md)（contract-only）
- [AuditRecord版本合同](audit-record.md)（v1已实现；v2 producer contract-only）
- [Kernel Control Protocol](kernel-control-protocol.md)（method-aware active生命周期合同；Schema/root types已落地，runtime实现仍为retained v1库级路径）
- 首批正式事件索引：[Event Catalog](event-catalog.md)（active五类，含`action.state_changed`与`approval.state_changed`；当前代码只有v1三payload/Outbox）
- 稳定错误索引：[Error Catalog](error-catalog.md)（method lifecycle、v2业务/身份/CAS错误）

Core API 不预留 `snapshot`、`user_takeover` 或其他 Computer Use 专用方法；这些能力若未来实现，应通过 Optional Profile 的正式契约接入，而不是扩张 Core API。

## 权威来源

- KCP、对象和 Schema：[`../../specs/IMPLEMENTATION_CONTRACTS.md`](../../specs/IMPLEMENTATION_CONTRACTS.md)
- Event/Outbox / Task·Action 状态机：[`../../specs/CORE_ARCHITECTURE.md`](../../specs/CORE_ARCHITECTURE.md)
- Policy 与错误安全语义：[`../../specs/SECURITY_PRIVILEGE.md`](../../specs/SECURITY_PRIVILEGE.md)
- 自动化锚点：[`../../specs/CONFORMANCE.md`](../../specs/CONFORMANCE.md)

## 版本原则

KCP Envelope 使用 `protocol_version`；payload、Event payload 和持久对象使用 `schema_version`。第一版 KCP protocol 为 `1.0`。正式 Schema 使用 JSON Schema 2020-12，并通过 RFC 8785 canonical JSON 支撑稳定哈希与幂等等价比较。

`domain-task` 只产出领域转换结果与事件**意图**；`domain-policy` 只产出非持久 decision draft / canonical input，以及纯 TaskScope resource containment（不授权），并显式区分 Stop Fence/Recovery invariant。`kernel-sqlite` 已拥有文件 migration、AuditRecord JSON、Event Outbox、事务绑定 rate-limit 消费和 legacy Task create/get repository。`kernel-kcp` 当前仍是不可连接的 retained v1 Value preflight、registration/dispatcher、三个 handler 与 SQLite adapter；active method-aware preflight、TaskCreate v2 repository/handler、server 与 SDK/client 仍未完成。Task list/update、Action 与 PermissionDecision repository仍未实现。

这些当前实现和计数均属于 Core；没有 Computer Use 预埋 API、Schema 或实现状态。
