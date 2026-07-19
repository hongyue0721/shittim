# kernel-sqlite 内部 Rust API

`rust/crates/kernel-sqlite`是文件型SQLite持久化crate，不是KCP或外部SDK API。它当前实现migration、AuditRecord v1、EventEnvelope v1 Outbox、rate limit与**legacy TaskCreate v1** create/get repository。ADR-0006/0007要求的active v2 repositories尚不存在。

## 打开与连接配置

```rust
let config = SqliteConfig::new(Duration::from_secs(5))?;
let store = SqliteStore::open("/var/lib/shittim/kernel.sqlite3", config)?;
```

- `busy_timeout` 必须显式非零。
- 只接受普通文件路径；拒绝空路径、`:memory:` 与所有以 `file:` 开头的 SQLite URI（包括普通文件 URI，不只 memory URI）。
- 每个连接设置并读取验证 `foreign_keys=ON`、`busy_timeout`。
- 初始化设置并验证 `journal_mode=WAL`；不覆盖 `synchronous`。
- migration 是内嵌、升序、SHA-256 checksum 保护的 SQL；重复 open 幂等。
- migration ledger 必须是 binary 内嵌 migrations 的连续前缀；只有 v2、断档、名称/checksum 漂移均返回 `migration_drift`，高于 binary 才返回 `database_schema_too_new`。
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

## Event Outbox

`PendingEvent` 由上层提供 `event_id`、生成的 `EventEnvelopeType`、aggregate、时间、`CausationRef`、correlation、dedup 与 payload；不接受 sequence、outbox position 或 Envelope schema version。

`append_event` 在事务内：

- 先用 `sequence=0`、`outbox_position="1"` 的占位 Envelope 对 caller 提供的 event/payload/type/aggregate/UUID/date/refs 完成 Schema 与 typed decode 预检；占位 sequence 不代表聚合当前 sequence；
- 为单次 append 建立内部 SAVEPOINT；
- 对 `(aggregate_type, aggregate_id)` 原子分配 sequence：首条 `0`，后续连续 `+1`；
- 插入 Outbox 并分配全局 AUTOINCREMENT position；
- 从规范化列事实构造最终 EventEnvelope JSON，再调用 `validate_json` 和 `TypedEventEnvelope::decode`；
- 任一步失败由该 append 自行 `ROLLBACK TO` 并释放 SAVEPOINT。即使调用者捕获该错误后让外层事务 closure 返回 `Ok`，也不会提交该次 append 的 sequence、position 或部分行；同一事务中先前成功 append 保留且后续不产生空洞。

Outbox 不保存 `envelope_json` 双源；payload 使用 canonical JSON 存储。

## Cursor 与 Publisher 存储面

- `OutboxPosition`：`i64 > 0`。
- `OutboxCursor`：`i64 >= 0`，只解析 ASCII 十进制；拒绝符号、空格、空字符串和溢出，接受前导零并以普通十进制输出。
- `PageLimit`：`1..=500`。
- `read_after`：严格 `position > cursor`、升序，包含已 delivered 历史。
- `latest_position`：空 Outbox 返回 `None`。
- `read_undelivered`：Publisher 按位置读取未投递记录。
- `mark_delivered`：Store convenience，委托统一 `with_write_transaction` / `BEGIN IMMEDIATE`；返回 `Marked | AlreadyMarked | NotFound`，且仅在 `COMMIT` 成功后返回。SQL 使用 conditional `UPDATE ... delivered_at IS NULL` 与同事务 existence `SELECT`；第一次时间不可覆盖。并发争同一 position 时恰好一个 `Marked`、其余 `AlreadyMarked`，DB 时间等于 winner 传入值。writer contention 映射 `sqlite_busy`，释放后可重试。外层主动 `Err` 或 panic 会回滚 helper 内的 mark，公共重试可再次 `Marked`。unhealthy store 上 public mark fail closed，其它健康 store 可见未改变。

读取和真实外部发布不在同一写事务。未调用 `mark_delivered` 前，同一 cursor 的重复 `read_undelivered` 以及进程重启后的读取都会再次返回同一事件，提供 at-least-once 存储语义；mark 后它从未投递读取消失，但 `read_after` 历史仍保留。没有 Publisher 循环、删除、retention、claim lease 或订阅者确认状态。commit/rollback 故障注入若无法无侵入测试，由统一写事务既有 panic/error rollback 与 unhealthy fail-closed 测试覆盖，不新增生产 seam。

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

active TaskCreate v2、Child Action materialization、Task creation provenance、CausationRef/EventEnvelope/ContentOrigin/Audit v2、Action/PermissionDecision/Approval repositories与migration均未实现。
