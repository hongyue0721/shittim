# Shittim 实现进度

> 状态日期：2026-07-20（`V2InitialBuildActive`切片1b完成：新增ActionTransitionIntentV1、ActionRequestV2、ChildTaskDeltaProjectionV1、MaterialAuthorizationProjectionV1、ObservationEvidenceProjectionV1；manifest=70（41 retained + 29 component-native），generated Rust、typed conformance、`kernel-authorization` pure crate与authorization projection official fixtures/harness/oracle已落地。production MethodVersionBindings仍为空；repository/handler/materializer/producer未实现。）

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
| 1b | Action/child Schema：ActionTransitionIntentV1、ActionRequestV2、Delta/Material/Observation projection + `kernel-authorization` pure crate | **已完成**（SubjectProjection与ApprovalEventAllocation留1c） |
| 2 | fresh SQLite 基线 + root repository | 未开始 |
| 3 | method-aware KCP v2 preflight/dispatcher/handler + production bindings | 未开始 |
| 4 | Action / PD / Approval / Identity repository | 未开始 |
| 5 | child Action materializer | 未开始 |
| 6 | 删除 v1 写路径收口 + 旧库拒绝 + §13.7 谓词闭合 | 未开始 |

**已实现（代码/Schema 事实）**

- 规范与工程基线：Freedom-first / Kernel Owns Reality 合同、Apache-2.0、双仓同步 library/CLI、Node/pnpm 零依赖根基座（exact Node 24.18.0 / pnpm 11.3.0）、统一门 `scripts/check-schema.sh`。
- Schema/Rust 契约：Rust workspace、Draft 2020-12 + manifest v2（production=70 entries，41 retained + 29 component-native）、`schema-tool` 单 root transaction / target-scoped IR / TaggedUnion / string enum `ALL` / RFC 8785；production `METHOD_VERSION_BINDINGS=[]`。
- 纯领域：`domain-task`（Task/Action 状态图、revision/plan_version）、`domain-policy`（URI/glob/Default Allow/rate-limit draft，Stop Fence/Recovery 独立 Blocked）。
- 持久化：`kernel-sqlite` migration 0001–0003；Audit canonical Store；legacy Task create/get（**待删除**）；版本化统一 Outbox + mixed v1/v2 append/read + 严格 stored decoder + savepoint poison（legacy append **待删除**）。
- KCP 库级：`kernel-kcp` retained v1 Value preflight、三方法 registration/dispatcher/handler（`system.ping` / legacy `task.create` / `task.get`）与 SQLite adapter；不可连接，无 bytes/frame/server。
- ADR-0006 首批：12 business-v2 Schema + `kernel-task-creation` pure library + official fixtures/harness + schema-tool strict pointer CLI；**未**接 repository/handler。
- ADR-0008 前两段：Event v2 八 Schema、`EventTypeBinding`/active·legacy catalog、typed EventEnvelope v1/v2、migration 0003 descriptor v1 与 mixed Outbox API。
- V2InitialBuildActive切片1b：ActionTransitionIntentV1、ActionRequestV2与ChildTaskDelta/MaterialAuthorization/ObservationEvidence三projection source/manifest/generated Rust/typed round-trip/conformance已落地；`kernel-authorization` pure crate负责typed authoritative facts→构造/验证/JCS/SHA-256，不读repo、不分配ID、不写存储、不替代matcher。IC §5.3.1要求的四份authorization projection official fixtures（child delta、material、observation not_applicable、observation observed）与production-owner harness、schema-tool CLI oracle已落地，wrapper不进manifest。retained VerificationResultV1满足child materialization验证引用，未新造版本；SubjectProjection与ApprovalEventAllocation留1c。

**未实现（不得宣称完成）**

- `ActionTransitionIntentV1` repository、Action transition migration/producer（Schema与generated type已完成）。
- active Action repository：`approval_chain_id`、execution generation、materialized child result、Approval v2 消费。
- 其余 v2 对象：SubjectProjection、ApprovalEventAllocation、ApprovalRecord/PermissionDecision/PolicyRule v2、signature/credential/challenge/evidence 等。
- method-aware payload version preflight；active `task.create` 只接受 2；替换并删除 registered v1 create。
- root v2 repository/handler、child Action materializer。
- Action/PermissionDecision/Approval v2 repositories、current-head CAS、fingerprint 失效/复用、身份 challenge、plan 重评。
- 删除 v1 runtime 写路径与旧库 `reinitialize-required` 启动拒绝。
- active business **producer**（root/child/action/approval）；**Publisher** 与 versioned KCP **poll** 明确不在本里程碑。
- 其余 Command 幂等/乐观锁；**五方法**正式 handler；Unix Domain Socket / Windows Named Pipe KCP **server/client**；**agentd**。
- `ts/*` 包、SDK client、Pi `agent-runtime`；统一 **Extension SDK Base**；optional Computer Use Profile；Tauri 桌面客户端；Provider/Memory/Initiative/Broker。
- `specs/CONFORMANCE.md` 全部 BASE 与声明的 Profile/Platform 条件套件。

