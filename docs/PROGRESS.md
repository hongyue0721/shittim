# Shittim 实现进度

> 状态日期：KCP `serde_json::Value` preflight + 三方法注册式 dispatcher Rust 实现完成后。

## 当前阶段

已完成 Rust/Schema 契约基座、`domain-task`、`domain-policy`、`kernel-sqlite` Task create/get repository，以及不可连接的 `kernel-kcp` `serde_json::Value` preflight、全八方法 typed Accepted、三方法 registration/dispatcher 和 `system.ping` / `task.create` / `task.get` typed application handler。当前仍没有 Task 更新/list、Action/PermissionDecision repository、可连接 KCP server、`agentd`、TypeScript workspace、桌面客户端、Publisher 循环或 Provider。

`domain-task` 只计算状态图、不变量、revision/plan_version 和待持久化意图；`domain-policy` 只计算规则匹配、非持久 decision draft 与 canonical input。`kernel-sqlite` 拥有本批明确的 SQLite 基座和 Task create/get 事实，不伪造尚无权威表的其它跨对象一致性。

## 已完成

### 规范与工程基线

- [x] 建立 Freedom-first、Kernel Owns Reality、Core 不可自改规范基线。
- [x] 补齐 Task/Action/Recovery、Policy、Event/Outbox、KCP 首批可编码契约。
- [x] 明确 `owner` 只是未认证预留标签；`stop.activate` 是首批 Emergency Stop 入口。
- [x] 接受工作区、Schema 生成和 KCP 本地传输 ADR。
- [x] 添加 Apache-2.0 根许可证。

### Schema 与 Rust 契约

- [x] 创建 Rust workspace 与 `rust-toolchain.toml`（1.97.0）。
- [x] 创建 41 个 Draft 2020-12 Schema 和 `schemas/manifest.json`。
- [x] 实现 `schema-tool generate/check/validate/canonicalize`。
- [x] 从 Schema 自动生成 Rust 类型、catalog 及 Command/Query/Event typed decode。
- [x] optional/non-null 字段由 Schema 元数据生成 `skip_serializing_if = "Option::is_none"`；required-nullable 仍输出显式 `null`；optional-nullable 保持 `None -> null` 不改 wire。
- [x] 执行 meta-schema、跨文件 `$ref`、生成漂移和未知关键字检查。
- [x] 使用 `serde_json_canonicalizer` 实现 RFC 8785，并提供共享测试向量。
- [x] 拍板 `task.create` repository 四项阻塞：规范化后的完整 payload receipt hash、精确幂等等价 projection、TaskScope/ContentOrigin 初值、固定 `task.creation_recorded` producer 与 `task.created` 上层 ID 边界；新增独立复合 hash fixture，并由 Rust 契约测试和 schema-tool 实际 CLI 双路径共同验证。
- [x] `scripts/check-schema.sh` 覆盖重复生成、fmt、Clippy、workspace tests 和生成物 Git 漂移。
- [x] 定义 `AuditRecord` v1：增加 `task.creation_recorded` 不可变创建快照，显式 `external_content_status` / PayloadManifest stable refs，并拍板 PermissionDecision/policy context、rollback 权威投影、实际 Provider/模型建议引用的双源一致性；Schema 内条件已有运行时测试，不自动公开为 Event/Outbox。
- [x] 明确 Event aggregate `sequence`：首条已提交事件为 `0`，后续严格连续 `+1`，回滚事务暂分配不占号。

### Task/Action 纯领域状态机

- [x] 新增 `rust/crates/domain-task`，直接使用生成的 TaskStatus/ActionStatus，不复制状态枚举。
- [x] 实现 CORE §10 Task 状态图、revision 和 plan_version 规则；兼容 `task.create` 的 `plan_version=0`。
- [x] `succeeded` 按 `TaskSpec.success_criteria` 完整字符串**多重集合**精确覆盖，每个 occurrence 均需 `verified_ok`。
- [x] `partially_completed` 和 `rolling_back` 均要求明确副作用引用，不凭状态猜测事实。
- [x] 实现 CORE §11 Action 状态图；confirm 是 pending metadata update，不是假装 approved。
- [x] `completed`/`failed` 要求 Verification 事实；不确定结果要求 crash/timeout/ambiguous 等结构化原因。
- [x] Lease 过期与确定未派发取消返回绑定 action_id 的原子释放意图。
- [x] 补偿身份只由 `ActionRequest.parent_action_id` 推导，不存在平行 ActionRole。
- [x] `retry_original` 仅在副作用明确未发生且幂等保障成立时合法。
- [x] 新增 NxN 矩阵、证据测试与 proptest；`domain-task` 共 47 项测试。
- [x] 新增 [`api/domain-task.md`](api/domain-task.md)；本批无外部 SDK API 变化。

