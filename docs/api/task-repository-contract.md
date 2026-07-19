# Task创建、Child materialization与repository硬合同

> 状态：首批12项相关Schema source、manifest entries与generated Rust root types已落地；`InputTaskScopeV1.expires_at` pattern+format硬门、canonical timestamp API、纯crate `kernel-task-creation`、schema-tool strict RFC6901 pointer/mutation/selected CLI底座，以及三份official fixtures与harness已实现。分工是production owner负责业务hash关系、独立CLI进程/Schema路径负责中立validate/canonicalize、stored bytes/hash自一致性共享唯一JCS authority。production MethodVersionBindings仍为空；active v2 repository/handler/materializer与cutover仍未实现。现有Rust/SQLite只实现legacy TaskCreate v1 create/get，不得进入未来production server。

## Lifecycle矩阵

| path | active | legacy | 状态 |
|---|---:|---:|---|
| KCP `task.create` request | 2 | 1 validation/read | v2未实现；v1 handler已实现但须退出active dispatcher |
| KCP `task.get/list` | 1 | — | get库级已实现，list未实现 |
| child create | Kernel Action `kernel.task/task.child.create` proposal v1 | direct child command v1 read/migration | Action/PD/Approval/materializer未实现 |

## Root v2

Envelope task_id/expected_revision固定null；payload无parent。raw `TaskCreateRequestV2`、`ChildTaskProposalV1`与两份 normalized root的caller-owned字段完全同构。canonical字段合同宿主固定为既有root `NormalizedRootTaskCreatePayloadV2#/$defs`：宿主自身properties使用local fragment，另三root的九项caller字段均使用absolute fragment；`task_scope`与`origin`不得由这些root直接whole-schema引用`InputTaskScopeV1`/`InputContentOriginV1`绕过宿主，必须经宿主absolute fragment进入宿主的`$defs/task_scope`与`$defs/origin`，再分别whole-schema引用两个Input Schema。这只是中立task-create proposal字段宿主，不倒置root/child业务语义。保留四个独立root Schema身份，禁止第13个Schema或复制平行约束。`InputTaskScopeV1`是task component独立封闭对象，`InputContentOriginV1`是common component独立封闭对象。所有string数组可空、元素non-empty、保序保重复、无`uniqueItems`；非null `risk_hint`、非null `upstream_stable_id`同样non-empty，普通字符串不trim。`expires_at` source Schema现已使用`format: date-time`与完整文本pattern双门；pattern只接受大写T/Z、Z或`±HH:MM`、秒`00..59`、无fraction或全零fraction，format负责日期/时间/offset合法性，不用custom format。

`NormalizedRootTaskCreatePayloadV2`完整字段与receipt/idempotency两份preimage以IC §5.3.1为准；receipt就是该payload的JCS。root official fixture的执行分层必须保持闭合：先执行`KcpCommandEnvelopeV2` raw Schema，再执行`TaskCreateRequestV2` payload Schema，最后才调用normalization/projection/hash；该fixture不调用Value preflight、`preflight_value`、`narrow_to_registered`或dispatcher。因此auth非null在raw Schema层固定为`raw_schema_rejected`，公开投影为`public_error={code:"invalid_request",details:null}`，receipt/idempotency均为`not_computed`，且不进入payload Schema、normalization或hash；只有独立Value preflight实际执行auth判定时才返回`unsupported_auth_schema`。这两个层的错误语义不得互换。idempotency必须精确使用`RootTaskCreateIdempotencyProjectionV1 {schema_version:1,actor,entry_point,command_type:"task.create",task_id:null,context,expected_revision:null,payload:NormalizedRootTaskCreatePayloadV2}`，其中`schema_version=1` required且参与JCS/hash；Envelope context不属于payload，只在projection出现一次。只规范化非null source URI、Scope patterns/exclusions与`expires_at`：时间raw允许offset，无fraction或全零fraction合法，非零亚秒caller-invalid且不截断/四舍五入，合法值输出UTC秒。URI/source/pattern/exclusion normalization失败都公开为`invalid_scope_pattern`，details按`origin_source_uri/null`、`resource_pattern/index`、`exclusion/index`区分；post-normalize Schema/typed失败是internal contract failure。root fixture固定为`schemas/fixtures/kcp/task_create_normalized_hash.v2.json`，不能复用legacy或child fixture。

`TaskCreateResponseV2.task`直接引用当前active retained `https://schemas.shittim.local/v1/task/task_spec.json`；这不使TaskSpec legacy，也不创建TaskSpec v2。

本批12项source依赖不靠实现猜测：Projection是`task→common`并引用normalized payload；TaskCreate Request/Response是`kcp→task/common`；两个Envelope是`kcp→common`且无method payload refs；两个allocation无refs。完整逐Schema表与absolute/local fragment规则见IC §13.6。

生产纯逻辑owner已实现为crate `kernel-task-creation`。本阶段只拥有root/child proposal normalize、root receipt/idempotency projection与hash、child proposal/receipt hash、root/child allocation validation；`ChildTaskDeltaProjectionV1`、`MaterialAuthorizationProjectionV1`、`ObservationEvidenceProjectionV1`未来由专门authorization projection owner或独立切片实现。crate依赖`kernel-contracts`并调用`domain-policy`当前唯一权威URI parser/normalizer，不复制URI语义；长期抽取中立URI crate必须另立ADR。全部事实由caller typed input注入，crate不读repository、不依赖SQLite/KCP、不分配ID、不写存储。未来`kernel-sqlite`调用它；`domain-task`保持纯状态机且不增加policy依赖。当前尚未接入repository/handler。

