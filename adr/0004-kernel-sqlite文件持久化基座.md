# ADR-0004：Kernel SQLite 文件持久化基座

- 状态：accepted
- 日期：2026-07-17

## 背景

Kernel 需要一个本地、可迁移、可恢复的唯一持久化基座，先关闭不可变 AuditRecord、原子 Event Outbox、cursor/delivery 与 Policy rate-limit 消费的落地缺口。本决策不把 SQLite 扩张成网络服务，也不提前实现 Task、Action、PermissionDecision repository、KCP、`agentd` 或 Publisher 后台循环。

## 决策

1. 新增内部 Rust crate `kernel-sqlite`，使用 `rusqlite` 的 `bundled` SQLite，持久化目标只接受普通文件路径；拒绝空路径、`:memory:` 和所有以 `file:` 开头的 SQLite URI（不只 memory URI）。
2. 每个连接开启并读取验证 `foreign_keys=ON`，设置并验证显式非零 `busy_timeout`。数据库初始化开启并验证 `journal_mode=WAL`；不自行覆盖 `synchronous`。
3. migration 使用crate内嵌、严格升序descriptor。0001/0002保留legacy SQL-bytes SHA-256 ledger语义；已实现0003在同一`BEGIN IMMEDIATE`内升级ledger descriptor列，并用稳定JCS descriptor单hash覆盖exact SQL asset与Rust transform identity/version。已应用名称/checksum/descriptor漂移返回`migration_drift`，数据库版本高于binary优先返回`database_schema_too_new`。每个pending migration的DDL/transform/ledger row同事务提交或回滚；`schema_migrations` bootstrap仍在pending migration事务外独立幂等创建。
4. 所有 public 业务写 API 使用 `BEGIN IMMEDIATE`。这包括 `SqliteStore::with_write_transaction` 与所有 Store convenience 业务写入口（例如 `mark_delivered`）：convenience 不得绕过统一事务边界，只能委托 `with_write_transaction`；只有 `COMMIT` 成功后调用方才能观察到 `Marked` / `AlreadyMarked` / `NotFound` 等业务结果。`WriteTransaction` 只能由 `SqliteStore` 构造，调用方不能自行 commit；closure 成功提交，返回错误时回滚，panic 时先尽最大努力回滚并在释放 transaction 与连接 mutex guard 后恢复原 panic payload，避免 mutex 被展开过程 poison。commit 后补偿 rollback 或任一 rollback 失败时，当前 store 被标记为不可继续使用，后续操作 fail closed，不复用事务状态未知的连接。事务 closure 不接受 async 或网络 callback，外部发布和其它外部副作用必须在提交后执行。WAL journal-mode 初始化与 `schema_migrations` ledger bootstrap 属于 open 时的基础设施例外，不是 public 业务写 API；每个 pending migration 仍是独立的 `BEGIN IMMEDIATE` 单元。
5. AuditRecord 只保存一份 RFC 8785 canonical JSON 文档；ID、类型、时间、Task、Action 查询使用 SQLite JSON expression index，不建立重复普通列双源。插入前运行正式 Schema 校验和 `sent` 支撑引用规则，读取时再次校验并反序列化生成类型。
6. Outbox 已由migration 0003升级为版本化统一表（`payload_json`与`causation_json`均JCS，共享aggregate sequence和AUTOINCREMENT position）。**ADR-0009 / 切片3c 起 production API 为 v2-only**：仅 `append_active_event_v2`；`append_legacy_event_v1` / `PendingLegacyEventV1` / `StoredEventEnvelope::LegacyV1` 已删除。active type/aggregate由generated payload variant和`EVENT_ACTIVE_BINDINGS`派生。每次append在统一savepoint helper中分配sequence/position并最终严格readback；savepoint rollback/release cleanup失败会poison外层transaction，即使caller吞掉局部错误也不能commit。stored decoder 对 `schema_version!=2` 一律 `stored_data_invalid`；open 拒绝含 v1 业务事实的旧库（`reinitialize-required`）。
7. Publisher存储面提供按位置读取及幂等`mark_delivered`；第一次`delivered_at`不可覆盖。mark前执行唯一严格stored decoder（v2-only），损坏记录返回`stored_data_invalid`，不能以delivery mark隐藏。crate仍没有Publisher后台循环、删除、retention或claim lease。
8. Policy rate-limit 消费记录不清理。`RateLimitPort` 只从活动 `WriteTransaction` 借用；preview 不写，winner-only `check_and_consume` 在同一 `BEGIN IMMEDIATE` 事务重新计数并插入，窗口使用 `consumed_at_micros > instant - window`。不为 `SqliteStore` 实现可独立提交的 `RateLimitPort`。

## 备选方案

- 系统 SQLite 动态链接：拒绝；首批需要可重复构建和明确 SQLite 能力，选择 bundled。
- 内存数据库：拒绝；无法验证真实文件锁、WAL、多连接竞争和恢复语义。
- migration 框架：首批暂不采用；当前只有小型顺序 migration，自研 checksum 表面更小且无需引入 async/refinery。
- Audit 普通列加完整 JSON：拒绝；会形成可漂移双源。
- Outbox 保存完整 Envelope JSON：拒绝；列与文档会形成双源，完整 Envelope 可从规范化列确定重建和验证。
- Store 级 rate-limit 独立事务：拒绝；会破坏 PermissionDecision 与消费同事务的 winner-only 原子性。

## 影响与实现范围

- 本 ADR 已由 `rust/crates/kernel-sqlite` 的 migration、Audit、Outbox、cursor/delivery 和 transaction-bound rate limit 实现覆盖。
- migration 0002曾实现legacy Task create/get 表；0003实现descriptor ledger升级与版本化Outbox shape（切片3c 起 transform 不迁移非空 v1 Outbox）；0004实现root TaskCreate v2 fresh-baseline表与active root repository（`create_root_task_v2`）；0005 drop dead v1 业务表。legacy Task/Audit/Outbox v1 write 已在切片3c删除。
- Audit 的 PermissionDecision/policy context 字段相等、rollback 权威投影、Provider/ModelCall 一致性仍必须由后续 repository 在同一事务中完成；root TaskCreate v2 的 creation context / provenance / Event correlation 一致性已由 active root repository 覆盖。
- `system_internal` 使用 null actor 是否确无注册主体仍由生产者证明。
- Task 更新/list、Action/PermissionDecision repository、KCP、`agentd`、Publisher 循环、网络、retention 和 claim lease 不在本 ADR 已实现范围。
