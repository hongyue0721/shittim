# kernel-sqlite 内部 Rust API

`rust/crates/kernel-sqlite`是文件型SQLite持久化crate，不是KCP或外部SDK API。当前实现 migration 0001–0007、AuditRecord **v2**、版本化 **v2-only** Event Outbox、transaction-bound rate limit、strict Task/TaskScope/ContentOrigin(v2) 读路径、**active root TaskCreate v2** repository、切片4a **Action current-snapshot / ActionTransitionIntent / `action.state_changed` producer**，以及切片4b **PolicyRuleV2 / PermissionDecisionV2 repositories + `evaluate_action_permission` 评估编排**。Legacy TaskCreate v1 write、AuditRecord v1 write、`append_legacy_event_v1` / `PendingLegacyEventV1` / `StoredEventEnvelope::LegacyV1` 已按 ADR-0009（切片3c）删除。Approval/Identity repositories与child materializer仍不存在；不包含KCP handler、Publisher或versioned KCP poll。

## 打开与migration

```rust
let config = SqliteConfig::new(Duration::from_secs(5))?;
let store = SqliteStore::open("/var/lib/shittim/kernel.sqlite3", config)?;
```

- 只接受普通文件路径；拒绝空路径、`:memory:`及`file:` URI。
- 每个连接验证`foreign_keys=ON`、显式非零`busy_timeout`与WAL。
- migration definitions分为`LegacySql`（0001/0002）与`DescriptorV1`（0003+）；所有SQL使用`include_bytes!`原始asset bytes。
- 0001/0002 ledger checksum保持SQL bytes SHA-256，descriptor列必须null。
- 0003 exact identity：version `3`、name `versioned_event_outbox`、唯一asset `rust/crates/kernel-sqlite/migrations/0003_versioned_event_outbox.sql`、transform三元组`shittim.kernel-sqlite.outbox-v1-to-versioned-v1` / `1` / `kernel_sqlite::migration::outbox_v1_to_versioned_v1`；phase set=`ledger_upgrade|replacement_schema|table_swap`。**transform 不迁移 v1 业务数据**：非空 pre-0003 Outbox 直接 `reinitialize-required`；空表仅做 shape 升级。
- 0004 exact identity：version `4`、name `root_task_create_v2`、唯一asset `rust/crates/kernel-sqlite/migrations/0004_root_task_create_v2.sql`、transform三元组`shittim.kernel-sqlite.ddl-only-v1` / `1` / `kernel_sqlite::migration::root_task_create_v2_ddl_only_v1`；phase set=`schema`（纯DDL，无row transform）。
- 0005 exact identity：version `5`、name `drop_v1_business_tables`、唯一asset `rust/crates/kernel-sqlite/migrations/0005_drop_v1_business_tables.sql`、transform三元组`shittim.kernel-sqlite.ddl-only-v1` / `1` / `kernel_sqlite::migration::drop_v1_business_tables_ddl_only_v1`；phase set=`schema`。在空表前提下 drop `content_origins`(+parent_refs)、`audit_records`、`task_create_idempotency`；非空拒绝。
- 0006 exact identity：version `6`、name `action_and_transition`、唯一asset `rust/crates/kernel-sqlite/migrations/0006_action_and_transition.sql`、transform三元组`shittim.kernel-sqlite.ddl-only-v1` / `1` / `kernel_sqlite::migration::action_and_transition_ddl_only_v1`；phase set=`schema`。创建 `actions` 与 `action_transition_intents`（canonical `record_json` + 投影列 + 双唯一键 + `committed_event_id`）。
- 0007 exact identity：version `7`、name `policy_and_permission_decision`、唯一asset `rust/crates/kernel-sqlite/migrations/0007_policy_and_permission_decision.sql`、transform三元组`shittim.kernel-sqlite.ddl-only-v1` / `1` / `kernel_sqlite::migration::policy_and_permission_decision_ddl_only_v1`；phase set=`schema`。创建 `policy_set_metadata`（bootstrap revision 0）、`policy_rules`、`permission_decisions`。
- descriptor format v1是UTF-8、LF、无BOM、末尾单LF的JCS对象；asset SHA覆盖原始bytes，descriptor SHA同时写入`checksum`与`descriptor_hash`，`descriptor_format_version=1`。
- ledger shape严格识别：descriptor两列必须同时存在或同时不存在；半shape、半填row、未知format/identity、hash drift均`migration_drift`。数据库version高于binary优先`database_schema_too_new`。
- 每个pending migration先`BEGIN IMMEDIATE`，锁后重新验证ledger，再执行DDL/transform/ledger insert；任一步失败整体rollback。rollback失败不会被忽略。
- **open 后** `reject_legacy_v1_business_data`：若仍存在 `outbox.schema_version=1` 行，或 `content_origins` / `audit_records` / `task_create_idempotency` 非空（表尚在时），返回稳定 `StoreErrorCode::StoredDataInvalid`，message 含 `reinitialize-required:` 前缀。禁止自动清库/隐式升级。

