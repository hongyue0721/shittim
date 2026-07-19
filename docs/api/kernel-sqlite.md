# kernel-sqlite 内部 Rust API

`rust/crates/kernel-sqlite`是文件型SQLite持久化crate，不是KCP或外部SDK API。它当前实现migration 0001/0002、AuditRecord v1、EventEnvelope v1 Outbox、rate limit与**legacy TaskCreate v1** create/get repository。ADR-0006/0007要求的active v2 repositories尚不存在；ADR-0008规定的migration 0003/versioned unified Outbox同样只是contract-only，本页明确区分当前代码与未来合同。

## 打开与连接配置

```rust
let config = SqliteConfig::new(Duration::from_secs(5))?;
let store = SqliteStore::open("/var/lib/shittim/kernel.sqlite3", config)?;
```

- `busy_timeout` 必须显式非零。
- 只接受普通文件路径；拒绝空路径、`:memory:` 与所有以 `file:` 开头的 SQLite URI（包括普通文件 URI，不只 memory URI）。
- 每个连接设置并读取验证 `foreign_keys=ON`、`busy_timeout`。
- 初始化设置并验证 `journal_mode=WAL`；不覆盖 `synchronous`。
- migration 是内嵌、升序、SHA-256保护；当前0001/0002仍按SQL bytes checksum，0003起按ADR-0008 format-v1 descriptor单hash覆盖SQL assets与Rust transform identity/version。重复 open 幂等。
- migration ledger 必须是 binary 内嵌 migrations 的连续前缀；断档、名称/checksum/descriptor漂移均返回 `migration_drift`，高于 binary 才返回 `database_schema_too_new`。
- `schema_migrations` bootstrap 在 pending migration 事务外幂等创建；pending migration 的业务 DDL 与 ledger 行在同一 `BEGIN IMMEDIATE` 中原子提交。首次 migration 失败时可留下空 ledger 表，但不会留下该 migration 的部分业务 DDL。

## 写事务边界

```rust
store.with_write_transaction(|transaction| {
    transaction.append_audit(&audit)?;
    let event = transaction.append_event(pending_event)?;
    let rate_limits = transaction.rate_limit_port();
    // 只做数据库工作；不能在这里调用网络、Provider 或 Publisher。
    Ok(event)
})?;
```

- 使用 `BEGIN IMMEDIATE`。所有 public 业务写入口（包括 Store convenience，例如 `mark_delivered`）都必须经由此边界；只有 `COMMIT` 成功后才向调用方返回业务结果。
- closure 返回 `Ok` 才 commit，返回 `Err` 自动 rollback；panic 会先尽最大努力 rollback，释放 transaction 与连接锁后原样恢复 panic payload，因此不会因该 panic poison store mutex。调用者拿不到 commit API。
- commit 后补偿 rollback、错误 rollback 或 panic rollback 若失败，当前 `SqliteStore` 会被标记为不可继续使用；后续操作 fail closed，避免复用事务状态未知的连接。
- `WriteTransaction` 是受限表面，不公开任意 SQL；也不公开 `WriteTransaction::mark_delivered`（delivery mark 只通过 Store convenience）。
- open 路径上的 WAL journal-mode 初始化与 `schema_migrations` ledger bootstrap 是基础设施例外，不是 public 业务写 API；pending migration 仍各自使用 `BEGIN IMMEDIATE`。

## Legacy TaskCreate v1 repository

现有`WriteTransaction::create_task(TaskCreateCommand)`是仍在production构建与回归测试中的retained v1 compatibility write path：允许payload `parent_task_id`并实现direct-child，但不得由active v2 server路由。其内部prepare/type/normalization/savepoint/materialization helper均显式标记`legacy_v1`，新v2 repository只能调用`kernel-task-creation` pure API，禁止复用这些v1 helper或在当前函数上增加v2可选分支。

后续必须新增独立的root v2 create repository与child materialization repository。child repository以Action ID为唯一业务键，在同一事务写Origin/Scope/Task/provenance/Audit/Event/Verification与Action completion，canonical readback，全有或全无；不能在当前v1 create函数上增加可选分支继续维持双入口。

`SqliteStore::{get_task,get_task_scope,get_content_origin}` 返回 `Result<Option<生成类型>, StoreError>`。读取执行 parse → 正式 Schema → RFC 8785 canonical byte equality → typed decode，并逐项比较关系表 ordinal 与 JSON 数组；关联缺失、关系多/少/乱序、非 canonical JSON 或循环引用不一致均返回 `stored_data_invalid`，不会修复存储。

