# Task创建、Child materialization与repository硬合同

> 状态：active v2 contract-only；现有Rust/SQLite只实现legacy TaskCreate v1 create/get，不得进入未来production server。

## Lifecycle矩阵

| path | active | legacy | 状态 |
|---|---:|---:|---|
| KCP `task.create` request | 2 | 1 validation/read | v2未实现；v1 handler已实现但须退出active dispatcher |
| KCP `task.get/list` | 1 | — | get库级已实现，list未实现 |
| child create | Kernel Action `kernel.task/task.child.create` proposal v1 | direct child command v1 read/migration | Action/PD/Approval/materializer未实现 |

## Root v2

Envelope task_id/expected_revision固定null；payload无parent。`NormalizedRootTaskCreatePayloadV2`完整字段、InputContentOriginV1、数组规则与receipt/idempotency两份preimage均以IC §5.3.1为准：它与TaskCreateRequest v2一一对应，receipt就是该payload的JCS；Envelope context不属于payload，只在idempotency projection出现一次。仅规范化source URI与Scope patterns，保序保重复。root receipt/idempotency各有独立v2 fixture，不能复用legacy或child fixture。

单事务创建Origin v2、Scope、root Task、Provenance、Audit v2与一个`task.created` EventEnvelope v2；使用`RootTaskCreateAllocationV2`逐purpose分配**七个**UUID并验证ID互异/opaque correlation-dedup；所有时间来自唯一accepted_at，commit前canonical readback。legacy v1六UUID allocation不属于active root v2。

## Child Action

固定capability/operation/class：`kernel.task` / `task.child.create` / S1。proposal完整显式声明child facts、Scope、Delegation与Origin input，禁止child ID/parent/status/revision/time/shadow字段。父Task只取Action.task_id。

proposal的来源类型是`InputContentOriginV1`，stored事实是`ContentOriginV2`，禁止混名。Kernel构造并hash：

- `NormalizedChildTaskProposalV1`；
- `ChildTaskDeltaProjectionV1`；
- `MaterialAuthorizationProjectionV1`；
- `ObservationEvidenceProjectionV1`。

执行前验证Action revision/status、current PD/可消费Approval、Lease holder/generation/expiry、Stop Fence、Delegation authority与proposal引用。

### 原子bundle

同一事务创建Origin/Scope/Task/Provenance/Verification/Audit、child `task.created`、Action `action.state_changed`，并更新Action completed result/revision。使用`ChildTaskMaterializationAllocationV1`；Action event因果是正式`ActionTransitionRefV1`并先持久化`ActionTransitionIntentV1`，禁止self-causation。Action ID是child-by-action唯一业务键，跨generation最多一个child。

同Action同proposal/material hash且bundle完整为合法重放；同Action不同proposal/material为`child_materialization_conflict`；同execution idempotency key绑定不同Action为`idempotency_conflict`；mapping不完整为`stored_data_invalid`，禁止补半包。

## Repository硬门

Action、PermissionDecision、Approval、Identity、ActionTransition、RootCreate、ChildMaterialization使用IC规范闭集API，不暴露SQL/transaction。Approval三种CAS操作都必须消费`ApprovalEventAllocationV1`；Challenge读取只读，过期由`expire_challenge_with_expected_state`在resolve/consume事务中CAS持久化为expired并写identity/security Audit，绝不写`approval.state_changed`。必要unique facts和PD↔Approval↔Action一致性以IC §6.10.6为准。

读取固定：Schema版本验证→JCS byte equality→typed tagged-union decode→关系镜像/唯一键→current refs→重算projection hash；失败只读返回stored_data_invalid。

reconciliation只返回committed/absent/corrupt；migration先preflight+backup，写provenance/current projections，verify后再切active registration。legacy direct-child标`legacy_direct_create_v1`，不造Action/PD/Approval/Verification。

## 错误

稳定码和safe details见[Error Catalog](error-catalog.md)，覆盖scope/delegation、Action/PD/Approval、lease/fence、child uniqueness/material conflict、observation stale与stored corruption。`task.list` cursor编码仍正交未决。
