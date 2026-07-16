# Event Catalog

> 状态：仅规范，未实现。EventEnvelope 与 Outbox 语义以 [`CORE_ARCHITECTURE.md`](../../specs/CORE_ARCHITECTURE.md) 为准，首批 payload 以 [`IMPLEMENTATION_CONTRACTS.md` 第 5.6 节](../../specs/IMPLEMENTATION_CONTRACTS.md#56-首批正式-event-catalog) 为准。

## EventEnvelope 关键语义

- `type` 使用点号分隔的小写名称；
- `aggregate_type` + `aggregate_id` 标识聚合；
- `sequence` 是聚合内单调序号；
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

其他内部事件名称只有在加入正式 payload Schema、兼容说明和 Conformance 测试后，才成为对外 Catalog 成员。

## 当前状态

目前没有 Outbox 表、Publisher、订阅 server、生成事件类型或消费 SDK。本文不得被用于声称事件已经可订阅。