单事务创建Origin v2、Scope、root Task、Provenance、Audit v2与一个`task.created` EventEnvelope v2；使用task component `kind=object`、自身`schema_version=2`的`RootTaskCreateAllocationV2`逐purpose分配**七个**UUID；schema_version不计入数量。opaque只要求non-empty，不规定hex。UUID内部互异、与调用方typed `RootTaskCreateExternalUuidRefsV1 {command_request_id,delegation_ref,parent_origin_refs}`互异及opaque彼此不同由`kernel-task-creation`生产validator闭合；该Rust input snapshot不是Schema/manifest对象，不接受自由UUID bag。所有时间来自唯一accepted_at，commit前canonical readback。legacy v1六UUID allocation不属于active root v2。

## Child Action

固定capability/operation/class：`kernel.task` / `task.child.create` / S1。proposal完整显式声明child facts、Scope、Delegation与Origin input，禁止child ID/parent/status/revision/time/shadow字段。父Task只取Action.task_id。

proposal的来源类型是`InputContentOriginV1`，scope类型是`InputTaskScopeV1`，stored事实分别是`ContentOriginV2`与TaskScope；禁止混名。本阶段`kernel-task-creation`只构造并hash`NormalizedChildTaskProposalV1`。`ChildTaskDeltaProjectionV1`、`MaterialAuthorizationProjectionV1`、`ObservationEvidenceProjectionV1`仍是未来authorization projection合同，由专门owner/另切片实现，不得塞进task creation crate。

执行前验证Action revision/status、current PD/可消费Approval、Lease holder/generation/expiry、Stop Fence、Delegation authority与proposal引用。

### 原子bundle

同一事务创建Origin/Scope/Task/Provenance/Verification/Audit、child `task.created`、Action `action.state_changed`，并更新Action completed result/revision。使用task component `kind=object`、自身`schema_version=1`的`ChildTaskMaterializationAllocationV1`；另有十UUID与三个non-empty opaque值，schema_version不计数。跨字段验证使用闭合typed `ChildTaskMaterializationExternalUuidRefsV1`：parent Task/Action/current PD required，Approval resolution与Delegation optional，Credential/Challenge及parent origins为Vec；verification result与action transition属于内部allocation，不进入external snapshot。

同Action同proposal/material hash且bundle完整为合法重放；同Action不同proposal/material为`child_materialization_conflict`；同execution idempotency key绑定不同Action为`idempotency_conflict`；mapping不完整为`stored_data_invalid`，禁止补半包。

## Official fixture测试制品合同

- root：`schemas/fixtures/kcp/task_create_normalized_hash.v2.json`；
- child：`schemas/fixtures/task/child_task_proposal_normalized_hash.v1.json`；
- allocations合并：`schemas/fixtures/task/task_creation_allocations.v1.json`。

精确外层字段与tamper结构见IC §5.3.1/§6.10.6。JCS只存lowercase `jcs_utf8_hex`与`sha256`，不存canonical string；allocation fixture不存JCS/hash。wrapper不得升级为business Schema或manifest entry。root tamper固定覆盖：auth非null先执行raw Envelope Schema并得到`raw_schema_rejected`，固定`public_error={code:"invalid_request",details:null}`，receipt/idempotency两hash均为`not_computed`，不进入payload Schema、normalization或hash；deadline/request/idempotency两hash不变；context只改idempotency；payload改两hash；parent拒绝；URI lexical equivalent不改hash；数组顺序/重复改hash。allocation fixture必须明确包含Schema-valid但domain拒绝的内部UUID重复、external ref碰撞与opaque重复向量，以及Schema-invalid空opaque向量；其执行顺序是Schema validation→typed decode→domain validator，Schema invalid必须短路后两层，固定`schema_valid=false/domain_result=not_evaluated`，不增加空opaque专用domain结果；不证明generator非派生性。

## Repository硬门

Action、PermissionDecision、Approval、Identity、ActionTransition、RootCreate、ChildMaterialization使用IC规范闭集API，不暴露SQL/transaction。Approval三种CAS操作都必须消费`ApprovalEventAllocationV1`；Challenge读取只读，过期由`expire_challenge_with_expected_state`在resolve/consume事务中CAS持久化为expired并写identity/security Audit，绝不写`approval.state_changed`。必要unique facts和PD↔Approval↔Action一致性以IC §6.10.6为准。

读取固定：Schema版本验证→JCS byte equality→typed tagged-union decode→关系镜像/唯一键→current refs→重算projection hash；失败只读返回stored_data_invalid。

reconciliation只返回committed/absent/corrupt；migration先preflight+backup，写provenance/current projections，verify后再切active registration。legacy direct-child标`legacy_direct_create_v1`，不造Action/PD/Approval/Verification。

## 错误

稳定码和safe details见[Error Catalog](error-catalog.md)，覆盖scope/delegation、Action/PD/Approval、lease/fence、child uniqueness/material conflict、observation stale与stored corruption。`task.list` cursor编码仍正交未决。
