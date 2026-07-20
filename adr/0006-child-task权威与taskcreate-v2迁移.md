# ADR-0006：Child Task 权威与 TaskCreate v2 迁移

- 状态：accepted
- 日期：2026-07-18
- 实现状态徽章：**partial** — 唯一进度源：[`docs/IMPLEMENTATION_MATRIX.md`](../docs/IMPLEMENTATION_MATRIX.md) · [`docs/PROGRESS.md`](../docs/PROGRESS.md)
- **部分被 [ADR-0009](0009-v2从零构建并取消v1数据迁移.md) superseded**：§7 Legacy 读取与迁移、KCP 段中 `V2ProductionWriteCutover` 启用叙述、验收第 6 条（legacy direct-child provenance）的生产效力作废；root-only v2 / child Action 入口 / materialization 决策仍有效。

## 背景

现有 `TaskCreateRequestV1` 同时允许 `parent_task_id = null` 与非 null，因此同一个公开 KCP Command 既能创建根 Task，也能直接创建子 Task。与此同时，旧规范又写过“Child Task创建本身是一个Action”。这造成两个生产入口、两个因果模型和无法闭合的权限事实：直接 KCP 创建子 Task 时，没有一个父 Task 所拥有的 Action 可以承载 Policy、Lease、Stop Fence、Verification、重放与恢复。

仓库当前已经实现 v1 `task.create` typed handler 与 SQLite create/get repository。该实现尚未发布为可连接 server，因此不为保持未发布实现而固化双入口。历史或测试数据库中已经存在的 v1 direct-child 记录仍是合法事实，但不能通过迁移伪造过去并不存在的 Action、Approval 或认证证据。

## 决策

### 1. 唯一生产入口

- active KCP `task.create` 切换到 `TaskCreateRequestV2`，只创建根 Task。
- v2 payload 不包含 `parent_task_id`；Envelope `task_id` 与 `expected_revision` 必须为 `null`。
- `TaskCreateRequestV1`永久冻结为legacy-validation-only合同（仅Schema/fixture历史验证资产，无production read/write/migration）；manifest compatibility现已标为`legacy-validation-only`，不再进入active dispatcher，也不接受production write。
- `TaskSpec.parent_task_id` 继续存在，作为持久父子关系的唯一字段；根 Task 为 `null`，新 child Task 必须等于创建它的父 Action 的 `task_id`。

### 2. Child Task 的唯一新写入口

父 Task 拥有一个 Kernel-local Action：

```text
capability_id = "kernel.task"
operation = "task.child.create"
side_effect_class = S1
external_side_effect = none
```

Action 的 `structured_arguments` 使用版本化 `ChildTaskProposalV1`，完整声明 child 的 proposer、goal、constraints、success criteria、risk/capability hints、正式独立的`InputTaskScopeV1`、显式 Delegation 选择和`InputContentOriginV1`。raw root/child与normalized root/child的caller-owned字段完全同构；canonical字段合同宿主固定为既有root `NormalizedRootTaskCreatePayloadV2#/$defs`，宿主properties用local fragment，其余三root用absolute fragment。这是中立task-create proposal字段宿主，不倒置root/child业务语义；禁止第13个Schema或复制平行约束。所有string数组可空、元素出现时non-empty、保序保重复且无`uniqueItems`；非null `risk_hint`与`upstream_stable_id`也non-empty，普通字符串不trim。调用方不得提供 child Task/Scope/Origin/Receipt/Audit/Event/Verification ID，不得提供 `parent_task_id`、status、plan version、revision、created/updated time或 Action completion 字段。

父关系只由 `ActionRequest.task_id` 确定。任何 arguments 内同义字段、嵌套 shadow 字段或通过开放 JSON 伪装的 parent/id/state 字段都必须在 proposal Schema/producer validation 中拒绝。

root v2与child proposal的正式生产normalization owner现为纯crate `kernel-task-creation`。本阶段范围只含root/child proposal normalize、root receipt/idempotency projection与hash、child proposal/receipt hash、root/child allocation validation；`ChildTaskDeltaProjectionV1`、`MaterialAuthorizationProjectionV1`、`ObservationEvidenceProjectionV1`未来由专门authorization projection owner或独立切片实现，不得塞入task creation。依赖方向是`kernel-contracts + domain-policy -> kernel-task-creation -> kernel-sqlite（未来调用方）`；全部repository事实由caller typed input注入，crate不读repo、不分配ID、不写存储，也不依赖SQLite/KCP。URI parser/normalizer继续以`domain-policy`为当前唯一权威，task creation只编排调用、不得复制；长期抽取中立URI crate须另立ADR，当前不制造第二实现。`domain-task`继续保持纯状态机，不新增policy依赖。该crate已实现pure library但尚未接入repository/handler。