0001–0004 asset bytes 与 descriptor identity 保持稳定。没有自动备份API或down migration；操作员只能外部备份后删除/重建数据库文件。

## 写事务与savepoint poison

```rust
store.with_write_transaction(|transaction| {
    let event = transaction.append_active_event_v2(pending)?;
    Ok(event)
})?;
```

- public业务写统一使用`BEGIN IMMEDIATE`；closure成功后只有`COMMIT`成功才返回业务结果。
- panic/error回滚；outer rollback失败将store标记unhealthy，后续fail closed。
- Outbox append 使用唯一 transaction-bound savepoint helper。
- savepoint operation失败会`ROLLBACK TO`并`RELEASE`；release失败也尝试cleanup。
- rollback/release cleanup失败会poison outer transaction。即使caller吞掉局部错误并返回`Ok`，outer transaction仍不能commit。
- `WriteTransaction`不暴露任意SQL、commit或crate-private bypass constructor。

## Versioned unified Outbox public API（v2-only）

类型由`kernel-sqlite/src/outbox.rs`拥有，并从crate root re-export：

```rust
pub enum StoredEventEnvelope {
    ActiveV2(TypedEventEnvelopeV2),
}

pub struct OutboxRecord {
    pub envelope: StoredEventEnvelope,
    pub delivered_at: Option<DateTime<Utc>>,
}

pub enum EventAggregateId { Task(Uuid), Action(Uuid), ApprovalChain(Uuid), StopFenceGlobal }
pub struct PendingActiveEventV2 { /* exact ADR-0008 fields */ }
```

公开写方法只有：

```rust
WriteTransaction::append_active_event_v2(PendingActiveEventV2)
```

`PendingLegacyEventV1` / `append_legacy_event_v1` / `StoredEventEnvelope::LegacyV1` 已删除且无 alias。append 内核：

1. 分配前exact prevalidation；
2. savepoint内对`(aggregate_type, aggregate_id)`分配连续sequence；
3. 单表insert并由AUTOINCREMENT分配position；
4. 从最终规范列调用唯一stored decoder readback。

active caller不能提供event type或aggregate type：store对`EventEnvelopeV2Payload`穷举匹配，并从`EVENT_ACTIVE_BINDINGS`派生mapping；`EventAggregateId` variant及UUID必须与payload中的task/action/approval chain ID canonical相等，Stop Fence只能是global。mismatch在sequence/position分配前失败。

## 存储shape与严格decoder

post-0003一张`outbox`表保存：position、event/type/envelope version、aggregate/sequence/time、canonical `causation_json`、correlation/dedup、canonical `payload_json`、delivery metadata。

DB CHECK 仍允许 `schema_version IN (1,2)`（0003 asset 字节稳定）；**production decoder 与 open 守卫只接受 v2**：

- stored decoder 对 `schema_version != 2` 一律 `stored_data_invalid`；
- open 对 `schema_version=1` 行返回 `reinitialize-required`；
- task/action/approval_chain/stop_fence-global mapping；
- event ID、dedup key、aggregate sequence唯一；
- payload/causation JSON valid且root object。

单一stored decoder用于`read_after`、`read_undelivered`与`mark_delivered`写前校验。它执行：

- payload/causation parse + RFC 8785 canonical byte equality；
- exact row `schema_version=2` 选择 v2 Envelope Schema 和 typed decoder；
- type/aggregate/payload relation及payload aggregate ID相等；
- timestamp与delivery timestamp解析。

stored corruption统一返回`stored_data_invalid`；caller invalid仍返回`contract_invalid`/`serialization_failed`。分页中任一row损坏使整页失败，不返回partial page。`mark_delivered`先完整decode目标row再conditional update，不能用mark隐藏损坏。

## Cursor与delivery

- `OutboxPosition`: `i64 > 0`。
- `OutboxCursor`: `i64 >= 0`，只接受ASCII十进制；为retained兼容继续接受前导零，输出普通十进制。
- `PageLimit`: `1..=500`。
- `read_after`: `position > cursor`，升序，包含delivered历史。
- `read_undelivered`: 同一position流中的未投递记录。
- `latest_position`: 空表返回`None`。
- `mark_delivered`: `Marked | AlreadyMarked | NotFound`；第一次时间不可覆盖，只有outer commit后返回。

crate没有Publisher loop、claim lease、retention、删除或订阅者ack状态。retained KCP `EventPollResponse v1`仍只能承载Envelope v1；遇到v2不得跳过、降级或推进cursor，未来必须独立升级response/binding/handler。