### Freedom-first Policy matcher

- [x] 新增 `rust/crates/domain-policy`，直接使用生成 PolicyRule/Actor/ContentOrigin/EntryPoint/SideEffectClass/decision enum。
- [x] 实现 URI 规范化、segment glob、capability/operation `.*`、exclude、side-effect ceiling；公开单项 `normalize_uri` / `normalize_uri_pattern` 复用同一 parser，供未来 Task repository 保序、保重复地逐项调用。
- [x] 实现 TaskScope resource containment 纯函数 `resource_refs_within_task_scope`：include 空=不限制、exclude 优先、全量先验证再返回布尔；stored pattern 必须已规范化；不授权、不改 Scope、不复用 PolicyRule `match_resources`。
- [x] 按 SECURITY §2.3 实现 specificity 与 priority/effect/revision/ID 稳定排序，只计算实际命中备选。
- [x] 实现 time window、Delegation/local-presence 精确布尔和 authoritative `RateLimitPort` winner-only 原子消费重选。
- [x] Stop Fence/Recovery invariant 优先返回独立 Blocked，不创建隐藏 deny；S0–S5 无规则均 Default Allow。
- [x] 生成非持久 `PermissionDecisionDraft`、RFC 8785 key params hash 与 `CanonicalEvaluationInput`，不伪造持久 revision/hash。
- [x] 补充 ContentOrigin 多值同一-origin 匹配语义及 Conformance 锚点。
- [x] 新增 [`api/domain-policy.md`](api/domain-policy.md)。

### Kernel SQLite 文件持久化基座

- [x] 接受 ADR-0004，使用 `rusqlite` bundled、文件 DB、WAL、foreign keys、显式 busy timeout 与 checksum migration。
- [x] 新增 `rust/crates/kernel-sqlite` 和 migration 0001；重复 open 与两个线程首次并发 open 幂等，pending migration 的 DDL/ledger 原子，漂移、未知版本与过新 schema 使用稳定 machine code 拒绝。
- [x] AuditRecord 以 RFC 8785 canonical JSON 单源不可变存储，expression index 支持 ID/type/time/task/action；插入和读取均重验正式 Schema。
- [x] 实现 `sent` 至少一个 producer/causation 支撑引用的 repository 内单记录规则；Audit 失败可与同事务其它写整体回滚。
- [x] Outbox 使用规范化列与 payload JSON；每次 append 先预检并在内部 SAVEPOINT 中原子分配 sequence/position、插入和最终 decode，调用者忽略单次错误并继续 commit 也不留下脏行或空洞。
- [x] 实现十进制 cursor、严格 `>` 分页、历史读取、未投递重复读取/重启后重投的 at-least-once 语义与第一次 `delivered_at` 不可覆盖。
- [x] 写事务对 closure panic 安全：panic 前写入回滚，释放连接 mutex guard 后恢复原 payload，后续同 store 可继续读写且锁不 poison。
- [x] 实现只能从 `WriteTransaction` 获取的生产 `RateLimitPort`；preview 不消费，winner-only 在同一 `BEGIN IMMEDIATE` 中重新计数并插入。
- [x] 新增 migration 0002 与 Task create/get repository：canonical Task/TaskScope/ContentOrigin 单源、generated-column FK/index、关系 ordinal 镜像、幂等 replay/conflict、固定 Audit/Event 和严格 fail-closed 读取；不实现 list/update/KCP。
- [x] 使用真实文件验证 generated UNIQUE parent key、deferred Task↔Scope FK、fixture hash、完整 Audit/Event 公开读取、outer panic 全事实回滚与无号重试、重复分配 ID 矩阵、非法 URI/pattern 稳定错误码、幂等 canonical/hash 与 parent relation corruption、v1→v2 保留升级、多 store replay/conflict 串行，以及 parent/delegation 失败；`kernel-sqlite` 共 39 项测试。
- [x] 新增 [`api/kernel-sqlite.md`](api/kernel-sqlite.md)。

### KCP typed application handler 合同

