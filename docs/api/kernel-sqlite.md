# kernel-sqlite 内部 Rust API

`rust/crates/kernel-sqlite` 是文件型 SQLite 基座，不是 KCP 或外部 SDK API。它实现 migration、不可变 AuditRecord、原子 Event Outbox、cursor/delivery 和 transaction-bound Policy rate limit。

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

- 使用 `BEGIN IMMEDIATE`。
- closure 返回 `Ok` 才 commit，返回 `Err` 自动 rollback；panic 会先尽最大努力 rollback，释放 transaction 与连接锁后原样恢复 panic payload，因此不会因该 panic poison store mutex。调用者拿不到 commit API。
- commit 后补偿 rollback、错误 rollback 或 panic rollback 若失败，当前 `SqliteStore` 会被标记为不可继续使用；后续操作 fail closed，避免复用事务状态未知的连接。
- `WriteTransaction` 是受限表面，不公开任意 SQL。
- 后续 Task/Action/PermissionDecision repository 可复用同一事务，但本 crate 当前没有实现这些 repository。

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
- task creation context 与同事务 TaskSpec/Task 的一致性；
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
- `mark_delivered`：返回 `Marked | AlreadyMarked | NotFound`；第一次时间不可覆盖。

读取和真实外部发布不在同一写事务。未调用 `mark_delivered` 前，同一 cursor 的重复 `read_undelivered` 以及进程重启后的读取都会再次返回同一事件，提供 at-least-once 存储语义；mark 后它从未投递读取消失，但 `read_after` 历史仍保留。没有 Publisher 循环、删除、retention、claim lease 或订阅者确认状态。

## Transaction-bound RateLimitPort

`WriteTransaction::rate_limit_port()` 返回只在当前写事务生命周期内有效的 `RateLimitPort`：

- `preview` 只计数、不消费；
- `check_and_consume` 在同一 `BEGIN IMMEDIATE` 事务重新计数并写入 winner 的消费；
- 窗口为 `consumed_at_micros > instant - window`，边界记录不计入；
- 同一微秒允许多个 slot；
- rule revision 和 rate key 分别隔离；
- 消费记录当前不清理，因为时钟非单调时的安全清理契约尚未定义。

`SqliteStore` 本身不实现 `RateLimitPort`，防止独立 commit 破坏 PermissionDecision 同事务语义。

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
- `invalid_cursor`
- `not_found`
- `internal_store_error`

错误消息不包含 SQL 文本、参数或 Audit/Event payload。

## 明确不在范围

本 crate 没有 Task、Action、PermissionDecision repository，没有 KCP、`agentd`、网络、Provider 调用或 Publisher 后台循环。
