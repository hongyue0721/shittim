# Shittim 实现进度

> 状态日期：2026-07-21（`V2InitialBuildActive`切片4c完成：migration 0008 + Approval v2 current-head CAS 三方法与 approval.state_changed producer + Identity credential/challenge/evidence repositories。§13.7 未闭合——child materializer 与五方法 handler 仍缺。）

域状态表唯一来源：[`IMPLEMENTATION_MATRIX.md`](IMPLEMENTATION_MATRIX.md)。本文只保留当前切片事实、未完成 backlog 与下一步；逐切片编年史由 git log 与 ADR 承载。

## 当前里程碑：V2InitialBuildActive

**目标（ADR-0009 / IC §13.7）**

- 没有用户，不从 v1 迁移数据；当前存储格式就是 v2 fresh baseline。
- production bindings + v2 dispatcher + v2 repository 作为**初始交付**，不是旧部署 cutover。
- 旧开发数据库必须拒绝启动并返回稳定 `reinitialize-required`；禁止自动清库 / 隐式升级 / 读后补写。
- v1 runtime 写路径删除（handler/adapter/repository write、legacy direct-child、`append_legacy_event_v1`、`StoredEventEnvelope::LegacyV1` production variant），而非加版本分支。
- `TaskCreationProvenance` 只保留 `root_command_v2` / `child_action_v2`。
- **不在本里程碑**：Publisher、versioned KCP `event.poll`；retained poll v1 不得返回 EventEnvelope v2。

**完成定义**

1. root `task.create` v2 唯一 active 写路径（repository + handler + method-aware preflight + production bindings）；
2. child 唯一经 `task.child.create` Action materializer；
3. Action / PermissionDecision / Approval v2 真实持久化闭环；
4. 有业务 owner 的 active Event producer：root/child `task.created`、`action.state_changed`、`approval.state_changed`；
5. production MethodVersionBindings：`task.create` active=[2]、v1 仅 `unsupported_schema_version`，其余七方法 active=[1]；
6. 不得伪造无 owner 的 producer；v1 写路径已删；旧库 reinitialize-required。

**七切片计划**

| 切片 | 内容 | 状态 |
|---|---|---|
| 0 | 规范文档落锤：ADR-0009 + IC/CORE/CONFORMANCE/PROGRESS/MATRIX/API 对齐 | **已完成** |
| 1a | root创建路径持久对象Schema：ContentOriginV2、AuditRecordV2、TaskCreationProvenanceV1、AuditAllocationV2 + manifest/generated/conformance | **已完成** |
| 1b | Action/child Schema：ActionTransitionIntentV1、ActionRequestV2、Delta/Material/Observation projection + `kernel-authorization` pure crate | **已完成** |
| 1c-i | 授权核心五Schema：PermissionDecisionV2、PolicyRuleV2、ApprovalRecordV2、SubjectProjectionV1、ApprovalEventAllocationV1 | **已完成** |
| 1c-ii | 身份/挑战/证据与远程签名八Schema | **已完成** |
| 2 | fresh SQLite 基线 + root repository | **已完成** |
| 3a | production MethodVersionBindings 基础层（manifest 八方法 + stage gate + generated catalog/selector） | **已完成** |
| 3b | method-aware KCP v2 preflight/dispatcher/handler 消费 bindings | **已完成** |
| 3c | 删除 v1 runtime 写路径 + Outbox v2-only + 旧库 reinitialize-required + migration 0005 | **已完成** |
| 4a | Action 持久化 + ActionTransitionIntent + `action.state_changed` producer | **已完成** |
| 4b | PolicyRule + PermissionDecision repositories + Action 评估编排（Approval 属 4c） | **已完成** |
| 4c | Approval / Identity repository | **已完成** |
| 5 | child Action materializer | 未开始 |
| 6 | §13.7 谓词闭合（依赖 4–5 + 其余 active producers） | 未开始 |

**已实现（代码/Schema 事实）**