- [x] 闭合 `system.ping` / `task.create` / `task.get` 的 typed validated input 边界；当时未包含 Value preflight，现已由后续 §5.11 合同单独闭合。
- [x] 固定成功 payload 方法级 Schema 门、最终 Response Envelope Schema 门、request_id 原样与 ok/error 互斥。
- [x] 固定可注入 `KernelClock`、UUID/opaque ID generator 和闭集 `BackendError` 高阶 Task backend；实现 `SystemKernelClock` 与使用可失败 OS 随机源的 `RandomKernelIdGenerator`，随机源失败进入 `IdGenerationError` 而非 panic；SQLite adapter 穷举 `StoreErrorCode`，复用 `with_write_transaction` + 现有 repository，不暴露 transaction/SQL。
- [x] 固定 deadline RFC 3339 UTC instant 比较与两次读取：入口先检查；create 事务不可中途取消，commit 后到期返回 `deadline_exceeded` 但事实保留并以同一 idempotency key 恢复。
- [x] 固定六个 Kernel UUID（版本不限定）、独立 correlation/dedup 生成，不把 Kernel-owned 标识伪装成 caller-owned 或固定派生规则。
- [x] 固定 Created/Replayed 均返回当前 Task；Created 同时返回可信绑定的 committed Event ID，只有 Created 产生 durable Outbox 的 post-commit Publisher wake-up intent，通知失败不回滚、不声明 delivered。
- [x] 固定三个方法按 backend/`StoreErrorCode` 的 KCP code、safe message、details=null、retryable 映射，不匹配错误 message。
- [x] 增加完整 fake backend/clock/ID、deadline pre/post、Created/Replayed/get/notfound、每项错误与 payload/envelope Schema 的 Conformance 矩阵。
- [x] 新增 `rust/crates/kernel-kcp`，只接收 generated typed envelope；实现三个 handler、闭集 ports、稳定 response/error 与 `HandlerContractFailure`，不提供 raw JSON/frame/dispatcher/server。
- [x] 实现 `SqliteTaskBackend`，在 `with_write_transaction` 中复用现有 repository；当前 `StoreErrorCode` 无 wildcard 穷举映射。Created 的 operation Event UUID 由 repository append/verify + 外层 commit 证明，真实文件 SQLite 测试通过公开 Store API 绑定 intent、Outbox、Audit、Task、Scope 与 Origin，replay 不新增事实。
- [x] 公共 `handle_*` 固定内置 generated Schema response 门；validator fault seam 只存在于 crate 私有 unit test，不是 public API/feature/SDK。
- [x] `kernel-kcp` 原 handler 基线为 25 项测试（4 个 unit + 21 个 handler/SQLite integration）。
- [x] 新增 [`api/kernel-kcp.md`](api/kernel-kcp.md)。

### KCP Value preflight 与注册式 dispatcher 实现

- [x] 在 `kernel-contracts` 增加 `ContractFailureStage`、`ContractFailureClassification`、`ClassifiedContractFailure`、`ContractError::stage()` 与 `classification_for_preflight()`；caller Schema violation 与 post-Schema/generated/catalog failure 结构化区分。
- [x] schema-tool 模板生成 `decode_after_validation`，并令 wire/payload/discriminator default 分别产生 `WireDecodeAfterSchema`、`PayloadDecodeAfterSchema`、`GeneratedDiscriminatorMapping`；生成物通过 schema-tool regenerate，无手改。
- [x] 在 `kernel-kcp` 实现 `preflight_value(Value)`，按 request_id > family > protocol > auth > generated family method > 根 payload version > 完整 Schema/typed decode 固定优先级短路。
- [x] 固定五类 wire error 的 code/message/details/retryable，并复用 crate-private generated Response Schema finalizer；final response fault seam 本地 fail closed。
- [x] 实现 private-state `TypedCatalogRequest` / `RegisteredRequest`，避免调用方构造 family/discriminator/payload 错配；公开只读 family/method introspection。
- [x] `narrow_to_registered` 对 generated payload enum 穷举，无 wildcard：三 registered + 五不可序列化 Known enum。
- [x] 实现 borrowing `TypedDispatcher<C,G,B>`，直接调用三个 public `handle_*`，不增加平行 ports、不重复 deadline/Schema、不改写 `HandlerResult` 或 intent。
- [x] 增加 static negative Serialize assertions、八方法合法 Value、priority/field/cross-family/root/nested version、known malformed/valid、固定 error response、private unknown-schema/final-response fault seam、dispatcher response/ContractFailure/Created intent 与 clock 路由测试。
- [x] `kernel-contracts` 53 项测试；`schema-tool` 14 项测试；`kernel-kcp` 46 项测试（12 unit + 34 integration）。
- [x] 没有新增 Schema、bytes/UTF-8/JSON parse/frame/transport/server/agentd、五方法 handler或 `process_value`。