`domain-task` / `domain-policy` 只计算意图与 draft；`kernel-sqlite` 不伪造尚无权威表的跨对象一致性。Extension SDK Base 完成不要求任何 Profile 的 `provider contract` / `real-platform`；`desktop-client` 不等同 Computer Use。

## 未完成 backlog

- [x] 切片 0：规范文档落锤（ADR-0009 等）
- [x] 切片 1a：root创建路径持久对象Schema/manifest/generated/conformance
- [x] 切片 1b：Action/child五Schema + `kernel-authorization` pure crate + projection official fixtures/harness/oracle
- [ ] 切片 1c：SubjectProjection、ApprovalEventAllocation与Approval/PD/auth剩余Schema
- [ ] 切片 2：fresh SQLite 基线 + root repository
- [ ] 切片 3：method-aware KCP v2 + production bindings
- [ ] 切片 4：Action/PD/Approval/Identity repository
- [ ] 切片 5：child materializer
- [ ] 切片 6：删除 v1 写路径 + 旧库拒绝 + §13.7 收口
- [ ] `ActionTransitionIntentV1` + Action 持久对象/transition repository
- [ ] active Event business producers（在切片 2–5 内按 owner 接入）
- [ ] 五方法正式 handler；可连接 KCP server/client；`agentd`
- [ ] 其它 Command 幂等与乐观锁；Task list cursor ADR 拍板后实现
- [ ] Publisher + versioned KCP poll（**后续里程碑**，不在 V2InitialBuildActive）
- [ ] Extension SDK Base → `schema/SDK`；TS 包 / SDK client / `agent-runtime`
- [ ] optional Computer Use Profile；桌面客户端；Provider/Memory/Initiative/Broker
- [ ] CONFORMANCE 全量 BASE + 声明 Profile 套件

## 当前阻塞

- active contract 初始构建：v1 `task.create` 须退出并删除 production 写路径；v2 Schema/handler/repository 与 method-aware preflight 完成前禁止启动 server。
- legacy v1 repository 的 Delegation 正向路径未实现（非 null 固定 not found）；active v2 必须实现真实 Delegation authority。
- Task list cursor 编码须先 ADR/API 拍板。
- Audit 仍缺跨对象 PermissionDecision/policy context 相等、rollback 权威投影、Provider/ModelCall 一致性与其它业务 producer。
- Extension SDK Base / Computer Use 仍为 `contract-only`；不得把规范或根 Node 工作区冒充 SDK 实现。
- 真实 Provider/Channel/Privilege Broker 需要后续真实环境；当前无伪造支持。
- 默认 PATH 可能不是 Node 24.18.0；须显式 `~/.local/share/pnpm`。

## 下一步

1. 进入切片1c，完成SubjectProjection、ApprovalEventAllocation与Approval/PermissionDecision/auth剩余Schema；切片1b不实现repository/handler/producer。
2. 切片 2–3：fresh SQLite 基线、root v2 repository/handler、method-aware preflight 与 production bindings。
3. 切片 4–5：Action/PD/Approval repositories 与 child materializer；接入有 owner 的 active Event producers。
4. 切片 6：删除全部 v1 runtime 写路径，落地旧库 `reinitialize-required`，闭合 §13.7。
5. **之后**再做 Publisher 与 versioned KCP poll；再实现剩余五个 Catalog handler 与可连接 server。
6. 随后实现 Extension SDK Base 与 TypeScript/client。

## 最近验证

本切片（V2InitialBuildActive 1a）验证命令：

```text
export PATH="$HOME/.local/share/pnpm:$PATH"
export TMPDIR=/mnt/data/shittim-build-tmp
export CARGO_TARGET_DIR=/mnt/data/shittim-cargo-target
cargo test --manifest-path rust/Cargo.toml -p kernel-contracts
cargo test --manifest-path rust/Cargo.toml -p schema-tool
cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets -- -D warnings
./scripts/check-schema.sh
node scripts/update-file-manifest.mjs --check
git diff --check
```

上述门禁全部通过；`check-schema.sh`汇总624 passed、0 failed、1 ignored（helper entrypoint）。

## 事实来源

- 域状态表：[`IMPLEMENTATION_MATRIX.md`](IMPLEMENTATION_MATRIX.md)
- 全局不变量：[`../AGENT.md`](../AGENT.md)
- 状态机与恢复：[`../specs/CORE_ARCHITECTURE.md`](../specs/CORE_ARCHITECTURE.md)
- 实现契约：[`../specs/IMPLEMENTATION_CONTRACTS.md`](../specs/IMPLEMENTATION_CONTRACTS.md)
- 验收：[`../specs/CONFORMANCE.md`](../specs/CONFORMANCE.md)
- API 导航：[`api/README.md`](api/README.md)
- ADR 索引：[`../adr/README.md`](../adr/README.md)
