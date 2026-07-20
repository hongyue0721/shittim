# Approval v2 与 PermissionDecision 授权合同

> 状态徽章：**schema/library partial**（切片1c-i五个授权核心Schema、generated Rust、typed conformance、SubjectProjection pure API与official fixture已实现；Approval/PD repository、current-head CAS、identity/challenge/evidence/remote signature、producer仍未实现）

## 唯一事实源

| 主题 | 锚点 |
|---|---|
| 可编码投影 / subject / confirmation mode / JCS preimage | [`IMPLEMENTATION_CONTRACTS.md` §5.3.1](../../specs/IMPLEMENTATION_CONTRACTS.md#531-可编码投影jcs-preimage-与规范化) |
| Event payload / `change_kind` 真值表 | [`IMPLEMENTATION_CONTRACTS.md` §5.6](../../specs/IMPLEMENTATION_CONTRACTS.md#56-首批正式-event-catalog) |
| 稳定错误码 | [`IMPLEMENTATION_CONTRACTS.md` §5.7](../../specs/IMPLEMENTATION_CONTRACTS.md#57-v2权威-error-catalog) |
| PermissionDecision v2 | [`IMPLEMENTATION_CONTRACTS.md` §6.6](../../specs/IMPLEMENTATION_CONTRACTS.md#66-permissiondecision-v2) |
| ApprovalRecord v2 联合身份 / CAS / material·observation | [`IMPLEMENTATION_CONTRACTS.md` §6.10](../../specs/IMPLEMENTATION_CONTRACTS.md#610-approvalrecord-v2) |
| Event allocation / repository 闭集 / 唯一键 / 恢复硬门 | [`IMPLEMENTATION_CONTRACTS.md` §6.10.6](../../specs/IMPLEMENTATION_CONTRACTS.md#6106-approval-event-allocationrepository闭集唯一键与恢复硬门) |
| 决策背景与边界 | [ADR-0007](../../adr/0007-approval-v2不可变联合身份与失效.md) |
| 无迁移决策 | [ADR-0009](../../adr/0009-v2从零构建并取消v1数据迁移.md) |
| 域状态 | [`../IMPLEMENTATION_MATRIX.md`](../IMPLEMENTATION_MATRIX.md) · [`../PROGRESS.md`](../PROGRESS.md) |

## 已实现的1c-i表面

- `PermissionDecisionV2`：完整stored字段、material/observation fingerprint分离、canonical confirmation mode映射与required-nullable Approval binding；
- `PolicyRuleV2`：stored v2字段与`generic|local|system_authentication|remote_signature|plan_revision`闭集；
- `ApprovalRecordV2`：`record_kind`外层与`subject_kind`内层真正tagged enum，禁止v1平面optional拼装；
- `SubjectProjectionV1`：三subject branch唯一preimage；`kernel-authorization::project_subject_projection`返回typed value、JCS bytes与lowercase SHA-256；
- `ApprovalEventAllocationV1`：`event_id/correlation_id/dedup_key/changed_at/causation_ref`封闭对象。

Official fixtures：`schemas/fixtures/policy/subject_projection.v1.json`三branch各含subject、raw typed facts JSON、normalized projection、JCS UTF-8 hex、SHA-256与逐字段tamper；`schemas/fixtures/policy/approval_event_allocation.v1.json`按allocation惯例只含valid allocation与Schema tamper，不保存JCS/hash preimage。本片只证明Schema/typed shape，不宣称repository互异/causation/replay验证已实现。

身份/证据家族（Credential、Remote/System Challenge、Local/System Evidence、Remote Response/Signature Preimage）明确属于1c-ii。

## 范围

本页只导航 Authorization / Approval v2 合同边界：不可变 request/resolution/invalidation 链、subject 精确绑定、material 与 observation 分离、identity/challenge 证据与 current-head CAS。不复述字段、hash、真值表或 repository API 形状。

## 导航

- [Error Catalog](error-catalog.md)
- [Event Catalog](event-catalog.md)
- [Task repository 合同](task-repository-contract.md)
- [API 索引](README.md)