## Active root TaskCreate v2 repository

`WriteTransaction::create_root_task_v2`是active root-only v2 write path（IC §5.5 / §6.16；ADR-0009）：

```rust
pub struct RootTaskCreateV2Command {
    pub envelope: RootTaskCreateV2EnvelopeFacts, // actor, entry_point, request_id, context, idempotency_key
    pub request: TaskCreateRequestV2,
    pub allocation: RootTaskCreateAllocationV2,  // 七 UUID + correlation/dedup
    pub accepted_at: DateTime<Utc>,              // 调用方注入的第一次时钟读取；repository 不读时钟
}

pub enum CreateRootTaskV2Result {
    Created { task: TaskSpec, creation_provenance_ref: String },
    Replayed { task: TaskSpec, creation_provenance_ref: String },
}
```

流程：`kernel-task-creation` normalize → receipt/idempotency projection + hash → allocation validate → 统一 savepoint 内按固定顺序写入业务事实 `ContentOriginV2`（+ parent_refs）、TaskScope v1（+ source_refs）、TaskSpec v1（`parent_task_id=null`）、`TaskCreationProvenanceV1(root_command_v2)`、`AuditRecordV2(task.creation_recorded)` → 再写 `root_task_create_idempotency_v2` → 最后 `append_active_event_v2(task.created)` → 与 Created 等价的全闭包 canonical readback（Task/Origin/Scope/Provenance↔task 列交叉/Audit/Outbox Event/idempotency 映射）。同 scope 四元组 + 同 hash 重放执行同一闭包读回并返回已存 Task（不产生新 Event）；异 hash 返回 `idempotency_conflict`。任一失败整体回滚不占号。

0004 表：`content_origins_v2`（+ parent_refs）、`task_creation_provenances`、`audit_records_v2`、`root_task_create_idempotency_v2`；canonical `record_json` 为事实源，生成列仅投影。Task/Scope 继续复用 retained v1 **shape** 表。0005 删除 dead v1 业务表 `content_origins` / `audit_records` / `task_create_idempotency`。公开只读：`get_task` / `get_task_scope` / `get_content_origin_v2` / `get_audit_v2` / `get_task_creation_provenance`。legacy `get_content_origin` 已从公开 API 移除（crate 私有只读，0005 后恒 `None`）。

## Action current-snapshot + ActionTransitionIntent（切片4a）

```rust
// Action closed subset implemented in this slice (public):
WriteTransaction::insert_pending_action(InsertPendingActionCommand) -> ActionRequestV2
SqliteStore::get_action(id) -> Option<ActionRequestV2>
// crate-private internal CAS helper only (no Outbox; not a dual status-event authority):
// WriteTransaction::transition_with_expected_revision(TransitionActionCommand) -> ActionRequestV2

// ActionTransitionIntent closed set (IC §6.14) — sole public authority for status events:
WriteTransaction::insert_intent(ActionTransitionIntentV1) -> InsertIntentResult
SqliteStore::get_intent(transition_id) -> Option<ActionTransitionIntentV1>
SqliteStore::get_for_action_revision(...) -> Option<ActionTransitionIntentV1>
WriteTransaction::mark_committed_with_event(MarkCommittedCommand) -> (ActionRequestV2, OutboxRecord)
SqliteStore::reconcile_intent(transition_id) -> ReconcileIntentResult // prepared|committed|corrupt
```

- pending insert：`status=pending`、`revision=1`、`permission_decision_ref`/`approval_chain_id`/`result`/`lease` 为 null；owning Task 必须存在；canonical JCS readback。
- `transition_with_expected_revision`：**crate-private** 内部 CAS helper；expected revision+status CAS；边合法性与 evidence 不变量委托 `domain-task`；**不写 Outbox**。会发状态事件的边必须以 `mark_committed_with_event` 为唯一权威，禁止双写路径。
- **需 effects 的边当前 fail closed**：domain outcome 要求 lease/lock release（如 `leased → approved|cancelled|unknown_side_effect`）时，在 lease API 落地前明确拒绝，禁止静默半提交。
- `insert_intent`：`transition_id` 与业务六元组双唯一键；同事实重放返回原 intent；非法边 fail closed。
- `mark_committed_with_event`：同一 savepoint 内先经 `domain-task::apply_action_transition` 做完整 evidence 门（PD/verification/dispatch_certainty 等；intent 只作 anchor+唯一键），再 CAS Action + `append_active_event_v2(action.state_changed)`（payload 从 commit 后 Action+intent+`ActionEventIntent` 投影；causation 精确 `action_transition`）+ 回写 `committed_event_id`；失败不占 sequence/position。同 event id 重放幂等，只验 intent↔event 链路，不要求 Action head 仍停在该 revision。
- `reconcile_intent`：只观察 stored 关系，返回 `prepared|committed|corrupt`，不补造 event 或更换 transition id。Committed 只对比 intent↔outbox event 快照字段；后续合法推进不得映射 Corrupt。
- 本片**未实现** IC §6.10.6 Action 闭集中的 lease/policy-binding/child-completion/recovery-list 方法。