- 规范与工程基线：Freedom-first / Kernel Owns Reality 合同、Apache-2.0、双仓同步 library/CLI、Node/pnpm 零依赖根基座（exact Node 24.18.0 / pnpm 11.3.0）、统一门 `scripts/check-schema.sh`。
- Schema/Rust 契约：Rust workspace、Draft 2020-12 + manifest v2（production=83 entries，41 retained + 42 component-native）、`schema-tool` 单 root transaction / target-scoped IR / TaggedUnion / string enum `ALL` / string-array const / RFC 8785；production `METHOD_VERSION_BINDINGS` 为 IC §13.5 八方法集（切片3a）。
- 纯领域：`domain-task`（Task/Action 状态图、revision/plan_version）、`domain-policy`（URI/glob/Default Allow/rate-limit draft，Stop Fence/Recovery 独立 Blocked）。
- 持久化：`kernel-sqlite` migration 0001–0007；AuditRecord **v2**；strict Task/TaskScope/ContentOrigin(v2) 读；版本化 **v2-only** Outbox + 严格 stored decoder + savepoint poison；**active root TaskCreate v2 repository**（`create_root_task_v2`）；**Action current-snapshot + ActionTransitionIntent + `action.state_changed` producer**（切片4a）；**PolicyRuleV2 + PermissionDecisionV2 + `evaluate_action_permission`**（切片4b）；legacy TaskCreate/Audit/Outbox v1 write 已删；旧库 open `reinitialize-required`。
- KCP 库级：`kernel-kcp` method-aware Value preflight、三方法 registration/dispatcher/handler（`system.ping` / **active root `task.create` v2** / `task.get`）与 SQLite adapter→`create_root_task_v2`；不可连接，无 bytes/frame/server。
- ADR-0006 首批：12 business-v2 Schema + `kernel-task-creation` pure library + official fixtures/harness + schema-tool strict pointer CLI。
- ADR-0008 前两段：Event v2 八 Schema、`EventTypeBinding`/active·legacy catalog、typed EventEnvelope v1/v2、migration 0003 descriptor v1 与统一 Outbox shape（切片3c 起 production v2-only）。
- V2InitialBuildActive切片1a–1c-ii：root 持久对象、Action/child 授权、授权核心、身份/挑战/证据 Schema 与 pure crate 已落地（manifest=83）。
- V2InitialBuildActive切片2：migration 0004（`content_origins_v2`、`task_creation_provenances`、`audit_records_v2`、`root_task_create_idempotency_v2` + tasks/scope FK 重建以允许 v2 origin）；`WriteTransaction::create_root_task_v2` 单事务写 Origin/Scope/Task/Provenance/Audit/idempotency/Event；全闭包 canonical readback（Created/Replayed 共用）；幂等重放/冲突；回滚不占号；与 v1 表互不污染。
- V2InitialBuildActive切片3a：production `method_version_bindings` 精确八方法（`task.create` active=[2]/legacy=[1]，其余 active=[1]）；`validate_production_manifest_stage` 要求 Envelope-derived 完整集 + IC §13.5 lifecycle；generated `METHOD_VERSION_BINDINGS` 非空且 `select_request_version` 可用。
- V2InitialBuildActive切片3b：kernel-kcp preflight 按 (family, method, payload.schema_version) 调 `select_request_version`；V2 Envelope 结构验证 + active payload Schema；`task.create` v2 Accepted/Registered，v1 → `unsupported_schema_version`；handler 七 UUID（含 CreationProvenance）+ root-only 检查 + `TaskCreateResponseV2`；adapter 映射 `create_root_task_v2`；删除 kcp 侧 v1 create handler/adapter/ports 路径。
- V2InitialBuildActive切片3c：删除 `create_task`/`TaskCreateCommand`/`prepare_legacy_v1_create`、AuditRecord v1 write、`append_legacy_event_v1`/`PendingLegacyEventV1`/`StoredEventEnvelope::LegacyV1`；Outbox decoder 对 schema_version=1 → `stored_data_invalid`；`SqliteStore::open` 后 `reject_legacy_v1_business_data`；migration 0003 transform 对非空 legacy Outbox 直接 reinitialize-required；migration 0005 在空表前提下 drop dead v1 表。
- V2InitialBuildActive切片4a：migration 0006（`actions` + `action_transition_intents`）；`insert_pending_action` / `get_action`（公开）；intent 五方法 + `mark_committed_with_event` 同事务 CAS+`action.state_changed`（causation=`action_transition`）+ reconcile 三态；状态事件唯一权威为 mark，crate-private CAS transition 不写 Outbox；需 lease effects 的边 fail closed；domain-task 边合法性与 evidence 门；sequence/position 失败不占号。
- V2InitialBuildActive切片4b：migration 0007（`policy_set_metadata` bootstrap revision 0、`policy_rules`、`permission_decisions`）；PolicyRule append-only revision + global set counter；PD immutable append（连续 decision_revision）+ Action ref 双向校验；`evaluate_action_permission` 单事务 matcher→指纹→PD→`permission.evaluated` Audit→Action CAS（allow/deny/require_* deferred，无 Approval 创建）；rate-limit 同事务消费与回滚；material/observation 双指纹真实重算。
- V2InitialBuildActive切片4c：migration 0008（`approval_records`、`approval_chain_heads`、`identity_credentials`、`identity_challenges`、`identity_evidence`）；Approval 三复合 CAS 方法 `append_request`/`resolve`/`invalidate_and_optionally_replace`（expected head CAS、canonical subject 相等、replacement 原子推进、change_kind 真值表投影）；每成功 head 变化恰好一条 `approval.state_changed` + 对应 `approval.requested|resolved|invalidated` Audit；CAS 冲突/replay 不产 Event；Identity credential register/rotate/revoke、challenge issue/consume/expire（终态不可逆、expire 只写 `identity.challenge_expired` Audit 不发 Approval event）、local/system evidence immutable。

