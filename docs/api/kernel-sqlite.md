# kernel-sqlite 内部 Rust API

`rust/crates/kernel-sqlite`是文件型SQLite持久化crate，不是KCP或外部SDK API。当前实现migration 0001–0004、AuditRecord v1/v2、版本化统一Event Outbox、transaction-bound rate limit、**active root TaskCreate v2** repository，以及**legacy TaskCreate v1** create/get（**待删除**，ADR-0009）。ADR-0006/0007要求的Action/PermissionDecision/Approval repositories与child materializer仍不存在；ADR-0008交付统一Outbox shape与mixed API（legacy append production API待删除）；不包含KCP handler、Publisher或versioned KCP poll。

## 打开与migration

```rust
let config = SqliteConfig::new(Duration::from_secs(5))?;
let store = SqliteStore::open("/var/lib/shittim/kernel.sqlite3", config)?;
```

- 只接受普通文件路径；拒绝空路径、`:memory:`及`file:` URI。
- 每个连接验证`foreign_keys=ON`、显式非零`busy_timeout`与WAL。
- migration definitions分为`LegacySql`（0001/0002）与`DescriptorV1`（0003+）；所有SQL使用`include_bytes!`原始asset bytes。
- 0001/0002 ledger checksum保持SQL bytes SHA-256，descriptor列必须null。
- 0003 exact identity：version `3`、name `versioned_event_outbox`、唯一asset `rust/crates/kernel-sqlite/migrations/0003_versioned_event_outbox.sql`、transform三元组`shittim.kernel-sqlite.outbox-v1-to-versioned-v1` / `1` / `kernel_sqlite::migration::outbox_v1_to_versioned_v1`；phase set=`ledger_upgrade|replacement_schema|table_swap`。
- 0004 exact identity：version `4`、name `root_task_create_v2`、唯一asset `rust/crates/kernel-sqlite/migrations/0004_root_task_create_v2.sql`、transform三元组`shittim.kernel-sqlite.ddl-only-v1` / `1` / `kernel_sqlite::migration::root_task_create_v2_ddl_only_v1`；phase set=`schema`（纯DDL，无row transform）。
- descriptor format v1是UTF-8、LF、无BOM、末尾单LF的JCS对象；asset SHA覆盖原始bytes，descriptor SHA同时写入`checksum`与`descriptor_hash`，`descriptor_format_version=1`。
- ledger shape严格识别：descriptor两列必须同时存在或同时不存在；半shape、半填row、未知format/identity、hash drift均`migration_drift`。数据库version高于binary优先`database_schema_too_new`。
- 每个pending migration先`BEGIN IMMEDIATE`，锁后重新验证ledger，再执行DDL/transform/ledger insert；任一步失败整体rollback。rollback失败不会被忽略。

0003在同一事务升级ledger、创建replacement Outbox、逐row严格验证legacy v1、构造JCS `causation_json`、参数化copy、逐rowreadback、验证row count/aggregate `0..N`/sequence table/MAX position/`sqlite_sequence`，再swap表和索引。没有自动备份API或down migration；回滚binary只能恢复外部已验证备份。

## 写事务与savepoint poison

```rust
store.with_write_transaction(|transaction| {
    transaction.append_audit(&audit)?;
    let event = transaction.append_legacy_event_v1(pending)?;
    Ok(event)
})?;
```

- public业务写统一使用`BEGIN IMMEDIATE`；closure成功后只有`COMMIT`成功才返回业务结果。
- panic/error回滚；outer rollback失败将store标记unhealthy，后续fail closed。
- Task legacy create和Outbox append共用唯一transaction-bound savepoint helper。
- savepoint operation失败会`ROLLBACK TO`并`RELEASE`；release失败也尝试cleanup。
- rollback/release cleanup失败会poison outer transaction。即使caller吞掉局部错误并返回`Ok`，outer transaction仍不能commit。
- `WriteTransaction`不暴露任意SQL、commit或crate-private bypass constructor。

## Versioned unified Outbox public API

类型由`kernel-sqlite/src/outbox.rs`拥有，并从crate root re-export：