### 3. 直接因果与版本升级

`CausationRef v2`是判别联合：`command_request{id}`、`event{id}`、`action{id}`、`action_transition{action_id,transition_id}`。新child的`task.created`直接使用：

```text
{ kind: "action", id: <child-create Action id> }
```

Action自身正式状态事件使用`action_transition`锚点而不是Action ID，避免aggregate为该Action时出现self-causation。不得为了复用v1因果类型而伪造一条`action.requested` Event。使用该引用的`EventEnvelope`、`ContentOrigin` carrier、`AuditRecord`及相关payload按各自v2合同升级；v1记录继续只读（历史：v1 读写义务已被 ADR-0009 supersede，v1 仅保留 Schema/fixture 历史验证资产，无 production store read/write/migration API）。

### 4. Scope、Capability 与 Delegation

- Child TaskScope 必须在 proposal 中完整显式声明；不自动继承父 Scope，不自动做父子交集，也不把父 Scope 复制成默认值。
- Child `delegation_ref` 字段必须显式出现，值可以是 `null`。非 null 时必须由 Delegation authority repository 验证存在、current、active、未过期，并验证该 delegation 可用于本次 child proposal。
- Kernel 计算父子资源 scope、capability 与 delegation delta，并把完整 delta 放进 Policy evaluation context。
- scope/capability 扩大不是 Kernel hard deny；它由 Freedom-first Policy 按当前事实裁决。合同缺失、非法 URI、无法验证 Delegation authority 则是事实/合同错误，不进入 Default Allow。

### 5. 原子 materialization

`task.child.create` 是无外部副作用的 Kernel-local S1 业务事实创建。执行前必须证明：

- Action 属于存在的父 Task，operation/capability/class 精确匹配；
- Action 当前 revision 等于 expected revision；
- 当前 PermissionDecision/必要 Approval 有效；
- Action 处于可执行的 leased generation，Lease 未过期且绑定正确；
- Stop Fence 未激活；
- proposal、scope、delegation authority 与父子 delta 已完成验证。

一次 `BEGIN IMMEDIATE` 事务消费正式`ChildTaskMaterializationAllocationV1`：自身required `schema_version=1`，另有十个UUID与三个non-empty opaque值；schema_version不计入十UUID。Schema不伪装验证跨字段互异/外部ID不相等/opaque独立；未来validator接收闭合typed `ChildTaskMaterializationExternalUuidRefsV1`，required槽为parent Task、Action、current PermissionDecision，optional Approval resolution/Delegation，Credential/Challenge与parent origins使用Vec完整注入，不接受自由UUID bag。事务写入child ContentOrigin、TaskScope、Task、创建 provenance、VerificationResult、AuditRecord、`task.created` 与 `action.state_changed` Outbox、Action 结果与 completion revision。child事件以Action为直接原因；Action自身事件以预持久化`action_transition{action_id,transition_id}`为原因，禁止self-causation。事务内执行 canonical readback verification；任一引用、Schema、hash、canonical、关系镜像、Event/Audit/Verification 或 Action completion 不一致，全部回滚。

一个 Action 在其整个生命周期最多物化一个 child。`action_id` 是业务唯一键；execution generation 与 Action idempotency key用于证明重放属于同一执行，不允许新 generation 产生第二个 child。

### 6. 重放与 reconciliation

- commit 成功但响应丢失：以同一 Action ID/generation/idempotency 重放，读取并 canonical verify 已存在 child，返回原 child，不创建新事实。
- 事务未 commit：不存在 child/materialization marker；Lease 到期后按普通 Kernel-local Action 恢复，可重新获取新 generation 执行。
- 发现 child mapping 但 Action completion、Verification、Audit 或规定 Outbox 不完整，属于 `stored_data_invalid` / Safe Recovery；不得“补齐一半”或创建第二个 child。
- reconcile 只可依据已提交 canonical materialization bundle 判断 committed/absent/corrupt，不从 UI、模型文本或 Event delivery 状态猜测。

### 7. Legacy 读取与迁移

迁移必须为每个 Task 提供可查询的 creation provenance：

- `root_command_v2`；
- `child_action_v2`；
- `legacy_direct_create_v1`；
- `imported_legacy`（仅有明确导入事实时）。

（历史：本节 provenance 四分支与迁移义务已被 ADR-0009 supersede，生产仅保留 `root_command_v2`/`child_action_v2` 两支，无 v1 数据迁移。）

已有 `parent_task_id != null` 且由 v1 command 直接创建的 Task 标记 `legacy_direct_create_v1`，继续合法读取、list、归档和恢复（历史：该读取义务同样已被 ADR-0009 supersede）。迁移不得创建虚假的 Action、Approval、PermissionDecision、Verification 或认证证据；未知事实保持未知。