**未实现（不得宣称完成）**

- child Action materializer；Approval/Identity repositories；Action 闭集其余写方法（lease/policy binding/child completion/recovery list）。
- 其它 active business **producer**（child/approval）；**Publisher** 与 versioned KCP **poll** 明确不在本里程碑。
- 其余 Command 幂等/乐观锁；**五方法**正式 handler；Unix Domain Socket / Windows Named Pipe KCP **server/client**；**agentd**。
- `ts/*` 包、SDK client、Pi `agent-runtime`；统一 **Extension SDK Base**；optional Computer Use Profile；Tauri 桌面客户端；Provider/Memory/Initiative/Broker。
- `specs/CONFORMANCE.md` 全部 BASE 与声明的 Profile/Platform 条件套件。

## 未完成 backlog

- [x] 切片 0：规范文档落锤（ADR-0009 等）
- [x] 切片 1a：root创建路径持久对象Schema/manifest/generated/conformance
- [x] 切片 1b：Action/child五Schema + `kernel-authorization` pure crate + projection official fixtures/harness/oracle
- [x] 切片 1c-i：授权核心五Schema + SubjectProjection pure API/fixture
- [x] 切片 1c-ii：Credential/Challenge/Evidence/Remote signature家族
- [x] 切片 2：fresh SQLite 基线 + root repository
- [x] 切片 3a：production MethodVersionBindings 基础层
- [x] 切片 3b：method-aware KCP v2 preflight/dispatcher/handler
- [x] 切片 3c：删除 v1 写路径 + Outbox v2-only + 旧库拒绝 + migration 0005
- [x] 切片 4a：Action 持久化 + ActionTransitionIntent + `action.state_changed` producer
- [x] 切片 4b：PolicyRule + PermissionDecision repositories + Action 评估编排
- [ ] 切片 4c：Approval/Identity repository
- [ ] 切片 5：child materializer
- [ ] 切片 6：§13.7 谓词闭合（child/Action/PD/Approval + 其余 producers）
- [ ] active Event business producers（child/approval；root 已在切片 2 接入；action 已在切片 4a 接入）
- [ ] 五方法正式 handler；可连接 KCP server/client；`agentd`
- [ ] 其它 Command 幂等与乐观锁；Task list cursor ADR 拍板后实现
- [ ] Publisher + versioned KCP poll（**后续里程碑**，不在 V2InitialBuildActive）
- [ ] Extension SDK Base → `schema/SDK`；TS 包 / SDK client / `agent-runtime`
- [ ] optional Computer Use Profile；桌面客户端；Provider/Memory/Initiative/Broker
- [ ] CONFORMANCE 全量 BASE + 声明 Profile 套件

## 当前阻塞

- kcp runtime 与 sqlite repository 已切到 active create v2 / Outbox v2-only；Action/`action.state_changed`（4a）与 PD/PolicyRule/评估编排（4b）已落地；五方法无 handler，禁止启动 server。§13.7 仍缺 Approval/Identity/child 闭环。
- legacy v1 repository 的 Delegation 正向路径未实现（非 null 固定 not found）；active v2 repository 同样在 Delegation authority 未落地前 fail-closed 返回 `delegation_not_found`。
- Task list cursor 编码须先 ADR/API 拍板。
- Audit：`permission.evaluated` 已在评估编排同事务校验 policy_context 与 PD 字段相等；仍缺 rollback 权威投影、Provider/ModelCall 一致性与其它业务 producer。
- Extension SDK Base / Computer Use 仍为 `contract-only`；不得把规范或根 Node 工作区冒充 SDK 实现。
- 真实 Provider/Channel/Privilege Broker 需要后续真实环境；当前无伪造支持。
- 默认 PATH 可能不是 Node 24.18.0；须显式 `~/.local/share/pnpm`。

## 下一步

1. 切片 4c–5：Approval/Identity repositories 与 child materializer；接入 approval/child active Event producers。
2. 切片 6：在 4c–5 完成后闭合 §13.7 全部谓词。
3. **之后**再做 Publisher 与 versioned KCP poll；再实现剩余五个 Catalog handler 与可连接 server。
4. 随后实现 Extension SDK Base 与 TypeScript/client。

## 最近验证

本切片（V2InitialBuildActive 4b）验证命令：

```text
export PATH="$HOME/.local/share/pnpm:$PATH"
export TMPDIR=/mnt/data/shittim-build-tmp
export CARGO_TARGET_DIR=/mnt/data/shittim-cargo-target
cargo test --manifest-path rust/Cargo.toml -p kernel-kcp
cargo test --manifest-path rust/Cargo.toml --workspace
cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets -- -D warnings
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
./scripts/check-schema.sh
node scripts/update-file-manifest.mjs --check
git diff --check
```
