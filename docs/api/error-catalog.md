# Error Catalog

> 状态：Schema 与 `system.ping` / `task.create` / `task.get` typed handler 的稳定映射均已实现。机器错误码与固定字段的唯一事实源见 [`IMPLEMENTATION_CONTRACTS.md` §5.7](../../specs/IMPLEMENTATION_CONTRACTS.md#57-首批错误目录) 与 [§5.10.5](../../specs/IMPLEMENTATION_CONTRACTS.md#5105-稳定-kcp-error-mapping)。

## Preflight 与 typed handler 分界

raw JSON/frame 层在 Envelope Schema、payload Schema、protocol/auth/method/schema 版本或 typed decode 失败时，可以返回：

- `invalid_request`
- `unsupported_protocol_version`
- `unsupported_schema_version`
- `unsupported_method`
- `unsupported_auth_schema`

三个 typed application handler 的输入已经通过这些门，因此不得在 handler 内返回 `invalid_request`，也不新增 `method_unavailable`。

## 三方法稳定映射

本表所有错误固定 `schema_version=1`、`details=null`。实现按 backend 稳定枚举或 `StoreErrorCode` 映射，禁止匹配 `StoreError.message`。

| 条件/分类 | code | 固定 message | retryable |
|---|---|---|---:|
| 入口或完成检查到期 | `deadline_exceeded` | `request deadline exceeded` | true |
| task.get 无记录 | `task_not_found` | `task was not found` | false |
| InvalidScopePattern | `invalid_scope_pattern` | `task scope contains an invalid URI pattern` | false |
| IdempotencyConflict | `idempotency_conflict` | `idempotency key was used for different task facts` | false |
| DelegationNotFound | `delegation_not_found` | `delegation was not found` | false |
| ParentTaskNotFound | `parent_task_not_found` | `parent task was not found` | false |
| ParentOriginNotFound | `parent_origin_not_found` | `parent content origin was not found` | false |
| SqliteBusy | `sqlite_busy` | `kernel storage is busy` | true |
| SqliteFull | `sqlite_full` | `kernel storage is full` | false |
| SqliteCorrupt | `sqlite_corrupt` | `kernel storage is corrupt or invalid` | false |
| StoredDataInvalid | `stored_data_invalid` | `stored task data failed integrity validation` | false |
| constraint/contract/serialization/not-found/internal，以及 open/config/migration/schema/clock/ID/response contract 等其余失败 | `internal_error` | `internal kernel error` | false |

`deadline_exceeded` 对 Query 表示可安全重试；对 `task.create` 只表示可用**同一** idempotency key 与同一业务投影重试。commit 后发现超时不撤销已提交事实。

## 其余首批目录

完整首批目录还包含 `revision_conflict`、`stop_fence_active`、`unsupported_policy_condition` 以及其它方法专属 Task/Origin/Subscription/Cursor/Event type 错误。它们不在本批三个 typed handler 的适用矩阵中，不能被提前实现成平行事实。

## 客户端规则

- 只根据响应中的 `retryable` 决定是否建议重试，并结合原方法恢复语义；
- `task.create` 的 `deadline_exceeded` 或 `internal_error` 可能发生在 commit 后，不能假定命令未生效；应保留同一 idempotency key 重放，或在已知 Task ID 时查询；
- `unsupported_policy_condition` 不是“无规则命中”，不得转换成 Default Allow；
- `stop_fence_active` 期间已存在的 pending Action 保持 pending；第一版没有 Fence 解除 API；
- `internal_error` 不展示内部 Secret、Token、完整敏感 payload、数据库路径、SQL 或堆栈。
