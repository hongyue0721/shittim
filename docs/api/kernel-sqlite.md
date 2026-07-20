# kernel-sqlite 内部 Rust API

`rust/crates/kernel-sqlite`是文件型SQLite持久化crate，不是KCP或外部SDK API。当前实现migration 0001/0002/0003、AuditRecord v1、版本化统一Event Outbox、transaction-bound rate limit与**legacy TaskCreate v1** create/get repository（**待删除**，ADR-0009）。ADR-0006/0007要求的active业务repositories仍不存在；ADR-0008交付统一Outbox shape与mixed API（legacy append production API待删除）；不包含active business producer、Publisher或versioned KCP poll。

## 打开与migration

```rust
let config = SqliteConfig::new(Duration::from_secs(5))?;
let store = SqliteStore::open("/var/lib/shittim/kernel.sqlite3", config)?;
```

- 只接受普通文件路径；拒绝空路径、`:memory:`及`file:` URI。
- 每个连接验证`foreign_keys=ON`、显式非零`busy_timeout`与WAL。
- migration definitions分为`LegacySql`（0001/0002）与`DescriptorV1`（0003+）；所有SQL使用`include_bytes!`原始asset bytes。
- 0001/0002 ledger checksum保持SQL bytes SHA-256，descriptor列必须null。
- 0003 exact identity：version `3`、name `versioned_event_outbox`、唯一asset `rust/crates/kernel-sqlite/migrations/0003_versioned_event_outbox.sql`、transform三元组`shittim.kernel-sqlite.outbox-v1-to-versioned-v1` / `1` / `kernel_sqlite::migration::outbox_v1_to_versioned_v1`。
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

## Legacy TaskCreate v1 repository

`WriteTransaction::create_task`仍是未发布compatibility producer，现已显式调用`append_legacy_event_v1`。它不代表active root v2 producer；`V2InitialBuildActive`实现切片必须删除该写路径与`append_legacy_event_v1`/`StoredEventEnvelope::LegacyV1` production variant，不得复用legacy helper或增加可选v2分支。旧开发库open必须`reinitialize-required`，禁止自动清库/隐式升级。

## Transaction-bound rate limit

`WriteTransaction::rate_limit_port()`保持原合同：preview只读，winner-only consume在同一transaction重新计数并写入；Store本身不实现可独立commit的RateLimitPort。

## 错误

稳定`StoreErrorCode`包括配置/open、SQLite busy/full/corrupt、migration failed/drift/too-new、constraint、serialization、contract invalid、stored data invalid、cursor、not found与internal store error。错误消息不包含SQL、参数、payload或密钥。

## 明确未实现

active TaskCreate v2、Child Action materialization、Action/PermissionDecision/Approval repositories、active Event business producer、Publisher、versioned KCP poll、server/agentd、retention/claim lease仍未实现。Event v2 Schema与Outbox storage API存在不等于这些业务能力可用；Publisher/poll不在`V2InitialBuildActive`。
