# Event Catalog

> 状态徽章：**partial**（Event v2 八 Schema / catalog / typed decode 与 SQLite migration 0003 / 统一 Outbox shape **已实现**；V2InitialBuildActive切片1a又落地AuditRecordV2、AuditAllocationV2与TaskCreationProvenanceV1，明确Audit/Event正交；active business producer、Publisher、versioned KCP poll **未实现**；legacy Outbox append production API 待删除）

## 唯一事实源

| 主题 | 锚点 |
|---|---|
| 首批正式 Event Catalog / payload 真值表 | [`IMPLEMENTATION_CONTRACTS.md` §5.6](../../specs/IMPLEMENTATION_CONTRACTS.md#56-首批正式-event-catalog) |
| Action transition authority / `ActionTransitionIntentV1` 边界 | [`IMPLEMENTATION_CONTRACTS.md` §6.14](../../specs/IMPLEMENTATION_CONTRACTS.md#614-action-transition-authority) |
| CausationRef / EventEnvelope 版本 | [`IMPLEMENTATION_CONTRACTS.md` §6.15](../../specs/IMPLEMENTATION_CONTRACTS.md#615-causationref-与-eventenvelope版本) |
| Event v2 八 Schema 精确 ID/source/DAG | [`IMPLEMENTATION_CONTRACTS.md` §13.6.2](../../specs/IMPLEMENTATION_CONTRACTS.md#1362-event-v2-八schema实现合同schemacompilergenerated已落地) |
| 决策与 Outbox/poll 边界 | [ADR-0008](../../adr/0008-active-event-v2与版本化统一outbox.md)（部分被 [ADR-0009](../../adr/0009-v2从零构建并取消v1数据迁移.md) supersede） |
| Outbox 事务语义 | [`CORE_ARCHITECTURE.md` §17](../../specs/CORE_ARCHITECTURE.md#17-事务边界与-sqlite-outbox) |
| 域状态 | [`../IMPLEMENTATION_MATRIX.md`](../IMPLEMENTATION_MATRIX.md) · [`../PROGRESS.md`](../PROGRESS.md) |

## 范围

本页索引 active Event Catalog、CausationRef v2、typed envelope、版本化统一 Outbox 与 retained `event.poll` v1 不得返回 v2 的边界。不复述八 Schema 字段、claimant 谓词、binding 常量或 mixed API 形状。AuditRecordV2与AuditAllocationV2虽已进入Schema/generated链，仍不是Event或Outbox envelope；challenge expiry只写Audit且不发`approval.state_changed`。

## 导航

- [kernel-sqlite API](kernel-sqlite.md)
- [Error Catalog](error-catalog.md)
- [Approval 合同](approval-contract.md)
- [API 索引](README.md)
