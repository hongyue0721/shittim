# AuditRecord版本合同

> 状态：AuditRecord v2、AuditAllocationV2与TaskCreationProvenanceV1的source/manifest/generated Rust/conformance已实现；root `task.create` v2 已写入 `AuditRecordV2`（切片2/3b）。v1 Schema/fixture 仅历史验证；v1 SQLite Store 与 legacy task.create producer 已在切片3c删除（0005 drop 死表，旧库 reinitialize-required）。其它 active producer 的 v2 Audit 仍未实现。完整wire与producer矩阵唯一事实源为IC §6.16。

## v1

v1仅保留Schema/fixture历史验证资产，无production read/write/migration。全部字段required，无关联事实用null/空数组；`approval_record_ref`与CausationRef v1只属于legacy shape。v1 `audit_type`闭集仅七类：`task.creation_recorded`、`command.accepted`、`permission.evaluated`、`kernel.invariant_blocked`、`event.published`、`recovery.recorded`、`config.changed`。切片3c 起 SQLite 不再持有 v1 Audit Store 或 legacy task.create 同事务 producer；root create 的生产证明路径仅为 AuditRecordV2。

## v2完整wire

v2 source位于`schemas/source/audit/audit_record.v2.json`，manifest compatibility=`breaking-replacement`，generated root为`AuditRecordV2`。它不是“v1加字段”的增量对象。全部顶层required：`id,schema_version=2,audit_type,level,actor,entry_point,occurred_at,task_id,task_creation_context,action_id,permission_decision_ref,approval_resolution_ref,recovery_attempt_ref,delegation_ref,model_call_refs,payload_manifest_refs,external_content_status,verification_result_refs,content_origin_refs,artifact_refs,resource_refs,extension_id,provider_id,causation_ref,correlation_id,rollback_capability,stop_fence_generation,policy_context,outcome,reason_codes,summary,details`。完整类型/null/empty权威见IC §6.16。

## v2 `audit_type` 闭集（镜像 IC §6.16.0）

```text
task.creation_recorded
| command.accepted
| permission.evaluated
| kernel.invariant_blocked
| event.published
| recovery.recorded
| config.changed
| approval.requested
| approval.resolved
| approval.invalidated
| identity.challenge_expired
| identity.credential_registered
| identity.credential_rotated
| identity.credential_revoked
| identity.local_presence_recorded
| identity.system_authentication_recorded
```

列入闭集不等于producer已实现。Approval head变化使用`approval.*`；Challenge expiry使用`identity.challenge_expired`且不发`approval.state_changed`。固定归因必须在顶层字段，禁止只塞`details`。

## `AuditAllocationV2`（镜像 IC §6.16.0a）

该对象已按IC“正式required对象”与跨语言Schema验证边界source化为`schemas/source/audit/audit_allocation.v2.json`，manifest compatibility=`new-contract`，generated root为`AuditAllocationV2`；它不是仅供Rust内部的自由typed input。

```text
AuditAllocationV2 {
  audit_record_id,
  correlation_id,   // non-empty opaque
  occurred_at,
  causation_ref     // CausationRef v2
}
```

root/child/approval 可从更大bundle allocation投影同一四元组语义；challenge expiry禁止消费`ApprovalEventAllocationV1`。

## TaskCreationProvenanceV1

真正tagged union；source为`schemas/source/task/task_creation_provenance.v1.json`，generated Rust使用`#[serde(tag="kind")] enum TaskCreationProvenanceV1`，没有v1 optional平面struct。

| kind | 权威事实 |
|---|---|
| `root_command_v2` | command request、actor/entry、receipt；parent/action为null |
| `child_action_v2` | parent Task、Action、PD、可空Approval resolution、Verification、proposal/delta hash |

## Producer固定表（总览镜像；逐字段矩阵见 IC §6.16.2）

| producer | 时间 | causation | Audit关键null/empty/reason | Outbox |
|---|---|---|---|---|
| root TaskCreate v2 | 唯一`accepted_at`投影到Origin receipt/received、Scope、Task、Provenance、Audit、Event；materialized_at=null | command request | `task.creation_recorded` / `user_activity` / `task_created_root_v2`；Action/PD/Approval/Recovery/Extension/Provider/policy/summary null；verification/model/manifest/artifact/resource空 | 一个`task.created` |
| child Action materialization | 唯一`materialized_at`投影到所有新对象与Action transition；不改proposal既有accepted time | child Task/Audit=Action；Action event=`ActionTransitionRefV1` | `task.creation_recorded` / `task_created_by_action_v2`；Action/PD/Verification必有；Approval按真实消费可空；details空object | `task.created`+`action.state_changed` |
| Approval initial request | request业务time投影到record/Audit/event；消费`ApprovalEventAllocationV1` | 真实command/event/action来源 | `approval.requested` / `security` / `deferred` / `approval_requested`；resolution ref null | 一个`approval.state_changed` |
| Approval resolution | resolution业务time投影；消费`ApprovalEventAllocationV1` | 真实来源；remote还消费challenge | `approval.resolved` / approved→succeeded+`approval_resolved_approved`；denied→blocked+`approval_resolved_denied` | 一个`approval.state_changed` |
| Approval invalidation/replacement | 两record同一业务`changed_at`；消费`ApprovalEventAllocationV1` | invalidating command/event/action | `approval.invalidated` / `observed` / reason与invalidation `reason_code`一致；失效approved resolution时`approval_resolution_ref`指该resolution，失效尚未resolution的request时固定为null，禁止伪造resolution ref | 一个逻辑head `approval.state_changed` |
| Challenge expiry | issued→expired CAS的`expired_at`；**独立`AuditAllocationV2`** | resolve/consume/sweeper的真实来源 | `identity.challenge_expired` / `security` / `observed` / `challenge_expired`；approval/PD refs全null | **无**`approval.state_changed` |
| Credential register/rotate/revoke | 业务register/rotate/revoke time | 真实authority | `identity.credential_registered\|rotated\|revoked` | 默认无Approval/Task event |
| Local presence / system authentication evidence | evidence时间 | 受信transport/OS adapter | `identity.local_presence_recorded` / `identity.system_authentication_recorded` | 默认无Approval event；resolution另走approval producer |

root ContentOrigin carrier为command request；child carrier为Action。root receipt hash覆盖normalized TaskCreate v2 payload；child覆盖`NormalizedChildTaskProposalV1`。parent origin refs保序保重复；receipt不含carrier、IDs、时间、provenance或物化对象。

## 原子性

业务事实、Provenance、Audit以及规定Outbox必须在同一事务Schema验证、canonical readback和关系核对。任何ID/time/hash/causation/correlation/PD/Approval/Verification不一致整体回滚，不得用默认null或details掩盖缺失事实。
