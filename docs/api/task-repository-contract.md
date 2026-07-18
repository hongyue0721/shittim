# Task创建、Child materialization与repository硬合同

> 状态：首批12项相关Schema source、manifest entries与generated Rust root types已落地；production MethodVersionBindings仍为空。active v2仍contract-only，official JCS/hash fixtures、repository canonical write/readback、handler与cutover未完成。现有Rust/SQLite只实现legacy TaskCreate v1 create/get，不得进入未来production server。

## Lifecycle矩阵

| path | active | legacy | 状态 |
|---|---:|---:|---|
| KCP `task.create` request | 2 | 1 validation/read | v2未实现；v1 handler已实现但须退出active dispatcher |
| KCP `task.get/list` | 1 | — | get库级已实现，list未实现 |
| child create | Kernel Action `kernel.task/task.child.create` proposal v1 | direct child command v1 read/migration | Action/PD/Approval/materializer未实现 |

## Root v2

Envelope task_id/expected_revision固定null；payload无parent。raw `TaskCreateRequestV2`、`ChildTaskProposalV1`与两份 normalized root的caller-owned字段完全同构。canonical字段合同宿主固定为既有root `NormalizedRootTaskCreatePayloadV2#/$defs`：宿主自身properties使用local fragment，另三root的九项caller字段均使用absolute fragment；`task_scope`与`origin`不得由这些root直接whole-schema引用`InputTaskScopeV1`/`InputContentOriginV1`绕过宿主，必须经宿主absolute fragment进入宿主的`$defs/task_scope`与`$defs/origin`，再分别whole-schema引用两个Input Schema。这只是中立task-create proposal字段宿主，不倒置root/child业务语义。保留四个独立root Schema身份，禁止第13个Schema或复制平行约束。`InputTaskScopeV1`是task component独立封闭对象，`InputContentOriginV1`是common component独立封闭对象。所有string数组可空、元素non-empty、保序保重复、无`uniqueItems`；非null `risk_hint`、非null `upstream_stable_id`同样non-empty，普通字符串不trim。

`NormalizedRootTaskCreatePayloadV2`完整字段与receipt/idempotency两份preimage以IC §5.3.1为准；receipt就是该payload的JCS。idempotency必须精确使用`RootTaskCreateIdempotencyProjectionV1 {schema_version:1,actor,entry_point,command_type:"task.create",task_id:null,context,expected_revision:null,payload:NormalizedRootTaskCreatePayloadV2}`，其中`schema_version=1` required且参与JCS/hash；Envelope context不属于payload，只在projection出现一次。仅规范化source URI与Scope patterns。root receipt/idempotency各有独立v2 fixture，不能复用legacy或child fixture。

`TaskCreateResponseV2.task`直接引用当前active retained `https://schemas.shittim.local/v1/task/task_spec.json`；这不使TaskSpec legacy，也不创建TaskSpec v2。

本批12项source依赖不靠实现猜测：Projection是`task→common`并引用normalized payload；TaskCreate Request/Response是`kcp→task/common`；两个Envelope是`kcp→common`且无method payload refs；两个allocation无refs。完整逐Schema表与absolute/local fragment规则见IC §13.6。

单事务创建Origin v2、Scope、root Task、Provenance、Audit v2与一个`task.created` EventEnvelope v2；使用task component `kind=object`、自身`schema_version=2`的`RootTaskCreateAllocationV2`逐purpose分配**七个**UUID；schema_version不计入数量。opaque只要求non-empty，不规定hex。UUID互异和opaque独立由producer/conformance验证，Schema不伪装能表达。所有时间来自唯一accepted_at，commit前canonical readback。legacy v1六UUID allocation不属于active root v2。

## Child Action

固定capability/operation/class：`kernel.task` / `task.child.create` / S1。proposal完整显式声明child facts、Scope、Delegation与Origin input，禁止child ID/parent/status/revision/time/shadow字段。父Task只取Action.task_id。

proposal的来源类型是`InputContentOriginV1`，scope类型是`InputTaskScopeV1`，stored事实分别是`ContentOriginV2`与TaskScope；禁止混名。Kernel构造并hash：

- `NormalizedChildTaskProposalV1`；
- `ChildTaskDeltaProjectionV1`；
- `MaterialAuthorizationProjectionV1`；
- `ObservationEvidenceProjectionV1`。

执行前验证Action revision/status、current PD/可消费Approval、Lease holder/generation/expiry、Stop Fence、Delegation authority与proposal引用。

### 原子bundle

同一事务创建Origin/Scope/Task/Provenance/Verification/Audit、child `task.created`、Action `action.state_changed`，并更新Action completed result/revision。使用task component `kind=object`、自身`schema_version=1`的`ChildTaskMaterializationAllocationV1`；另有十UUID与三个non-empty opaque值，schema_version不计数。跨字段UUID互异/外部ID不等/opaque独立由producer与Conformance闭合，Schema不规定hex。Action event因果是正式`ActionTransitionRefV1`并先持久化`ActionTransitionIntentV1`，禁止self-causation。Action ID是child-by-action唯一业务键，跨generation最多一个child。

同Action同proposal/material hash且bundle完整为合法重放；同Action不同proposal/material为`child_materialization_conflict`；同execution idempotency key绑定不同Action为`idempotency_conflict`；mapping不完整为`stored_data_invalid`，禁止补半包。

## Repository硬门

Action、PermissionDecision、Approval、Identity、ActionTransition、RootCreate、ChildMaterialization使用IC规范闭集API，不暴露SQL/transaction。Approval三种CAS操作都必须消费`ApprovalEventAllocationV1`；Challenge读取只读，过期由`expire_challenge_with_expected_state`在resolve/consume事务中CAS持久化为expired并写identity/security Audit，绝不写`approval.state_changed`。必要unique facts和PD↔Approval↔Action一致性以IC §6.10.6为准。

读取固定：Schema版本验证→JCS byte equality→typed tagged-union decode→关系镜像/唯一键→current refs→重算projection hash；失败只读返回stored_data_invalid。

reconciliation只返回committed/absent/corrupt；migration先preflight+backup，写provenance/current projections，verify后再切active registration。legacy direct-child标`legacy_direct_create_v1`，不造Action/PD/Approval/Verification。

## 错误

稳定码和safe details见[Error Catalog](error-catalog.md)，覆盖scope/delegation、Action/PD/Approval、lease/fence、child uniqueness/material conflict、observation stale与stored corruption。`task.list` cursor编码仍正交未决。
