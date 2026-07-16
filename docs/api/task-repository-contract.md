# Task Repository 创建契约

> 状态：规范已拍板，repository 尚未实现。本文是实现入口摘要；唯一事实源是 [`specs/IMPLEMENTATION_CONTRACTS.md` §5.5](../../specs/IMPLEMENTATION_CONTRACTS.md#55-首批正式-kcp-catalog)、[`specs/CORE_ARCHITECTURE.md` §17](../../specs/CORE_ARCHITECTURE.md#17-事务边界与-sqlite-outbox) 与 [`specs/CONFORMANCE.md` §5](../../specs/CONFORMANCE.md#5-kernel-control-protocolschema-与事件)。

## 范围

下一笔 `kernel-sqlite` Task repository 必须实现 `task.create` 的本地事务物化与读取基础，但本仓库当前仍没有这些表、migration 或 API。不得把本文当作已实现能力。

## 输入规范化

仅规范化：

- 非 null `payload.origin.source_uri`；
- `payload.task_scope.resource_patterns[]`；
- `payload.task_scope.exclusions[]`。

使用 SECURITY 的 Policy URI 语法。数组顺序和重复项保留；其他字符串不 trim、不排序、不去重。结果必须再次通过 `TaskCreateRequest` Schema。

## 两个 canonical hash

- receipt hash：规范化后的**完整 TaskCreateRequest payload object**做 RFC 8785 JCS UTF-8 + SHA-256 lowercase。
- idempotency hash：精确对象 `{actor, entry_point, command_type, task_id, context, expected_revision, payload}` 做同样计算；`command_type` 固定为 `task.create`，payload 使用规范化结果。

幂等 scope 是 `(actor.id, entry_point, command_type, idempotency_key)`。记录与 Task 同生命周期，v1 不清理；同 hash 返回原 task ID 和当前 revision，不同 hash 冲突。全本地单事务不设置 processing 状态。

可执行向量：[`schemas/fixtures/kcp/task_create_normalized_hash.v1.json`](../../schemas/fixtures/kcp/task_create_normalized_hash.v1.json)。这是同时承载 command envelope、唯一 `normalized_payload`、receipt hash 与 idempotency projection/hash 的复合 fixture，不是 schema-tool 通用 `$schema_id`/`instance` example wrapper。fixture 当前固定：

- receipt content hash：`e700949bc03cba21d834ccce21dc594193456bc4590869230f57c8d14effd272`
- idempotency projection hash：`2f64e3515bc58dd11fb42d46c0192d7e17a2076e6f06558927601897df7e9ffe`

`kernel-contracts` 测试会重新验证 Schema、投影字段边界、顺序/重复保留、URI 规范化输出和两个 Rust hash；`schema-tool` CLI smoke 还会抽取两条 JSON 路径，实际执行 `validate` 与两次 `canonicalize --hash`，独立断言同一 fixture hash。

## 单事务物化

Kernel 上层先显式提供 Task/TaskScope/ContentOrigin/receipt/Audit/Event UUID，以及 Event correlation/dedup，并固定一个 `accepted_at`。repository 在同一 `WriteTransaction` 中：

1. 校验 parent Task、parent origins 与 Delegation 引用；当前非 null Delegation 一律 `delegation_not_found`。
2. 写幂等记录。
3. 创建 ContentOrigin；receipt/received 时间均为 `accepted_at`。
4. 创建 TaskScope，`revision=1`，`source_refs` 恰好为新 origin ID。
5. 创建 candidate Task，`plan_version=0`、`revision=1`，双时间为 `accepted_at`。
6. 创建固定 `task.creation_recorded` AuditRecord。
7. 写唯一 `task.created` Outbox，聚合首序号为 `0`。

任何 Schema、引用、hash 或 Task/Audit/Event canonical 子事实不一致均回滚。

## 暂不拍板

`task.list` cursor 继续保持 opaque。具体编码和分页键技术选择必须在 repository 实现前通过 ADR 或 API 契约单独拍板。

## 未实现

- Task/TaskScope/ContentOrigin/idempotency 表和 migration；
- Task repository Rust API；
- Delegation authority 正向查询；
- `task.create` KCP handler；
- Task list cursor 编码。