migration 0002 以 `record_json` 为 Task/TaskScope/ContentOrigin 唯一事实，STORED generated columns 只做唯一键、索引与 FK 投影；Rust 不双写业务列。TaskScope↔Task 使用 deferred FK 闭合一对一首版关系。ContentOrigin 和 parent refs 创建后不可更新/删除；parent refs 还有 no-late-insert trigger。Task/Scope 仅保护 identity/schema，保留未来 revision 更新空间。`task_scope_source_refs` 有意不加永久 immutable trigger：当前没有 Scope update API，严格读取会拒绝 out-of-band 关系篡改，未来 revision update 必须在 repository 内原子维护 JSON 与关系镜像。

## AuditRecord

`WriteTransaction::append_audit`：

1. 序列化生成的 `AuditRecord`；
2. 运行正式 AuditRecord v1 Schema；
3. 额外强制 `external_content_status=sent` 至少有 content origin、artifact、resource、model call、payload manifest 或 causation 支撑引用；
4. 生成 RFC 8785 canonical JSON；
5. 插入不可变 `audit_records.record_json`。

`SqliteStore::get_audit` 读取时重新解析、Schema 校验并反序列化生成类型。ID 使用 expression UNIQUE INDEX；type/time/task/action 使用 JSON expression index。数据库不保存完整 JSON之外的重复普通列。

当前未完成的 Audit 硬门：

- PermissionDecision 与 `policy_context.matched_rule_ref` / `policy_set_revision` 跨对象一致性；
- rollback capability 从 Action/Verification/Recovery 权威事实投影；
- Provider 与对应 ModelCall 的一致性；
- Task creation context 与同事务 TaskSpec/Task/ContentOrigin/`task.created` 的固定 canonical 子事实一致性已由 Task repository producer 校验；其它 Audit 类型仍无对应权威表；
- `system_internal` null actor 的“确无可归因主体”证明。

这些必须由拥有对应权威表的后续 repository 在同一事务中实现，不能由默认值代替。

## Event Outbox（当前legacy v1实现）

当前`PendingEvent`与模糊`append_event`只代表既有v1代码事实，不是未来active API。`PendingEvent`由上层提供 `event_id`、生成的 `EventEnvelopeType`、aggregate、时间、`CausationRef`、correlation、dedup 与 payload；不接受 sequence、outbox position 或 Envelope schema version。

`append_event` 在事务内：

- 先用 `sequence=0`、`outbox_position="1"` 的占位 Envelope 对 caller 提供的 event/payload/type/aggregate/UUID/date/refs 完成 Schema 与 typed decode 预检；占位 sequence 不代表聚合当前 sequence；
- 为单次 append 建立内部 SAVEPOINT；
- 对 `(aggregate_type, aggregate_id)` 原子分配 sequence：首条 `0`，后续连续 `+1`；
- 插入 Outbox 并分配全局 AUTOINCREMENT position；
- 从规范化列事实构造最终 EventEnvelope JSON，再调用 `validate_json` 和 `TypedEventEnvelope::decode`；
- 任一步失败由该 append 自行 `ROLLBACK TO` 并释放 SAVEPOINT。即使调用者捕获该错误后让外层事务 closure 返回 `Ok`，也不会提交该次 append 的 sequence、position 或部分行；同一事务中先前成功 append 保留且后续不产生空洞。

Outbox 不保存 `envelope_json` 双源；payload 使用 canonical JSON 存储。当前migration 0001仍以`causation_kind/causation_id`保存v1两支，不能被描述为v2可用。

## 未来migration 0003：统一v1/v2 Outbox合同（contract-only）

ADR-0008要求在同一`outbox`表内支持mixed v1/v2，保留唯一全局position与`aggregate_event_sequences`；禁止建立`outbox_v2`或第二套cursor/sequence。规范化列保留event/type/envelope version/aggregate/sequence/time/correlation/dedup/payload/delivery；v1的`causation_kind + causation_id`迁移为单个RFC 8785 JCS `causation_json`，避免四branch nullable列膨胀。`payload_json`继续JCS；仍不保存完整Envelope JSON。