```rust
pub enum StoredEventEnvelope {
    LegacyV1(TypedEventEnvelope),
    ActiveV2(TypedEventEnvelopeV2),
}

pub struct OutboxRecord {
    pub envelope: StoredEventEnvelope,
    pub delivered_at: Option<DateTime<Utc>>,
}

pub struct PendingLegacyEventV1 { /* exact retained fields */ }
pub enum EventAggregateId { Task(Uuid), Action(Uuid), ApprovalChain(Uuid), StopFenceGlobal }
pub struct PendingActiveEventV2 { /* exact ADR-0008 fields */ }
```

公开写方法只有：

```rust
WriteTransaction::append_legacy_event_v1(PendingLegacyEventV1)
WriteTransaction::append_active_event_v2(PendingActiveEventV2)
```

旧`PendingEvent`/`append_event`已删除且无alias。两入口共用私有`append_versioned_event`：

1. 分配前exact prevalidation；
2. savepoint内对`(aggregate_type, aggregate_id)`分配连续sequence；
3. 单表insert并由AUTOINCREMENT分配position；
4. 从最终规范列调用唯一stored decoder readback。

legacy caller仍显式提供event type/aggregate/payload，并由retained v1 Envelope Schema+typed decode验证。active caller不能提供event type或aggregate type：store对`EventEnvelopeV2Payload`穷举匹配，并从`EVENT_ACTIVE_BINDINGS`派生mapping；`EventAggregateId` variant及UUID必须与payload中的task/action/approval chain ID canonical相等，Stop Fence只能是global。mismatch在sequence/position分配前失败。

## 存储shape与严格decoder

post-0003一张`outbox`表保存：position、event/type/envelope version、aggregate/sequence/time、canonical `causation_json`、correlation/dedup、canonical `payload_json`、delivery metadata。v1/v2共享唯一position和aggregate sequence。

DB CHECK固定：

- `schema_version IN (1,2)`；
- v1只允许三类retained event，v2允许五类active event；
- task/action/approval_chain/stop_fence-global mapping；
- event ID、dedup key、aggregate sequence唯一；
- payload/causation JSON valid且root object。

单一stored decoder用于migration readback、`read_after`、`read_undelivered`与`mark_delivered`写前校验。它执行：

- payload/causation parse + RFC 8785 canonical byte equality；
- exact row `schema_version`选择v1/v2 Envelope Schema和typed decoder；
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

`WriteTransaction::create_root_task_v2`是active root-only v2 write path（IC §5.5 / §6.16；ADR-0009 切片2）：

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

0004 表：`content_origins_v2`（+ parent_refs）、`task_creation_provenances`、`audit_records_v2`、`root_task_create_idempotency_v2`；canonical `record_json` 为事实源，生成列仅投影；`task_creation_provenances.task_id` 是显式列，读回强制与对应 Task 一致。Task/Scope 继续复用 retained v1 表；为允许 origin 落在 v2 表，0004 重建 `tasks`/`task_scopes`/`task_scope_source_refs` 去掉对 v1 `content_origins` 的硬 FK，但**恢复** `tasks.task_scope_ref → task_scopes` 的 deferred FK（与 `task_scopes.task_id → tasks` 成环）；origin 存在性仍由 repository 严格读回证明。公开只读：`get_content_origin_v2` / `get_audit_v2` / `get_task_creation_provenance`。

## Legacy TaskCreate v1 repository

`WriteTransaction::create_task`仍是未发布compatibility producer，现已显式调用`append_legacy_event_v1`。它不代表active root v2 producer；后续切片必须删除该写路径与`append_legacy_event_v1`/`StoredEventEnvelope::LegacyV1` production variant。旧开发库 open 的 `reinitialize-required` 策略在切片 6 落地。

## Transaction-bound rate limit

`WriteTransaction::rate_limit_port()`保持原合同：preview只读，winner-only consume在同一transaction重新计数并写入；Store本身不实现可独立commit的RateLimitPort。

## 错误

稳定`StoreErrorCode`包括配置/open、SQLite busy/full/corrupt、migration failed/drift/too-new、constraint、serialization、contract invalid、stored data invalid、cursor、not found与internal store error。错误消息不包含SQL、参数、payload或密钥。

## 明确未实现

root TaskCreate v2 repository 已落地，但 KCP method-aware preflight/handler/production bindings（切片3）、Child Action materialization、Action/PermissionDecision/Approval repositories、其它 active Event business producer、Publisher、versioned KCP poll、server/agentd、retention/claim lease仍未实现。v1 write 路径删除与旧库 reinitialize-required 在切片6。Publisher/poll不在`V2InitialBuildActive`。
