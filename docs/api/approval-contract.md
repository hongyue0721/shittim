# Approval v2 与 PermissionDecision 授权合同

> 状态徽章：**schema/library partial**（切片1c-i五个授权核心Schema + 切片1c-ii八个身份/挑战/证据/远程签名Schema、generated Rust、typed conformance、SubjectProjection pure API与official fixtures已实现；Approval/PD/Identity repository、current-head CAS、producer与真实验签仍未实现）

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

## 已实现的1c-ii表面

- `RemoteSignatureAlgorithmV1`：仅`ed25519` tagged branch，unknown field/branch fail closed；
- `CredentialRefV1`：credential revision + algorithm union + status 闭集；
- `RemoteApprovalChallengeV1` / `SystemAuthenticationChallengeV1`：challenge wire、`allowed_decisions`精确有序array const、nonce≥32 bytes encoding、state 闭集；
- `LocalPresenceEvidenceV1` / `SystemAuthenticationEvidenceV1`：本地/OS 证据 wire；local 不复制 subject_hash，system `result=verified`；
- `RemoteApprovalResponseV1` / `RemoteApprovalSignaturePreimageV1`：响应与签名 preimage；response 禁止 expires_at/public_key 覆盖。

Official fixtures：`schemas/fixtures/policy/{remote_signature_algorithm,credential_ref,remote_approval_challenge,system_authentication_challenge,local_presence_evidence,system_authentication_evidence,remote_approval_response,remote_approval_signature_preimage}.v1.json`。preimage 额外保存 JCS UTF-8 hex 与 SHA-256。本片只证明 Schema/typed shape 与 fixture 向量，不宣称 repository CAS、TTL 相对时间、nonce CSPRNG 或真实验签已实现。

## 范围

本页只导航 Authorization / Approval v2 合同边界：不可变 request/resolution/invalidation 链、subject 精确绑定、material 与 observation 分离、identity/challenge 证据与 current-head CAS。不复述字段、hash、真值表或 repository API 形状。

## 导航

- [Error Catalog](error-catalog.md)
- [Event Catalog](event-catalog.md)
- [Task repository 合同](task-repository-contract.md)
- [API 索引](README.md)