当前migration代码的ledger精确为`schema_migrations(version,name,checksum,applied_at)`，0001/0002 checksum为SQL原始bytes的SHA-256。0003必须在其单一`BEGIN IMMEDIATE`内先ALTER ledger增加`descriptor_hash`与`descriptor_format_version`（历史行nullable，新行分别64位lowercase hex/固定1），再运行Outbox transform并写0003 row。descriptor format v1固定JCS bytes，列出按path排序的SQL assets及其raw-byte hash，并携带闭集Rust transform `algorithm_id/version/implementation_id`；通用模板中`algorithm_id`必须是具体stable transform算法ID，不能写migration-transform族名。0003 identity精确为version `3`、name `versioned_event_outbox`、唯一asset `rust/crates/kernel-sqlite/migrations/0003_versioned_event_outbox.sql`（单asset包含ledger ALTER、replacement table、indexes、constraints及table-swap DDL）与transform三元组`shittim.kernel-sqlite.outbox-v1-to-versioned-v1`/`1`/`kernel_sqlite::migration::outbox_v1_to_versioned_v1`；Rust transform同transaction执行。`checksum`和`descriptor_hash`对0003+均等于该单一descriptor hash。0001/0002继续按旧SQL-only算法且descriptor列null验证；同version name/asset/algorithm/version/implementation漂移、未知descriptor三元组、半填ledger均`migration_drift`，不能重跑或静默接受。

0003必须在同一事务创建replacement table，按原position读取并以exact v1 Schema/typed decode验证legacy row，用共享JCS authority写CausationRef v1 JSON，显式保留position/sequence/delivered/dedup/payload，再核对row count、逐rowcanonical readback、每aggregate `0..N`连续性、sequence table、MAX position和下一AUTOINCREMENT位置，最后原子换表/索引并写ledger。SQLite `json_object`不能冒充JCS。post-0003 DB CHECK至少固定schema_version×event_type×aggregate：v1只允许原三类，v2允许五类；task两类→task、action→action、approval→approval_chain、stop fence→stop_fence/global。两个JSON列DB只做`json_valid`+root object；JCS、UUID/date-time、causation branch、payload版本/shape仍由应用层exact Schema与typed readback负责。

任一corrupt row、descriptor/checksum/copy/readback/count/sequence失败使ledger ALTER、DDL、data copy与0003 row整体回滚；rollback状态未知则store unhealthy。成功升级后旧binary必须返回`database_schema_too_new`；版本回滚只能恢复迁移前验证备份，不能把v2 row降解为v1。

未来公开mixed API形态固定为`StoredEventEnvelope::{LegacyV1(TypedEventEnvelope),ActiveV2(TypedEventEnvelopeV2)}`和`OutboxRecord { envelope, delivered_at }`。这些public类型固定由`kernel-sqlite/src/outbox.rs`拥有并从crate root re-export。`WriteTransaction`只公开`append_legacy_event_v1(PendingLegacyEventV1)`与`append_active_event_v2(PendingActiveEventV2)`，共用私有`append_versioned_event`；旧`PendingEvent/append_event`无alias。

`PendingLegacyEventV1`逐字段保留当前PendingEvent的`event_id,event_type,aggregate_type,aggregate_id,occurred_at,causation_ref,correlation_id,dedup_key,payload`，继续由v1 Schema/typed decode验证caller type/aggregate/payload mapping。active API新增`EventAggregateId::{Task(Uuid),Action(Uuid),ApprovalChain(Uuid),StopFenceGlobal}`；`PendingActiveEventV2`精确字段为`event_id:Uuid,aggregate_id:EventAggregateId,occurred_at:DateTime<Utc>,causation_ref:CausationRefV2,correlation_id:String,dedup_key:String,payload:EventEnvelopeV2Payload`。它不接受caller event_type/aggregate_type/schema_version/sequence/position；store从generated payload variant与`EVENT_ACTIVE_BINDINGS`派生type/aggregate，并在分配sequence前验证aggregate enum variant与payload ID exact匹配，correlation/dedup非空。unknown/corrupt version/type/JCS/mapping返回`stored_data_invalid`且不推进cursor。`mark_delivered`写前必须验证目标row仍可完整读取，不能用delivery mark隐藏corruption。

## Cursor 与 Publisher 存储面