## KCP 与兼容策略

KCP protocol 仍可为 `1.0`，但 payload version preflight 必须 method-aware：

- `task.create` active request version仅`2`、legacy validation version仅`1`；`system.ping`、`task.get`、`task.list`、`event.subscribe`、`event.poll`、`stop.activate`、`stop.status` active request version均为`1`且无legacy validation version；
- manifest生成的`MethodVersionBinding`完整规则以IC §13.5为准：通用validator在Schema/工具阶段接受合法非空synthetic 8-method registry；expected set直接从registry中唯一匹配的V2 Envelope facts派生，不按ID suffix、不读取generated catalog。production manifest由`validate_production_manifest_stage`在check/generate入口保持空，synthetic registry不走该stage gate；最终启用时机已由 ADR-0009 改为 V2InitialBuildActive（历史：原文为 V2ProductionWriteCutover）。binding与entry compatibility正交；response按family+method+request version选择；
- `task.create` version `1` 是 known legacy，但 active KCP 返回 `unsupported_schema_version`，不 narrow 到 registered handler；
- 其他方法也必须读取各自 binding，不能继续全局硬编码“所有 payload 都只允许 1”。

`TaskCreateRequestV2`的receipt hash与带`schema_version=1`的`RootTaskCreateIdempotencyProjectionV1`均以规范化后的v2 payload为输入；不得把v1 `parent_task_id`的null占位带入v2 hash，projection的schema_version参与JCS/hash。`InputTaskScopeV1.expires_at`现行source Schema固定使用`format: date-time`与完整文本pattern双门：`^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-5][0-9](?:\.0+)?(?:Z|[+-][0-9]{2}:[0-9]{2})$`；pattern固定大写T/Z、offset形状、秒范围及零fraction，format负责真实日期/时间/offset合法性，不用custom format。非零亚秒caller-invalid且禁止截断/四舍五入，合法值解析instant后输出UTC秒精度。`origin.source_uri`、resource pattern与exclusion normalization失败统一公开为`invalid_scope_pattern`并携带各自input_kind/index；post-normalize Schema/typed失败为internal contract failure。`TaskCreateRequestV2`、`TaskCreateResponseV2`与`KcpCommandEnvelopeV2`使用component-native ID；Response中的`task`明确引用当前active retained TaskSpec v1，不虚构TaskSpec v2。KcpCommandEnvelopeV2仍为protocol 1.0，结构验证与method payload业务验证分成Envelope后binding两阶段；retained v1 Envelope不修改。精确字段在 `IMPLEMENTATION_CONTRACTS.md` 定义。

## 拒绝的替代方案

- **保留 v1 root/child 双入口**：拒绝；会永久保留两个权限与因果模型。
- **KCP 先创建 child，再补一条 Action**：拒绝；Action 不是因果权威且事务边界倒置。
- **用 `action.requested` Event 作为 child 直接原因**：拒绝；Event 只是 Action 的派生事实，不是实际执行主体。
- **自动继承或求交父 Scope/Delegation**：拒绝；会把未声明策略伪装成事实，且丢失扩大意图。
- **为历史 direct-child 伪造 Action/Approval**：拒绝；违反审计真实性。

## 实现影响

后续实现必须在已落地的`kernel-task-creation` pure library之上新增TaskCreate v2 handler、child materialization repository、Action/PermissionDecision/Approval repositories、CausationRef/EventEnvelope/ContentOrigin/Audit v2、migration/provenance 与 Conformance（历史：migration/provenance 的 v1 数据迁移部分已被 ADR-0009 supersede，实际仅 root_command_v2/child_action_v2 两支 provenance），不得复制helper语义。official测试制品路径固定为root `schemas/fixtures/kcp/task_create_normalized_hash.v2.json`、child `schemas/fixtures/task/child_task_proposal_normalized_hash.v1.json`、allocation合并 `schemas/fixtures/task/task_creation_allocations.v1.json`；wrapper不是business Schema。现有 v1 Rust/Schema/SQLite 事实不因本 ADR 自动改变，当前 production dispatcher 仍未达到本决策。

## 验收

1. active KCP 不能以任何 v1/v2 `task.create` payload直接创建 child；
2. 所有新 child 都能沿 `Task.parent_task_id -> parent Task -> child-create Action -> PermissionDecision/必要 Approval -> Verification/Audit/Event` 闭合；
3. child `task.created` causation 是 Action，不是伪造 Event；
4. 扩大 scope/capability 进入 Policy context而不是 hard deny，Delegation authority 缺失则明确失败；
5. 故障注入证明 bundle 全有或全无、同 Action最多一个 child；
6. （历史：本条已被 ADR-0009 supersede，无 legacy direct-child 生产读取义务）legacy direct-child 可读且明确标记 provenance，不伪造历史事实。
