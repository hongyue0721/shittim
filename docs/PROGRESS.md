# Shittim 实现进度

> 状态日期：2026-07-20（`V2InitialBuildActive`切片2完成：migration 0004 + `kernel-sqlite` active root TaskCreate v2 原子持久化 repository；`kernel-task-creation` pure API 接入；幂等/回滚/corruption fail-closed 测试闭合。production MethodVersionBindings 仍为空；KCP handler/preflight、child materializer、v1 写路径删除未完成。）

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
| 3 | method-aware KCP v2 preflight/dispatcher/handler + production bindings | 未开始 |
| 4 | Action / PD / Approval / Identity repository | 未开始 |
| 5 | child Action materializer | 未开始 |
| 6 | 删除 v1 写路径收口 + 旧库拒绝 + §13.7 谓词闭合 | 未开始 |

**已实现（代码/Schema 事实）**

- 规范与工程基线：Freedom-first / Kernel Owns Reality 合同、Apache-2.0、双仓同步 library/CLI、Node/pnpm 零依赖根基座（exact Node 24.18.0 / pnpm 11.3.0）、统一门 `scripts/check-schema.sh`。
- Schema/Rust 契约：Rust workspace、Draft 2020-12 + manifest v2（production=83 entries，41 retained + 42 component-native）、`schema-tool` 单 root transaction / target-scoped IR / TaggedUnion / string enum `ALL` / string-array const / RFC 8785；production `METHOD_VERSION_BINDINGS=[]`。
- 纯领域：`domain-task`（Task/Action 状态图、revision/plan_version）、`domain-policy`（URI/glob/Default Allow/rate-limit draft，Stop Fence/Recovery 独立 Blocked）。
- 持久化：`kernel-sqlite` migration 0001–0004；Audit canonical Store；legacy Task create/get（**待删除**）；版本化统一 Outbox + mixed v1/v2 append/read + 严格 stored decoder + savepoint poison（legacy append **待删除**）；**active root TaskCreate v2 repository**（`create_root_task_v2`）。
- KCP 库级：`kernel-kcp` retained v1 Value preflight、三方法 registration/dispatcher/handler（`system.ping` / legacy `task.create` / `task.get`）与 SQLite adapter；不可连接，无 bytes/frame/server。
- ADR-0006 首批：12 business-v2 Schema + `kernel-task-creation` pure library + official fixtures/harness + schema-tool strict pointer CLI。
- ADR-0008 前两段：Event v2 八 Schema、`EventTypeBinding`/active·legacy catalog、typed EventEnvelope v1/v2、migration 0003 descriptor v1 与 mixed Outbox API。
- V2InitialBuildActive切片1a–1c-ii：root 持久对象、Action/child 授权、授权核心、身份/挑战/证据 Schema 与 pure crate 已落地（manifest=83）。
- V2InitialBuildActive切片2：migration 0004（`content_origins_v2`、`task_creation_provenances`、`audit_records_v2`、`root_task_create_idempotency_v2` + tasks/scope FK 重建以允许 v2 origin）；`WriteTransaction::create_root_task_v2` 单事务写 Origin/Scope/Task/Provenance/Audit/idempotency/Event；全闭包 canonical readback（Created/Replayed 共用）；幂等重放/冲突；回滚不占号；与 v1 表互不污染。

**未实现（不得宣称完成）**

- method-aware payload version preflight；active `task.create` 只接受 2；替换并删除 registered v1 create。
- KCP root v2 handler + production MethodVersionBindings。
- child Action materializer；Action/PermissionDecision/Approval v2 repositories。
- 删除 v1 runtime 写路径与旧库 `reinitialize-required` 启动拒绝。
- 其它 active business **producer**（child/action/approval）；**Publisher** 与 versioned KCP **poll** 明确不在本里程碑。
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
- [ ] 切片 3：method-aware KCP v2 + production bindings
- [ ] 切片 4：Action/PD/Approval/Identity repository
- [ ] 切片 5：child materializer
- [ ] 切片 6：删除 v1 写路径 + 旧库拒绝 + §13.7 收口
- [ ] `ActionTransitionIntentV1` + Action 持久对象/transition repository
- [ ] active Event business producers（child/action/approval；root 已在切片 2 接入）
- [ ] 五方法正式 handler；可连接 KCP server/client；`agentd`
- [ ] 其它 Command 幂等与乐观锁；Task list cursor ADR 拍板后实现
- [ ] Publisher + versioned KCP poll（**后续里程碑**，不在 V2InitialBuildActive）
- [ ] Extension SDK Base → `schema/SDK`；TS 包 / SDK client / `agent-runtime`
- [ ] optional Computer Use Profile；桌面客户端；Provider/Memory/Initiative/Broker
- [ ] CONFORMANCE 全量 BASE + 声明 Profile 套件

## 当前阻塞

- active contract 初始构建：v1 `task.create` 须退出并删除 production 写路径；v2 handler 与 method-aware preflight 完成前禁止启动 server。
- legacy v1 repository 的 Delegation 正向路径未实现（非 null 固定 not found）；active v2 repository 同样在 Delegation authority 未落地前 fail-closed 返回 `delegation_not_found`。
- Task list cursor 编码须先 ADR/API 拍板。
- Audit 仍缺跨对象 PermissionDecision/policy context 相等、rollback 权威投影、Provider/ModelCall 一致性与其它业务 producer。
- Extension SDK Base / Computer Use 仍为 `contract-only`；不得把规范或根 Node 工作区冒充 SDK 实现。
- 真实 Provider/Channel/Privilege Broker 需要后续真实环境；当前无伪造支持。
- 默认 PATH 可能不是 Node 24.18.0；须显式 `~/.local/share/pnpm`。

## 下一步

1. 切片 3：method-aware preflight、root v2 handler 与 production bindings。
2. 切片 4–5：Action/PD/Approval/Identity repositories 与 child materializer；接入其余有 owner 的 active Event producers。
3. 切片 6：删除全部 v1 runtime 写路径，落地旧库 `reinitialize-required`，闭合 §13.7。
4. **之后**再做 Publisher 与 versioned KCP poll；再实现剩余五个 Catalog handler 与可连接 server。
5. 随后实现 Extension SDK Base 与 TypeScript/client。

## 最近验证

本切片（V2InitialBuildActive 2）验证命令：

```text
export PATH="$HOME/.local/share/pnpm:$PATH"
export TMPDIR=/mnt/data/shittim-build-tmp
export CARGO_TARGET_DIR=/mnt/data/shittim-cargo-target
cargo test --manifest-path rust/Cargo.toml -p kernel-sqlite
cargo test --manifest-path rust/Cargo.toml --workspace
cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets -- -D warnings
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
./scripts/check-schema.sh
node scripts/update-file-manifest.mjs --check
git diff --check
```