- `OutboxPosition`：`i64 > 0`。
- `OutboxCursor`：`i64 >= 0`，只解析 ASCII 十进制；拒绝符号、空格、空字符串和溢出，**继续接受前导零并以普通十进制输出**。这是retained v1 cursor兼容行为，本Event v2/0003批次不收紧；若未来v2 wire要求canonical cursor，必须另立版本化协议规则，不能通过此次migration改变既有输入接受集。
- EventEnvelope `outbox_position`：v1 source当前pattern为`^[0-9]+$`，因此历史typed read保持接受前导零；v2本批同样不额外收紧，DB重建始终从整数position输出无前导零的canonical普通十进制。
- `PageLimit`：`1..=500`。
- `read_after`：严格 `position > cursor`、升序，包含已 delivered 历史。
- `latest_position`：空 Outbox 返回 `None`。
- `read_undelivered`：Publisher 按位置读取未投递记录。
- `mark_delivered`：Store convenience，委托统一 `with_write_transaction` / `BEGIN IMMEDIATE`；返回 `Marked | AlreadyMarked | NotFound`，且仅在 `COMMIT` 成功后返回。SQL 使用 conditional `UPDATE ... delivered_at IS NULL` 与同事务 existence `SELECT`；第一次时间不可覆盖。并发争同一 position 时恰好一个 `Marked`、其余 `AlreadyMarked`，DB 时间等于 winner 传入值。writer contention 映射 `sqlite_busy`，释放后可重试。外层主动 `Err` 或 panic 会回滚 helper 内的 mark，公共重试可再次 `Marked`。unhealthy store 上 public mark fail closed，其它健康 store 可见未改变。

读取和真实外部发布不在同一写事务。未调用 `mark_delivered` 前，同一 cursor 的重复 `read_undelivered` 以及进程重启后的读取都会再次返回同一事件，提供 at-least-once 存储语义；mark 后它从未投递读取消失，但 `read_after` 历史仍保留。没有 Publisher 循环、删除、retention、claim lease 或订阅者确认状态。commit/rollback 故障注入若无法无侵入测试，由统一写事务既有 panic/error rollback 与 unhealthy fail-closed 测试覆盖，不新增生产 seam。

注意：retained KCP `EventPollResponse v1`的events item exact为EventEnvelope v1，不能返回mixed read中的v2。统一Outbox不等于当前`event.poll`可用；v1 poll若遇到下一条v2不得跳过/降级/推进cursor，未来必须另做versioned response Schema、binding和handler切片。

## Transaction-bound RateLimitPort

`WriteTransaction::rate_limit_port()` 返回只在当前写事务生命周期内有效的 `RateLimitPort`：

- `preview` 只计数、不消费；
- `check_and_consume` 在同一 `BEGIN IMMEDIATE` 事务重新计数并写入 winner 的消费；
- 窗口为 `consumed_at_micros > instant - window`，边界记录不计入；
- 同一微秒允许多个 slot；
- rule revision 和 rate key 分别隔离；
- 消费记录当前不清理，因为时钟非单调时的安全清理契约尚未定义。

`SqliteStore` 本身不实现 `RateLimitPort`，防止独立 commit 破坏 PermissionDecision 同事务语义。

## Typed handler 映射提示

SQLite adapter 必须按 `StoreErrorCode` 穷举，不匹配 `StoreError.message`。三个 typed handler 公开 `invalid_scope_pattern`、`idempotency_conflict`、三个引用 not-found、`sqlite_busy/full/corrupt`、`stored_data_invalid`；constraint/contract/serialization/not-found/internal 以及 open/config/migration/schema 类折叠为安全 `internal_error`。固定 message/retryable 见 [error-catalog.md](error-catalog.md)。`SqliteBusy` 可重试，其余 storage/internal 映射不可重试。

## 错误

`StoreError` 保留稳定 `StoreErrorCode`，包括：

- `invalid_database_path`
- `sqlite_open_failed`
- `sqlite_configuration_failed`
- `sqlite_busy`
- `sqlite_full`
- `sqlite_corrupt`
- `migration_failed`
- `migration_drift`
- `database_schema_too_new`
- `constraint_violation`
- `serialization_failed`
- `contract_invalid`
- `invalid_scope_pattern`
- `idempotency_conflict`
- `delegation_not_found`
- `parent_task_not_found`（仅legacy TaskCreate v1 repository/handler；active v2 root create与child Action不使用此错误）
- `parent_origin_not_found`
- `stored_data_invalid`
- `invalid_cursor`
- `not_found`
- `internal_store_error`

错误消息不包含 SQL 文本、参数或 Audit/Event payload。

## 明确不在当前实现范围

active TaskCreate v2、Child Action materialization、Task creation provenance、CausationRef/EventEnvelope/ContentOrigin/Audit v2、Action/PermissionDecision/Approval repositories与migration均未实现。Event v2八Schema与migration 0003已有权威文档合同，但没有SQL、Rust或测试实现。
