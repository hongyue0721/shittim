# Event Catalog

> 状态：EventEnvelope 与三个首批 payload 的契约、Schema、Rust 生成类型、校验及 SQLite Outbox 已完成；task.create 已生产唯一 `task.created`，Publisher 和订阅服务未实现。EventEnvelope 与 Outbox 语义以 [`CORE_ARCHITECTURE.md`](../../specs/CORE_ARCHITECTURE.md) 为准，首批 payload 以 [`IMPLEMENTATION_CONTRACTS.md` 第 5.6 节](../../specs/IMPLEMENTATION_CONTRACTS.md#56-首批正式-event-catalog) 为准。

## EventEnvelope 关键语义

- `type` 使用点号分隔的小写名称；
- `aggregate_type` + `aggregate_id` 标识聚合；
- `sequence` 是聚合内已提交事件序号：首条为 `0`，后续严格连续 `+1`，回滚事务的暂分配不占号；
- `outbox_position` 是全局单调投递位置，不表示跨聚合领域因果；
- `causation_ref.kind` 只允许 `command_request | event`；
- cursor 只使用 `outbox_position`；
- `delivered_at` 只表示 Publisher 已发布，不表示各订阅者已消费；
- at-least-once 下消费者必须按 `dedup_key` 或 `event_id` 幂等。

## 首批正式事件

| type | aggregate_type | aggregate_id | payload 状态 |
|---|---|---|---|
| `task.created` | `task` | Task ID | schema_version 1 已在规范定义 |
| `task.state_changed` | `task` | Task ID | schema_version 1 已在规范定义 |
| `stop_fence.activated` | `stop_fence` | `global` | schema_version 1 已在规范定义 |

AuditRecord 是独立的本地不可变审计对象，不属于这三个事件的 payload，也不会仅因写入 Audit Store 自动成为公开事件或进入 Outbox。参见 [AuditRecord v1](audit-record.md)。

其他内部事件名称（包括未来 Profile 可能使用的 `snapshot`、`user_takeover` 等 Extension event）只有在加入正式 payload Schema、兼容说明和 Conformance 测试后，才成为对应 Profile 的 Catalog 成员。它们默认不是公共 Kernel Event；只有正式晋升并纳入公共 Schema/Catalog 后，才可成为公共 Kernel Event。

## 当前状态

目前已有 SQLite Outbox 表、sequence/position 原子分配、历史/未投递读取与 task.create producer；仍没有 Publisher、订阅 server 或消费 SDK，因此不能声称事件已经可外部订阅。未来 Computer Use Profile 的 `snapshot`、`user_takeover` 等名称即使由 Provider 返回，也默认只是 Profile Extension event；Provider 返回的 Event-like JSON 不能自行晋升为 Kernel Event。
