# Shittim 实现进度

> 状态日期：`kernel-sqlite` 文件持久化基座通过独立验收后。

## 当前阶段

已完成 Rust/Schema 契约基座、`domain-task` 纯领域 Task/Action 状态机、`domain-policy` 纯领域 matcher，以及 `kernel-sqlite` 文件 migration、不可变 AuditRecord、原子 Event Outbox、cursor/delivery 与 transaction-bound RateLimitPort。当前仍没有 Task/Action/PermissionDecision repository、KCP server、`agentd`、TypeScript workspace、桌面客户端、Publisher 循环或 Provider。

`domain-task` 只计算状态图、不变量、revision/plan_version 和待持久化意图；`domain-policy` 只计算规则匹配、非持久 decision draft 与 canonical input。`kernel-sqlite` 只拥有本批明确的 SQLite 基座，不伪造尚无权威表的跨对象一致性。

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
- [x] 使用真实临时文件与多 `SqliteStore` 验证 migration、Audit、Outbox 并发/回滚、panic、delivery、rate-limit 边界/隔离/竞争和 matcher winner-only；`kernel-sqlite` 共 24 项测试。
- [x] 新增 [`api/kernel-sqlite.md`](api/kernel-sqlite.md)。

## 未完成

- [ ] 实现 Task/TaskScope/ContentOrigin/idempotency、Action、PermissionDecision repository，以及 Audit 的 PermissionDecision/policy context 字段相等、rollback 权威投影、ModelCall provider、Task creation canonical 子事实跨对象一致性；这些必须复用现有 `WriteTransaction`。`task.create` 契约已拍板，Policy URI 单项 normalizer 已公开可复用，但没有表、migration 或 repository 实现。
- [ ] 实现请求幂等与乐观锁；`task.create` scope/projection/生命周期已定义，尚未持久化。
- [ ] 实现 Unix Domain Socket / Windows Named Pipe KCP server/client。
- [ ] 实现 `agentd` 组合根和首批八个 KCP 方法处理。
- [ ] 创建 TypeScript workspace、SDK client 与 Pi `agent-runtime`。
- [ ] 创建 Tauri/React/Ant Design 蓝白桌面客户端。
- [ ] 实现 Extension SDK、Provider、Memory、Initiative、Computer Use 与 Broker。
- [ ] 完成 `specs/CONFORMANCE.md` 全量自动化测试。

## 当前阻塞

- `task.create` repository 的四项规范阻塞已关闭：receipt hash、idempotency projection、TaskScope/ContentOrigin 初值、固定 Audit/Event producer 均有精确契约和 fixture。实现仍未开始；当前没有 Task/Scope/Origin/idempotency 表，非 null Delegation 正向路径仍因 authority repository 缺失而固定返回 `delegation_not_found`。
- Task list cursor 仍保持 opaque；编码技术选择必须在 repository 实现前通过 ADR/API 拍板，不属于上述四项阻塞。
- AuditRecord 的 Schema 内条件、SQLite immutable/canonical Store 和 `sent` 支撑引用检查已完成。PermissionDecision/policy context、rollback 投影、Provider/ModelCall、Task creation canonical 子事实仍缺少对应权威 repository 表，明确作为下一 repository 硬门；不得用默认值或本 crate 的单记录校验冒充跨对象一致性。
- `system_internal` null actor 的“确无可归因注册主体”仍由上层 producer 证明。
- Node 24 LTS 已可用（24.18.0，pnpm user runtime），TypeScript 工具链不再受版本阻塞。
- 真实模型 Provider、远程 Channel、跨平台 Provider 与 Privilege Broker 仍需要后续真实环境和用户选择；当前没有伪造支持。

## 下一步

1. 按 [`api/task-repository-contract.md`](api/task-repository-contract.md) 在 `kernel-sqlite::WriteTransaction` 上实现 Task/TaskScope/ContentOrigin/idempotency repository 与固定 Task creation Audit/Event producer；实现前单独拍板 Task list cursor。
2. 实现 Action/PermissionDecision repository，并关闭其余 Audit 跨对象一致性硬门。
3. 实现 KCP 本地传输和 Task 创建/查询/Event 轮询纵切，随后接入 Publisher 循环。
4. 再建立 TypeScript client/SDK 和 Ant Design 桌面端。

## 最近验证

```text
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path rust/Cargo.toml --workspace
./scripts/check-schema.sh
git diff --check
```

全部通过；`domain-task` 47 项测试，`domain-policy` 30 项测试，`kernel-contracts` 45 项测试（5 unit + 40 contract），`schema-tool` 11 项测试，`kernel-sqlite` 24 项测试，当前 workspace 共 157 项测试。

## 事实来源

- 全局不变量：[`../AGENT.md`](../AGENT.md)
- 状态机与恢复：[`../specs/CORE_ARCHITECTURE.md`](../specs/CORE_ARCHITECTURE.md)
- 实现契约：[`../specs/IMPLEMENTATION_CONTRACTS.md`](../specs/IMPLEMENTATION_CONTRACTS.md)
- 验收：[`../specs/CONFORMANCE.md`](../specs/CONFORMANCE.md)
- Schema：[`api/schema-generation.md`](api/schema-generation.md)
- 状态机 API：[`api/domain-task.md`](api/domain-task.md)
- Policy matcher API：[`api/domain-policy.md`](api/domain-policy.md)
- Task repository 契约：[`api/task-repository-contract.md`](api/task-repository-contract.md)
- SQLite API：[`api/kernel-sqlite.md`](api/kernel-sqlite.md)
