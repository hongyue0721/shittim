# Event Catalog

> 状态徽章：**partial**（Event v2 八 Schema / catalog / typed decode 与 SQLite migration 0003–0006 / 统一 Outbox shape **已实现**；V2InitialBuildActive切片1a又落地AuditRecordV2、AuditAllocationV2与TaskCreationProvenanceV1，明确Audit/Event正交；切片3c 起 production Outbox 为 **v2-only**（`append_legacy_event_v1` / `LegacyV1` 已删，旧库 reinitialize-required）；root `task.created` 与切片4a `action.state_changed`（causation=`action_transition`）active producer 已接入；child/approval producer、Publisher、versioned KCP poll **未实现**）

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

本页索引 active Event Catalog、CausationRef v2、typed envelope、版本化统一 **v2-only** Outbox 与 retained `event.poll` v1 不得返回 v2 的边界。不复述八 Schema 字段、claimant 谓词、binding 常量或已删除的 legacy append API 形状。AuditRecordV2与AuditAllocationV2虽已进入Schema/generated链，仍不是Event或Outbox envelope；challenge expiry只写Audit且不发`approval.state_changed`。切片4a已在`kernel-sqlite`落地`ActionTransitionIntentV1` repository 与唯一 owner producer：`mark_committed_with_event` 同事务 CAS Action 并 append `action.state_changed`，causation 精确为该 intent 的 `ActionTransitionRefV1`；`reconcile_intent` 只返回 prepared|committed|corrupt，不补造。child/approval producer 仍未实现。

切片1c-i新增`ApprovalEventAllocationV1` Schema/generated typed对象，作为未来Approval三种CAS方法required输入；它没有实现repository消费、ID/opaque互异验证或`approval.state_changed` producer。其`causation_ref`直接复用`CausationRefV2`，`changed_at`限制为UTC秒精度。

## 导航

- [kernel-sqlite API](kernel-sqlite.md)
- [Error Catalog](error-catalog.md)
- [Approval 合同](approval-contract.md)
- [API 索引](README.md)