## 未完成

- [ ] 实现 Task 更新/list、Action、PermissionDecision repository，以及 Audit 的 PermissionDecision/policy context 字段相等、rollback 权威投影、ModelCall provider 一致性；Task create/get 与 task creation Audit/Event canonical 一致性已实现。
- [ ] 为其它 Command 实现请求幂等与乐观锁；`task.create` scope/projection/生命周期已持久化。
- [ ] 为五个已知未注册方法实现正式 handler；完成前 `KnownCatalogMethodNotImplemented` 只作为本地注册完整性结果，server 阶段门保持关闭。
- [ ] 实现 Unix Domain Socket / Windows Named Pipe KCP server/client（受上述阶段门阻塞）。
- [ ] 实现 `agentd` 组合根和首批八个 KCP 方法处理（本批三方法合同不等于八方法可用）。
- [ ] 创建 TypeScript workspace、SDK client 与 Pi `agent-runtime`。
- [ ] 创建 Tauri/React/Ant Design 蓝白桌面客户端。
- [ ] 实现 Extension SDK、Provider、Memory、Initiative、Computer Use 与 Broker。
- [ ] 完成 `specs/CONFORMANCE.md` 全量自动化测试。

## 当前阻塞

- Value preflight/registration/dispatcher 已实现，但五个 Catalog 方法仍没有 handler，因此 server 生命周期仍被硬性阻塞。
- `task.create` repository 已完成；Delegation authority 正向路径仍未实现，任何非 null Delegation 固定返回 `delegation_not_found`。
- Task list cursor 仍保持 opaque；编码技术选择必须在 repository 实现前通过 ADR/API 拍板，不属于三方法 handler。
- AuditRecord 的 Schema 内条件、SQLite immutable/canonical Store 和 `sent` 支撑引用检查已完成。PermissionDecision/policy context、rollback 投影、Provider/ModelCall、Task creation canonical 子事实仍缺少对应权威 repository 表，明确作为下一 repository 硬门；不得用默认值或本 crate 的单记录校验冒充跨对象一致性。
- `system_internal` null actor 的“确无可归因注册主体”仍由上层 producer 证明。
- Node 24 LTS 已可用（24.18.0，pnpm user runtime），TypeScript 工具链不再受版本阻塞。
- 真实模型 Provider、远程 Channel、跨平台 Provider 与 Privilege Broker 仍需要后续真实环境和用户选择；当前没有伪造支持。

## 下一步

1. 实现 Action/PermissionDecision repository，并关闭其余 Audit 跨对象一致性硬门。
2. 为剩余五个 Catalog 方法逐个提供正式 handler；八方法 registration 完整后再关闭 server 阶段门。
3. 随后实现本地传输、Task/Event 纵切与 Publisher 循环。
4. 再建立 TypeScript client/SDK 和 Ant Design 桌面端。

## 最近验证

```text
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path rust/Cargo.toml --workspace
./scripts/check-schema.sh
git diff --check
```

全部通过；workspace 共 247 项测试，`domain-policy` 48（含 TaskScope containment 18）、`kernel-contracts` 53、`schema-tool` 14、`kernel-kcp` 46。

## 事实来源

- 全局不变量：[`../AGENT.md`](../AGENT.md)
- 状态机与恢复：[`../specs/CORE_ARCHITECTURE.md`](../specs/CORE_ARCHITECTURE.md)
- 实现契约：[`../specs/IMPLEMENTATION_CONTRACTS.md`](../specs/IMPLEMENTATION_CONTRACTS.md)
- 验收：[`../specs/CONFORMANCE.md`](../specs/CONFORMANCE.md)
- Schema：[`api/schema-generation.md`](api/schema-generation.md)
- 状态机 API：[`api/domain-task.md`](api/domain-task.md)
- Policy matcher API：[`api/domain-policy.md`](api/domain-policy.md)
- Value preflight/dispatcher 合同：[`api/kcp-preflight-dispatcher.md`](api/kcp-preflight-dispatcher.md)
- Typed handler API：[`api/kernel-kcp.md`](api/kernel-kcp.md)
- Task repository 契约：[`api/task-repository-contract.md`](api/task-repository-contract.md)
- SQLite API：[`api/kernel-sqlite.md`](api/kernel-sqlite.md)
