# Shittim API 文档

## 当前状态

已有首批v1 JSON Schema源、manifest、Rust生成类型与校验/哈希API、纯领域状态机、SQLite基座，以及不可连接的 method-aware KCP preflight/dispatcher/handlers（切片3b：active create v2 + ping/get）。当前manifest共有83个Schema（41 retained + 42 component-native）；ADR-0006首批12项、ADR-0008 Event v2八项、ADR-0009切片1a–3c（含 root repository、bindings、kcp runtime、v1 write 删除/Outbox v2-only/旧库拒绝）已落地，production MethodVersionBindings为IC §13.5八方法集（切片3a）。`kernel-authorization` pure crate现负责Delta/Material/Observation/Subject四投影，SubjectProjection含三branch official fixture；身份/证据八Schema与official fixtures已在1c-ii落地。SQLite migration 0001–0005、descriptor v1、**v2-only** Outbox 与 root `task.created` producer 已实现；其余 active business producer、Publisher、versioned KCP poll仍未实现。child materializer与Approval/PermissionDecision/Identity current-head CAS仍未完成；里程碑为`V2InitialBuildActive`（非cutover）。当前**没有**可连接`agentd`、稳定网络endpoint或TypeScript客户端包。

本目录是中文导航，不是新的事实源。字段、状态机、错误和兼容规则以 `specs/` 及 `schemas/source` 为准。Core API 不暴露预埋的 Computer Use 方法；未来 Profile 的方法必须在正式 Schema、Catalog 和 Extension SDK Base 组合契约确立后再出现。`desktop-client` 是桌面客户端，不是 Computer Use。

## 文档

- [双仓库与持续文档维护](../REPOSITORY_MAINTENANCE.md)（主仓权威、文档主动更新、纯文档镜像同步与发布门禁）

- [Schema 生成与契约类型](schema-generation.md)
- [domain-task 内部 Rust API](domain-task.md)（非 KCP 外部 API）
- [domain-policy 内部 Rust API](domain-policy.md)（非 KCP 外部 API）
- [kernel-sqlite 内部 Rust API](kernel-sqlite.md)（文件 migration 0001–0005、AuditRecord v2、v2-only Outbox、rate limit、root TaskCreate v2 + 严格读；非 KCP 外部 API）
- [KCP Value preflight 与注册式 dispatcher](kcp-preflight-dispatcher.md)（已实现、不可连接、非 SDK）
- [kernel-kcp typed application handler](kernel-kcp.md)（`system.ping`、`task.create`、`task.get`；不可连接、非 SDK）
- [Task创建、Child materialization与repository硬合同](task-repository-contract.md)（首批Schema/纯library/fixtures、切片1b authorization pure crate、切片2 root repository、切片3a–3c bindings/kcp/v1 write删除已落地；child materializer与Action/PD/Approval未完成；无v1数据迁移）
- [Approval v2与PermissionDecision授权合同](approval-contract.md)（contract-only）
- [AuditRecord版本合同](audit-record.md)（v2 Schema/generated/conformance已实现；repository/producer未实现）
- [Kernel Control Protocol](kernel-control-protocol.md)（method-aware active生命周期合同；Schema/root types与切片3b method-aware runtime已落地，仍为不可连接库级路径）
- 首批正式事件索引：[Event Catalog](event-catalog.md)（Event v2 Schema/catalog/typed decode、v2-only Outbox、root task.created producer已实现；其余 active producer/Publisher/versioned poll未实现）
- 稳定错误索引：[Error Catalog](error-catalog.md)（method lifecycle、v2业务/身份/CAS错误）

Core API 不预留 `snapshot`、`user_takeover` 或其他 Computer Use 专用方法；这些能力若未来实现，应通过 Optional Profile 的正式契约接入，而不是扩张 Core API。

## 权威来源

- KCP、对象和 Schema：[`../../specs/IMPLEMENTATION_CONTRACTS.md`](../../specs/IMPLEMENTATION_CONTRACTS.md)
- Event/Outbox / Task·Action 状态机：[`../../specs/CORE_ARCHITECTURE.md`](../../specs/CORE_ARCHITECTURE.md)
- Policy 与错误安全语义：[`../../specs/SECURITY_PRIVILEGE.md`](../../specs/SECURITY_PRIVILEGE.md)
- 自动化锚点：[`../../specs/CONFORMANCE.md`](../../specs/CONFORMANCE.md)

## 版本原则

KCP Envelope 使用 `protocol_version`；payload、Event payload 和持久对象使用 `schema_version`。第一版 KCP protocol 为 `1.0`。正式 Schema 使用 JSON Schema 2020-12，并通过 RFC 8785 canonical JSON 支撑稳定哈希与幂等等价比较。

`domain-task` 只产出领域转换结果与事件**意图**；`domain-policy` 只产出非持久 decision draft / canonical input，以及纯 TaskScope resource containment（不授权），并显式区分 Stop Fence/Recovery invariant。`kernel-sqlite` 已拥有文件 migration 0001–0005、AuditRecord v2、v2-only Event Outbox、事务绑定 rate-limit 消费和 active root TaskCreate v2 repository（legacy v1 write 已删）。`kernel-kcp` 当前是不可连接的 method-aware Value preflight、registration/dispatcher、三个 handler（ping/create v2/get）与 SQLite adapter；child materializer、Action/PD/Approval、`V2InitialBuildActive` 全谓词、server 与 SDK/client 仍未完成。Task list/update 仍未实现。

这些当前实现和计数均属于 Core；没有 Computer Use 预埋 API、Schema 或实现状态。