## PolicyRuleV2 + PermissionDecisionV2 + 评估编排（切片4b）

```rust
// PolicyRule closed write/read
WriteTransaction::append_policy_rule_revision(PolicyRuleV2) -> PolicyRuleMutationResult
SqliteStore::get_policy_rule_revision(rule_id, revision) -> Option<PolicyRuleV2>
SqliteStore::get_current_policy_rule(rule_id) -> Option<PolicyRuleV2>
SqliteStore::list_current_policy_rules() -> Vec<PolicyRuleV2>
SqliteStore::get_policy_set_revision() -> i64 // bootstrap 0 = empty set

// PermissionDecision closed set (IC §6.10.6 subset; no update/delete)
WriteTransaction::append_permission_decision(PermissionDecisionV2) -> PermissionDecisionV2
// decision_revision: 0 placeholder or exact next; repository allocates max(action)+1
SqliteStore::get_permission_decision(id) -> Option<PermissionDecisionV2>
SqliteStore::get_current_permission_decision_for_action(action_id) -> Option<PermissionDecisionV2>
SqliteStore::list_permission_decisions_for_action(action_id) -> Vec<PermissionDecisionV2>
SqliteStore::validate_current_permission_decision_for_action(action_id)
    -> Option<PermissionDecisionV2> // Action.permission_decision_ref ↔ PD current

// Evaluation orchestration (single savepoint; no Approval creation)
WriteTransaction::evaluate_action_permission(EvaluateActionPermissionCommand)
    -> EvaluateActionPermissionResult // PD + Action CAS + permission.evaluated Audit
```

- **PolicySet**：空初始 revision=0 是权威空状态；每次成功 rule mutation 同事务 +1。
- **PolicyRule**：append-only revision 历史；current head = MAX(revision) per id；disable 写新 revision `enabled=false`；物理 delete 禁止。
- **PermissionDecision**：immutable append；`decision_revision` 连续；canonical JCS readback；断号/id 冲突 fail closed。
- **评估编排不变量**：pending Action + expected revision CAS；可选 TaskScope containment；enabled v2 heads → domain-policy matcher（`remote_signature` 规则不进 v1 matcher，fail closed 不生效直到 matcher 升级 v2）；`TransactionRateLimitPort` winner-only 消费；`kernel-authorization` 真实重算 material/observation 指纹（禁用 evaluation_context_hash；material preimage 的 policy_set_revision 与 PD 存储值一致，空 set 为 0，共享 `material_policy_set_revision_for_projection`）；append PD + Audit `permission.evaluated`（policy_context 与 PD 字段一致）；allow/deny 状态边经 intent + `mark_committed_with_event` 发 `action.state_changed`（唯一权威，reason_code=policy_allow|policy_deny），confirm 为 metadata CAS 不发事件；confirm deferral 绑定真实 PD，无 approval 伪造（`approval_chain_id` null，Approval 属 4c）；失败整体回滚。
- 本片**未实现** Approval/Identity repositories、Approval 创建、Action lease/policy-binding 闭集其余方法。

## Transaction-bound rate limit

`WriteTransaction::rate_limit_port()`保持原合同：preview只读，winner-only consume在同一transaction重新计数并写入；Store本身不实现可独立commit的RateLimitPort。评估编排在同一事务内注入该 port。

## 错误

稳定`StoreErrorCode`包括配置/open、SQLite busy/full/corrupt、migration failed/drift/too-new、constraint、serialization、contract invalid、stored data invalid、cursor、not found与internal store error。**旧库 reinitialize-required 使用既有闭集 `StoredDataInvalid`**（`as_str() = "stored_data_invalid"`），message 稳定前缀 `reinitialize-required:`，不泄漏 SQL/payload。错误消息不包含SQL、参数、payload或密钥。

## 明确未实现

root TaskCreate v2 repository + kcp method-aware runtime（切片3a/3b）+ v1 write 删除与旧库拒绝（切片3c）+ Action/transition/`action.state_changed`（切片4a）+ PolicyRule/PD/评估编排（切片4b）已落地。Child Action materialization、Approval/Identity repositories、Action lease/policy-binding/child-completion 写方法、approval/child active Event business producer、Publisher、versioned KCP poll、server/agentd、retention/claim lease仍未实现。Publisher/poll不在`V2InitialBuildActive`。§13.7 完整闭合仍需切片4c–5（Approval/Identity + child materializer）。
