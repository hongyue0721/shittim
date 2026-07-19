# CORE_ARCHITECTURE.md

> 本文件是运行时拓扑、状态所有权、任务、Action、事件、恢复、并发与 Stop Fence 的唯一事实源。

## 1. 架构目标

系统必须同时满足：

- UI 可关闭和重连；
- 模型 Runtime 可崩溃和重启；
- Extension 可按需启动；
- 可选 Profile 可以完全未安装，Core 仍可完整启动并提供非该 Profile 的全部能力；
- Provider 可部分缺失；
- 任务状态不依赖某个模型会话；
- 权限不依赖 Prompt；
- 平台差异不进入 Kernel Domain；
- Core 不理解 Display、Window、Screenshot、Coordinate、Desktop generation、OperationSnapshot 或 Input Session Lease 等 Profile 私有对象；
- 远程 Channel 不直接接触能力层；
- 故障后不会重复执行不可逆动作。

## 2. 部署单元

### 2.1 常驻

#### agentd

唯一控制面和事实权威。

#### agent-runtime

模型、Pi 和角色运行时。可以重启，不拥有现实状态。

### 2.2 非常驻与可选部署

#### Client

本地桌面客户端、Web 客户端或其他内部客户端都按各自界面生命周期存在。`desktop-client` 是可选视图实现，不是 Core 启动依赖；UI 关闭、未安装或不支持某个 Profile 时不影响 `agentd` 与 Task 真相。

#### Extension/Provider Process

只有能力被使用、订阅或配置为后台入口时存在。

#### Privilege Broker

只在 S5 动作期间存在，或由系统以按需服务形式激活。

## 3. 领域模块与进程不是同义词

以下是 `agentd` 内部领域模块，不是独立服务：

- Task Engine；
- Policy Engine；
- Delegation Engine；
- Memory Domain；
- Initiative System；
- Extension Supervisor；
- Provider Registry；
- Audit Store；
- Object Registry；
- Client Session Manager。

禁止因为模块名称创建对应微服务。

## 4. 状态所有权

| 状态 | 唯一所有者 |
|---|---|
| Task/Step/Action 状态 | Task Engine |
| 权限判定与授权租约 | Policy Engine |
| Delegation Contract | Delegation Engine |
| 有效记忆与来源 | Memory Domain |
| Initiative Candidate | Initiative System |
| 扩展安装与进程状态 | Extension Supervisor |
| 已声明、协商且 probe 得到的能力档案 | Provider Registry |
| Profile 私有观察、结果、句柄与会话状态 | 对应 Extension/Profile 实现；不是 Kernel Domain 事实 |
| Prompt/模型会话临时状态 | agent-runtime |
| UI 视图状态 | 对应 client |
| 特权执行内部状态 | Privilege Broker，仅动作期间 |

Provider 可以缓存本地对象，但其缓存不是 Kernel 事实。Profile 私有对象只能通过通用 Object Handle、stable resource ref 或已协商的 typed Extension result 被 Kernel 引用；Core 不复制其领域模型。未来 Extension Event 必须先有独立正式合同，当前不能用“result/event”并称来暗示事件边界已存在。能力未声明、未协商或 probe 未通过时，Registry 必须诚实记录缺失或不可用，不得合成占位能力。

## 5. 启动

### 5.1 agentd 冷启动

顺序：

1. 确认单实例和数据目录权限；
2. 打开 SQLite，检查 Schema；
3. 恢复未完成事务和 Action Lease；
4. 重建 Task 状态、通用资源锁与 Stop Fence；
5. 加载 Extension Registry，但不启动所有扩展；可选 Profile 缺失不得阻止 Core 启动；
6. 启动本地认证 IPC；
7. 启动或连接 agent-runtime；
8. 恢复允许后台运行的 Channel；
9. 将无法安全恢复的任务置为 `waiting_user` 或更新 `failed_recovery_meta`。

### 5.2 Provider 按需启动

触发条件：

- Task 需要某能力；
- 用户打开诊断或管理页面；
- Channel 被启用；
- 后台委托需要订阅事件；
- Provider 被配置为常驻且策略允许。

按需启动流程：

1. 解析 Extension Manifest 中声明的 Profile/version 与 Task 的能力需求；
2. 从已安装实现中选择协议和 Profile 兼容候选；
3. 启动隔离进程；
4. 完成 SDK 协议握手与 Profile/version negotiation；
5. 执行 capability discovery/probe，以实际结果而非 Manifest 声明建立能力档案；
6. 注册到 Provider Registry；
7. 向等待中的 Task 返回可用性。

没有兼容声明时返回 `unsupported`；已支持但当前 health/backend/session 丢失时返回 `unavailable`；SDK/Profile/operation Schema 版本无兼容交集时返回 `incompatible`；能力已发现但当前调用未获 grant 时返回 `capability_not_granted`。它们不是 Core 启动错误，也不能由 Kernel、Planner 或 UI 伪造为可用。

## 6. agent-runtime 生命周期与重连

agent-runtime 是 agentd 的子进程，由 agentd 管理其生命周期。

### 6.1 启动

1. agentd 在冷启动步骤 7 中 spawn agent-runtime；
2. agent-runtime 通过 Kernel Control Protocol 连接 agentd；
3. 完成协议握手，声明自身版本和能力；
4. agentd 下发当前活跃 session 列表供重建 Context Pack。

### 6.2 崩溃与重启

- agentd 检测到 agent-runtime 退出或心跳丢失；
- 取消 agent-runtime 上所有未完成模型调用；
- Task 状态保留在 agentd，不受影响；
- agentd 按配置重试策略重启 agent-runtime（默认立即重启一次，之后退避）；
- 重启后 agent-runtime 从 agentd 拉取活跃 Task 重建最小 Context Pack；
- Provider 副作用不因 agent-runtime 重启而重放。

### 6.3 重连

- agent-runtime 每次连接都是全新会话；
- 旧会话上的模型调用 Promise 被取消；
- 新会话重新加载 Agent Identity、Persona、活跃 Task 与相关 Memory；
- 用户通过任一 client 看到的是 agentd 持有的 Task 真相，不感知 agent-runtime 重启。

### 6.4 多实例

- 同一 agentd 同一时刻只允许一个 agent-runtime 连接；
- 新连接到达时，旧连接的未完成调用被取消；
- 此约束防止同一 Task 的模型推理分叉。

## 7. 关闭

`agentd` 关闭前必须：

- 停止接受新副作用；
- 取消或暂停未完成调用；
- 将不可中断动作标记为恢复待查；
- 刷新 Task/Audit；
- 撤销临时权限租约；
- 通知扩展优雅退出；
- 强制终止超时扩展；
- 终止 agent-runtime；
- 最后关闭数据库。

UI 退出不触发上述流程。

## 8. 命令、查询和事件

### 8.1 Command

请求改变状态，具体 Envelope、Actor、EntryPoint、auth、deadline 与版本字段由 `IMPLEMENTATION_CONTRACTS.md` 第 5 节定义。所有 Command 至少具备：

- `request_id`：客户端生成，用于去重和关联响应；
- `actor`：发起者身份；Actor 不重复携带 EntryPoint；
- `entry_point`：Envelope 上的入口点；
- `task_id` / `context`：所属 Task 或上下文；
- `deadline`：过期时间；Kernel 收到时已过期必须返回 `deadline_exceeded`。处理期间超过 deadline 时取消可取消工作并返回同一错误；对于已开始且不能安全取消的外部动作，不得宣称已取消或回滚，必须持久化为 `unknown_side_effect`/恢复待查后返回 `deadline_exceeded`；不得静默丢弃。`task.create` 的本地 SQLite 事务不属于外部动作，但同样不可中途取消；其 commit 后到期返回错误而事实保留，按 `IMPLEMENTATION_CONTRACTS.md` §5.10 使用同一幂等键恢复；
- `idempotency_key`：客户端生成的幂等键，范围由命令语义定义；
- `expected_revision`：需要并发控制时携带对象当前 revision。

### 8.2 Command 幂等范围

- 同一 `idempotency_key` 在 Kernel 内只生效一次；
- 幂等窗口由对象类型和命令语义决定（Task 创建窗口 = 创建中 + 已创建去重查；Action 提交窗口 = 提交中 + 已完成去重查）；
- 重复命令返回原始结果引用，不重新执行；
- 幂等键过期后可被清理，不在该窗口内的重复视为新命令。

### 8.3 Query

只读查询，不得产生副作用。

### 8.4 Event

描述已经发生的事实。Event 不可被处理者修改。

禁止用 Event 伪装 Command，也禁止因收到 Provider Event 就自动授权副作用。

### 8.5 EventEnvelope

> active EventEnvelope v2与版本化统一Outbox当前为contract-only；exact Schema/Catalog/migration合同见`IMPLEMENTATION_CONTRACTS.md` §5.6、§6.14–6.15、§13.6.2及ADR-0008。现有Rust/SQLite仍只实现legacy v1。

所有持久化事件包装在 EventEnvelope 中。`sequence` 只表达同一聚合内的领域顺序：某聚合第一条**已提交**事件固定为 `0`，此后每条已提交事件必须严格等于上一条已提交事件的 `sequence + 1`。事务中暂时分配但最终回滚的序号不构成已提交事实、不得占号；重试事务必须基于当前最后已提交序号重新分配。`outbox_position` 是事件写入 Outbox 时由 Kernel 分配的全局单调递增投递位置，只用于发布、分页和 cursor，不代表跨聚合因果顺序。

| 字段 | 说明 |
|---|---|
| `event_id` | 全局唯一事件标识 |
| `type` | 点号分隔的小写事件类型（如 `task.state_changed`） |
| `schema_version` | EventEnvelope 持久对象的 schema 版本；`payload` 另带自身 schema_version |
| `aggregate_type` | 聚合类型（如 `task`、`stop_fence`） |
| `aggregate_id` | 聚合根 ID（如 `task_id`） |
| `sequence` | 聚合内已提交事件序号；首条为 `0`，此后严格连续 `+1`，回滚事务不占号 |
| `outbox_position` | Outbox 全局单调递增投递位置；不是领域事件因果顺序 |
| `occurred_at` | 事件发生时间（Kernel 时钟） |
| `causation_ref` | 直接原因引用。v2 是 tagged union：`command_request{id}`、`event{id}`、`action{id}`、`action_transition{action_id,transition_id}`。`action`表示该Action直接产生其他聚合事实；Action自身状态事件必须使用预持久化transition anchor，禁止self-causation。v1历史只允许前两支。 |
| `correlation_id` | 关联 ID，贯通整条因果链 |
| `dedup_key` | 去重键，用于消费者幂等处理 |
| `payload` | 带自身 `schema_version` 的类型化事件体 |

## 9. Task 模型

### 9.1 Task

Task 表示一个可解释的用户目标或 AI 主动目标。

必需字段：

- `id`；
- `origin_ref`；
- `actor`；
- `proposer`（user/companion/system）；
- `goal`；
- `constraints`；
- `success_criteria`；
- `risk_hint`（可空）；
- `capability_hints`（可为空数组）；
- `status`；
- `plan_version`；
- `task_scope_ref`（Task 的临时权限/资源上下文）；
- `delegation_ref`（可空，无委托时为 `null`）；
- `schema_version`；
- `revision`；
- `created_at` / `updated_at`。

### 9.2 Child Task（父子 Task）

Child Task 不是第四种状态机。它就是一个普通 Task，通过 `parent_task_id` 引用父 Task；该字段是持久父子关系的唯一事实。

规则：

- Child Task 拥有独立的 `id`、`status`、完整显式 TaskScope、权限上下文和生命周期；
- active KCP `task.create` v2 只创建根 Task，payload 不接受 `parent_task_id`；`TaskCreateRequest v1` 只保留 legacy validation/read/migration，不进入 active dispatcher；
- 新 Child Task 的唯一写入口是父 Task 拥有的 Kernel-local Action：`capability_id=kernel.task`、`operation=task.child.create`、`side_effect_class=S1`。父关系只由该 Action 的 `task_id` 权威确定；proposal 不得提交 child ID、parent、status、revision、plan/time 等 Kernel-owned 字段；
- Child proposal 必须完整显式声明 Scope 与 `delegation_ref`（可为 null），不得自动继承父 Scope、自动求交或复制父 Delegation；Kernel 计算父子 resource scope、capability 与 delegation delta，并纳入 Policy evaluation context；扩大不是 Kernel hard deny，按 Freedom-first Policy 裁决，但 Delegation authority 缺失或不可验证属于事实错误；
- `task.child.create` 是无外部副作用的 Kernel-local S1 业务事实创建，仍必须满足 current PermissionDecision/必要 Approval、Action revision、Lease、Stop Fence 与 Verification；
- child materialization 在单一原子事务中创建 child Origin/Scope/Task/provenance/Audit/Event/Verification，原子完成 Action，并执行 canonical readback；同一 Action 全生命周期最多一个 child，失败全回滚；
- 新 child 的 `task.created` 使用 CausationRef v2 `{kind:action,id:<创建 Action>}`，不得伪造 `action.requested` Event 充当直接原因；
- 历史 v1 direct-child 是合法可读事实，必须标记 `legacy_direct_create_v1` provenance；不得迁移伪造过去不存在的 Action、Approval、PermissionDecision 或 Verification；
- 父 Task 的 `plan_version` 可以引用子 Task ID 列表；
- 取消父 Task 时子 Task 按各自当前状态执行取消语义，不做级联强制；
- 子 Task 失败不自动导致父 Task 失败；父 Task 的 success criteria 决定是否可接受。

字段、事务、重放、迁移和错误合同以 `IMPLEMENTATION_CONTRACTS.md` §5.5、§6.5.1、§6.10 与 ADR-0006 为准。

### 9.3 Step

Step 是计划中的逻辑工作单元，可以重新规划，不直接等同于 Provider 调用。

### 9.4 Action

Action 是一个具体现实副作用或高成本观察。

Action 必须有：

- `resource target`；
- `side_effect_class`；
- `extension/capability`；
- `structured_arguments`；
- `permission_decision_ref`（初建 `pending` 且尚未评估时可空，首次评估后必须引用不可变 PermissionDecision）；
- `idempotency_key`；
- `verification_policy`；
- `rollback_metadata`（若适用）。

## 10. Task 状态机

### 10.1 TaskStatus 枚举

```text
candidate
awaiting_approval
planned
rejected
running
waiting_user
paused
partially_completed
succeeded
failed
cancelled
rolling_back
rolled_back
archived
```

`failed_recovery_meta` 不是 Task 状态，而是 Task 上的恢复元数据对象，包含 `attempted`、最近尝试时间/原因以及不可变 RecoveryAttemptRef 引用。

### 10.2 合法转换

```text
candidate
  -> awaiting_approval  (需要用户审批)
  -> planned            (策略允许直接规划)
  -> rejected           (策略或用户拒绝)

awaiting_approval
  -> planned            (批准)
  -> rejected           (拒绝)

planned
  -> running            (开始执行)
  -> cancelled          (用户取消)
  -> rejected           (规划被策略否决)

running
  -> waiting_user       (需要用户输入)
  -> paused             (用户或策略暂停)
  -> partially_completed (部分步骤完成，可继续)
  -> succeeded          (全部成功标准满足)
  -> failed             (不可恢复的失败且无需补偿，或补偿不可开始)
  -> cancelled          (取消完成且没有需要补偿的已发生副作用)
  -> rolling_back       (失败或取消处理中发现已发生且需要补偿的外部副作用)

waiting_user
  -> running            (用户响应)
  -> cancelled          (用户取消或超时且无需补偿)
  -> rolling_back       (取消处理中发现需要补偿的外部副作用)

paused
  -> running            (恢复)
  -> cancelled          (取消且无需补偿)
  -> rolling_back       (取消处理中发现需要补偿的外部副作用)

partially_completed
  -> running            (继续剩余步骤)
  -> succeeded          (接受部分结果为最终成功)
  -> failed             (声明失败且无需补偿)
  -> cancelled          (取消且无需补偿)
  -> rolling_back       (已完成部分包含需要补偿的外部副作用)

succeeded
  -> archived

failed
  -> archived
  -> planned            (用户要求以新计划重试；plan_version++)
  -> rolling_back       (恢复编排确认已有副作用需要补偿)

cancelled
  -> archived
  -> rolling_back       (取消完成后恢复编排确认仍有副作用需要补偿)

rolling_back
  -> rolled_back        (所有可回滚 Action 已回滚)
  -> failed             (回滚中遇到不可恢复情况，更新 failed_recovery_meta)

rolled_back
  -> archived
  -> planned            (用户要求以新计划重试)
```

### 10.3 约束

- `succeeded` 只能由 Kernel 根据成功标准和验证结果设置；
- `cancelled` 只表示取消流程结束，不代表已发生副作用被撤销；若取消处理中识别到必须补偿的外部副作用，Task 必须进入 `rolling_back`；
- `partially_completed` 必须列出已产生的副作用；恢复编排可据此进入 `rolling_back`；
- `failed` 必须保留可恢复信息（包括 `failed_recovery_meta`）；后续恢复编排确认需要补偿时可进入 `rolling_back`；
- `running` / `waiting_user` / `paused` / `partially_completed` 的取消处理若发现存在需要补偿的外部副作用，必须进入 `rolling_back`；如果最初已写入 `cancelled` 后才由恢复编排发现该事实，允许 `cancelled -> rolling_back`；
- `failed -> rolling_back` 仅用于失败已被持久化后，恢复编排随后确认已有需补偿副作用的场景；若在写入失败终态前已经知道需要补偿，应直接从当前活动状态进入 `rolling_back`；
- `rolling_back` 表示对已提交的外部副作用运行持久化补偿工作流，不是回滚 SQLite 事务。SQLite 事务只能回滚尚未提交的本地事实；外部补偿必须创建新 Action；
- 重规划必须增加 `plan_version`，不覆盖原计划证据；
- Task 的 `failed_recovery_meta` 是元数据对象 + 不可变恢复尝试引用，不是状态机节点。

## 11. Action 状态机

### 11.1 ActionStatus 枚举

```text
pending
approved
leased
in_flight
completed
failed
unknown_side_effect
rolling_back
rolled_back
rollback_failed
cancelled
```

### 11.2 语义

| 状态 | 含义 |
|---|---|
| `pending` | Action 已创建，等待 Policy 评估 |
| `approved` | Policy 已允许，等待资源锁和租约 |
| `leased` | 已获取租约和资源锁，等待调度执行 |
| `in_flight` | 已派发到 Extension/Provider，等待结果 |
| `completed` | 副作用已发生并通过验证 |
| `failed` | 确认失败，副作用未发生或已明确失败 |
| `unknown_side_effect` | Extension 崩溃/超时/返回模糊，副作用状态未知 |
| `rolling_back` | 正在执行回滚补偿 |
| `rolled_back` | 回滚补偿成功 |
| `rollback_failed` | 回滚补偿失败，需人工介入 |
| `cancelled` | 在执行前被取消 |

### 11.3 合法转换

```text
pending
  -> approved  (Policy allow，或关联 ApprovalRecord 的 decision=approved)
  -> cancelled (Policy deny、Approval v2 current resolution decision=denied，或用户取消)
  -> pending   (Policy confirm：保持状态并创建/关联Approval v2 chain current request)

approved
  -> leased    (获取租约和锁成功)
  -> cancelled (超时或取消)

leased
  -> in_flight           (派发执行)
  -> approved            (仅 `lease_expired`；同一 SQLite 事务使 Lease 失效并释放全部资源锁)
  -> cancelled           (显式取消；同一 SQLite 事务使 Lease 失效并释放全部资源锁，若派发是否发生不确定则不得使用此转换)
  -> unknown_side_effect (派发是否已发生无法确定)

in_flight
  -> completed            (副作用确认 + 验证通过)
  -> failed               (确认失败)
  -> unknown_side_effect  (无法确定副作用状态)

unknown_side_effect
  -> completed            (后续验证确认副作用已成功)
  -> failed               (后续验证确认副作用未发生)
  -> rolling_back         (需回滚以恢复安全状态)

failed
  -> rolling_back         (需回滚)
  -> (terminal)           (无需回滚，直接终态)

rolling_back
  -> rolled_back          (补偿成功)
  -> rollback_failed      (补偿失败)

rolled_back
  -> (terminal)

rollback_failed
  -> (terminal，需人工介入)

cancelled
  -> (terminal)
```

### 11.4 关键规则

- Policy返回`confirm`时Action保持`pending`，创建/关联Approval v2 `request` chain head；不能提前进入`approved`或获取资源。current approved `resolution`可消费后`pending -> approved`，denied resolution后`pending -> cancelled`；invalidation后保持/返回pending并重评，不把失效当作deny；
- `leased -> approved` 只能由 `lease_expired` 触发，并且 Lease 失效、Action 状态更新与该 Action 全部资源锁释放必须在同一 SQLite 事务中提交；显式取消只有在 Kernel 能证明尚未派发时才走 `leased -> cancelled`，也必须原子释放 Lease 与锁；若派发是否已发生不确定，必须进入 `unknown_side_effect`，不能伪装为取消成功；
- `unknown_side_effect` 是 Action 专属状态，用于恢复流程。不可逆动作进入此状态后**禁止盲目重放**，必须先查询外部状态；若仍无法确定，则创建恢复决策候选并重新经过 Policy，无匹配规则时不得暗中强制确认；
- `rollback_failed` 描述**原始 Action** 的补偿编排失败。原始 Action 先由 Recovery 编排进入 `rolling_back`，再根据补偿 Action 的最终结果进入 `rolled_back` 或 `rollback_failed`，Task 同步记录恢复尝试；
- 补偿（Compensation）必须作为**新 Action** 创建，重新经过 Policy 评估、Approval、租约、资源锁、执行和验证，不得绕过正常执行链路；
- 补偿 Action 自身按普通链路运行：`pending -> approved -> leased -> in_flight -> completed | failed | unknown_side_effect`。补偿执行或验证失败时它进入 `failed`（结果未知时进入 `unknown_side_effect`），不得因为它是补偿就直接跳到 `rollback_failed`；
- 不可逆的未知结果动作（如外部发送、支付、删除）不得被自动重试或假定回滚；后续查询、补偿、继续、停止或确认均由新的恢复 Action 与匹配 Policy 决定。

## 12. Recovery 与 Verification 分层

### 12.1 分层关系

Recovery 和 Verification 不是独立状态机，而是横切关注点：

```text
                    ┌─────────────────────────┐
                    │     Task Status          │
                    │  (succeeded/failed/...)  │
                    └───────────┬─────────────┘
                                │ 汇总
                    ┌───────────▼─────────────┐
                    │   Recovery Decision      │
                    │ (per-Task 恢复策略选择)    │
                    └───────────┬─────────────┘
                                │ 驱动
                    ┌───────────▼─────────────┐
                    │    Action Status         │
                    │ (completed/unknown/...)  │
                    └───────────┬─────────────┘
                                │ 判定依据
                    ┌───────────▼─────────────┐
                    │   Verification Result    │
                    │ (per-Action 验证输出)     │
                    └─────────────────────────┘
```

### 12.2 Verification

- 每个 Action 完成后必须经过 Verification 才能进入 `completed` 或 `failed`；
- Verification 由 Task Engine 根据 Action 的 `verification_policy` 执行；
- 验证策略可包含：返回码检查、资源状态比对、前后证据对比、外部查询、用户确认；具体 Profile 可以通过 Extension 结果或 Object Handle 提供自己的证据类型，但不得要求 Core 理解其私有对象；
- Extension 返回自然语言"成功"不能跳过 Verification；
- Verification 结果写入 Action 记录，作为后续 Recovery 决策的输入。

### 12.3 Recovery

- Recovery 是 Task Engine 在 Action 进入 `unknown_side_effect` 或 `failed` 后触发的流程；
- Recovery 策略按 Action 的 `rollback_metadata` 和 Task 约束决定：(a) 查询外部状态确认 (b) 补偿回滚 (c) 标记失败 (d) 创建恢复决策候选并重新执行 Policy；
- `failed_recovery_meta` 是 Task 上的恢复元数据对象，表示恢复已尝试但未成功，并引用不可变 RecoveryAttemptRef 历史；
- 恢复过程本身产生新 Action（查询或补偿），这些 Action 经过完整的 Policy→Lease→Execute→Verify 链路。

## 13. Action Lease、In-flight 与 Reconcile

### 13.1 Lease

Action 在 `leased` 状态下持有一个时间有限的租约：

- Lease 绑定：`action_id`、`task_id`、`holder`（Extension/Provider 实例）、`expires_at`、`max_uses`（默认 1）；
- Lease 过期时，`leased -> approved` 只能由结构化原因 `lease_expired` 触发；Lease 记录失效、Action revision 更新及全部资源锁释放必须在同一 SQLite 事务中完成，防止状态已回退但锁仍被占用；
- 租约期间 Extension 崩溃：Action 进入 `unknown_side_effect`，Lease 失效。

### 13.2 In-flight 追踪

- `in_flight` 状态的 Action 在 agentd 内存中有活跃追踪记录；
- 追踪记录包含：开始时间、Extension 实例标识、超时 deadline、取消令牌；
- agentd 重启时内存追踪丢失，所有 `in_flight` 的 Action 在恢复时转入 `unknown_side_effect`。

### 13.3 Reconcile（启动恢复）

agentd 冷启动时对每个 `in_flight` Action：

1. 查询 Extension 是否仍在运行且持有结果；
2. 如能获取确定结果 → 正常完成 `completed` / `failed`；
3. 如 Extension 已不在 → 标记 `unknown_side_effect`，触发 Recovery；
4. 对不可逆 Action 的 `unknown_side_effect`：禁止自动重放；先执行可用的外部查询或验证，仍未知时创建 RecoveryDecisionCandidate，并按候选动作重新经过 Policy。是否查询、补偿、继续、停止或请求确认由恢复事实与匹配规则决定，不强制把 Task 置为 `waiting_user`。

### 13.4 补偿是新 Action

- 回滚某个原始 Action 意味着先由 Recovery 编排将原始 Action 置为 `rolling_back`，再创建一个新的补偿 Action；
- 补偿 Action 具有自己的 `id`、`idempotency_key`、`side_effect_class`；
- 补偿 Action 同样经过 Policy Engine 评估（可能有不同于原始 Action 的权限需求）；
- 补偿 Action 同样经过 Approval → Lease → Execute → Verify 完整链路；
- 补偿 Action 成功进入 `completed` 后，原始 Action `rolling_back -> rolled_back`；补偿 Action 确认失败进入 `failed` 后，原始 Action `rolling_back -> rollback_failed`；补偿结果未知时保持恢复流程可继续判定，不伪造失败或成功；
- `rollback_failed` 不是补偿 Action 的快捷失败状态。只有当一个 Action 自身成为被补偿的原始对象时，Recovery 编排才可将它置为 `rolling_back` / `rollback_failed`。

## 14. 计划与执行分离

Planner 生成的是建议计划。

Task Engine 在执行前必须：

1. 规范化能力名称；
2. 解析资源；
3. 评估权限；
4. 获取资源锁；
5. 创建 Action；
6. 调用 Extension；
7. 验证结果；
8. 提交状态或触发恢复。

Planner 不能直接调用 Extension。

## 15. 并发与通用资源锁

Core Resource Lock 按逻辑资源 URI / stable resource ref 命名，不按进程或某个 Profile 的对象类型命名。通用资源类别示例：

- session/device；
- file/document；
- account/channel；
- package manager；
- system service；
- memory maintenance；
- extension installation；
- 由 Extension 映射得到的 opaque Profile resource ref。

默认规则：

- 同一可变资源的并发写必须串行，或由领域定义可验证的版本合并；
- 同一账号的外部发送动作必须有顺序；
- 只读观察可共享，但不得与声明要求稳定观察面的写动作冲突；
- 外部主体接管资源时，Kernel 按该资源的抢占规则撤销或阻断冲突 Action。

锁必须绑定 Task/Action、持有者、租约、超时和恢复语义。Kernel 只判断 stable resource refs 的冲突与生命周期，不解释窗口、坐标、桌面会话等 Profile 私有结构。

Platform-specific desktop session、input device/session、window 等含义不在 Core 定义；Computer Use Profile 在 readiness / Input Session Lease 章节把它们映射为 opaque stable resource refs。相应 Core Resource Lock 只接收这些不透明引用并治理冲突与生命周期。

## 16. 幂等

所有可重试 Action 必须带幂等键。

对于不可安全重复的动作：

- 外部发送；
- 购买；
- 删除；
- 安装/升级；
- 系统服务变更；

恢复时必须先查询外部状态；若仍未知，则创建恢复决策候选并重新经过 Policy，不得盲目重放，也不得因为风险类别暗中强制确认。

## 17. 事务边界与 SQLite Outbox

### 17.1 事务边界

SQLite 事务用于：

- Task、TaskScope 与 ContentOrigin 创建/状态改变；
- 请求幂等记录；
- Action 创建；
- 权限判定引用；
- AuditRecord；
- Artifact/Memory Candidate 元数据。

当某项业务契约要求审计时，对应 AuditRecord 必须与该业务事实以及该事务要求产生的 Outbox 记录在同一个 SQLite 事务中校验并写入。AuditRecord Schema 校验或插入失败时，业务事实和 Outbox 必须整体回滚，不得留下“业务成功但审计缺失”的提交。AuditRecord 本身不因被写入而自动创建 EventEnvelope 或进入 Outbox。

`task.create` 有两个明确世代，禁止混用：

- **active v2 root create**：单事务写入幂等记录、ContentOrigin、TaskScope、根 Task、creation provenance、Audit 与唯一 `task.created` Outbox；payload 不含 parent；
- **legacy v1 direct create**：现有 SQLite/Rust 实现的历史合同，只用于 validation/read/migration 与既有数据恢复，禁止进入 active dispatcher或 production write。

active v2 typed handler 的第一次 `KernelClock` 读取同时固定 `accepted_at` 并做入口 deadline 检查；ContentOrigin receipt/received、TaskScope、Task、provenance、Audit 与 Event 的创建时间均从该值投影。任何引用缺失、Schema 失败或子事实不一致均回滚；不得只提交 Task。v1 已实现 producer 的精确旧字段继续由 `IMPLEMENTATION_CONTRACTS.md` legacy 小节记录，不能被描述为 active v2 实现。

`task.child.create`是第二个固定producer，但不是KCP TaskCreate：它以父Action为唯一入口，在一个`BEGIN IMMEDIATE`中一次性物化child Origin/Scope/Task/provenance/Audit/`task.created`/Verification、更新Action completion并写正式`action.state_changed`。child事件causation为Action；Action自身状态事件使用预持久化`action_transition` anchor，禁止Action self-causation。执行前校验Action current approved/lease/Stop Fence/expected revision；同一Action全生命周期最多映射一个child。事务内canonical readback失败则全部回滚；commit后响应丢失时以同一Action/generation/idempotency读取原bundle，不得创建第二个child。

这两类 SQLite 事务从 `BEGIN IMMEDIATE` 到 commit 不轮询 deadline，也不提供中途取消。commit 成功后 handler 才读取完成时钟；若此时到期，KCP/Action调用返回相应 deadline 错误，但已提交事实保留并按各自幂等合同恢复。post-commit notification intent 只用于在事务外唤醒 Publisher；通知失败不能回滚 durable Outbox，也不表示 Event 已发布或被消费。

外部副作用不能被 SQLite 回滚，因此必须使用：

```text
Intent persisted
-> external action
-> observation/verification
-> result persisted
```

这是一种可恢复工作流，不是假装跨系统 ACID。

### 17.2 SQLite Outbox

事件发布使用一个跨Envelope版本统一的Outbox，保证业务事实与待投递记录原子提交，并提供 at-least-once 发布。当前代码仍是v1规范化列实现；后续migration 0003按ADR-0008升级同一表，禁止建立并行`outbox_v2`：

1. 在同一个 SQLite 事务中：写入业务状态变更 + 插入 EventEnvelope 到 `outbox` 表，并分配全局单调递增 `outbox_position`；
2. 事务提交后，Event Publisher 按 `outbox_position` 升序读取未发布记录；
3. Publisher 成功发布到 Kernel 事件传输层后设置 `delivered_at`（或依保留策略删除记录）；`delivered_at` 只表示 Publisher 已发布，不表示任一或全部订阅者已经消费；
4. 订阅者使用 `dedup_key` 去重，实现幂等消费。

### 17.3 Outbox 字段（post-0003）

本节字段形态是migration 0003完成后的规范；当前0001/0002数据库仍保存v1 `causation_kind/causation_id`，不得把post-0003描述成现状。

- `event_id`、`type`、`schema_version`、`aggregate_type`、`aggregate_id`、`sequence`、`outbox_position`、`occurred_at`；
- `causation_ref`持久化为canonical `causation_json`：v2 tagged union为`command_request{id}` / `event{id}` / `action{id}` / `action_transition{action_id,transition_id}`，v1历史仅前两支；不得为分支扩张一组nullable列；
- `correlation_id`、`dedup_key`；
- `payload`持久化为canonical `payload_json`；
- `delivered_at`（可空，仅表示 Publisher 发布完成）。

post-0003 `outbox`表的DB级CHECK必须至少固定以下闭包，不能只用`length()>0`：`schema_version IN (1,2)`；`event_type`在五类active并兼容三类legacy的并集；`schema_version=1`时只允许`task.created|task.state_changed|stop_fence.activated`，`schema_version=2`时允许五类；event→aggregate固定为`task.*→task`、`action.state_changed→action`、`approval.state_changed→approval_chain`、`stop_fence.activated→stop_fence`且后者`aggregate_id='global'`。`payload_json`与`causation_json`至少CHECK `json_valid(...)`且root `json_type(...)='object'`；DB不宣称验证JCS字节、UUID/date-time、causation branch字段闭包、payload schema_version/type mapping或payload业务条件，这些由应用层按row envelope version执行exact Schema、typed decode、canonical byte equality与repository relation validation。payload版本仍由对应payload Schema权威，不能由DB把它推断为Envelope版本。

Outbox不保存完整`envelope_json`双源；读路径按row `schema_version`选择exact v1/v2 Envelope Schema与typed decoder，再从规范化列重建。v1/v2共享单一全局position与单一aggregate sequence空间。

### 17.4 订阅者 Cursor

- 所有持久事件订阅、轮询和重连 cursor **只能**使用 `outbox_position`；不得使用 `(aggregate_type, sequence)`、`event_id`、时间戳或订阅者本地计数作为协议 cursor；
- 订阅者保存最后确认处理的 `outbox_position`，重连时请求严格大于该位置的记录；
- 聚合内已提交事件仍按 `sequence` 严格有序：首条为 `0`，后续严格连续 `+1`；事务回滚的暂分配不占号；`outbox_position` 提供全局投递顺序，但不声明跨聚合领域因果关系；
- at-least-once 发布允许同一 `event_id` / `outbox_position` 对应的记录被重复投递，但每条 Outbox 记录的 `outbox_position` 必须全局唯一且只分配一次；消费者仍须按 `dedup_key` 或 `event_id` 幂等处理。

### 17.5 瞬时 UI 事件

- Client 通过本地 IPC 订阅 Kernel 事件；
- Client 重连时从 cursor 恢复，丢失窗口内的事件通过拉取当前 Task/Action 状态补偿；
- 瞬时 UI（如 toast 通知）是 best-effort，不保证送达。

### 17.6 Initiative 去重

Initiative System 在生成 Candidate 时使用 `dedup_key` 防止同一发现的重复建议：

- `dedup_key` 由 `(detector_type, resource_ref, reason_hash)` 组成；
- 已存在的 Candidate（不论状态）在去重窗口内阻止重复生成；
- 去重窗口过期后可重新建议。

## 18. Audit Store 与事件总线

### 18.1 AuditRecord

`AuditRecord v1`是现有legacy对象；active producer使用IC §6.16定义的完整AuditRecord v2。本节下表醒目标记为**v1 legacy wire only**，不得据此实现active producer。

AuditRecord v1 固定字段：

| 字段 | 约束与语义 |
|---|---|
| `id` / `schema_version` | UUID；schema_version 固定为 `1` |
| `audit_type` | v1 闭集：`task.creation_recorded`、`command.accepted`、`permission.evaluated`、`kernel.invariant_blocked`、`event.published`、`recovery.recorded`、`config.changed`；闭集是可编码分类，不声称所有生产路径已实现 |
| `level` | `user_activity`、`operational`、`security`、`debug` |
| `actor` | Actor 的完整 revision 快照或 `null`；Schema 仅能约束 `null` 只用于 `entry_point = system_internal`，生产者还必须证明确无可归因注册主体，否则选择完整 actor |
| `entry_point` / `occurred_at` | 既有 EntryPoint；Kernel 时间 |
| 核心对象引用 | `task_id`、`action_id`、`permission_decision_ref`、`approval_record_ref`、`recovery_attempt_ref` 可空；此处`approval_record_ref`仅为v1 legacy字段，active Audit v2使用`approval_resolution_ref`；`delegation_ref`是非空稳定引用或null |
| `task_creation_context` | 仅v1 legacy `task.creation_recorded`为严格对象；active Audit v2完整context与producer fixed values见IC §6.16。root v2 `reason_codes=["task_created_root_v2"]`，child action v2 `reason_codes=["task_created_by_action_v2"]`；不得沿用含混`task_created`。 |
| 建议与外发引用 | `model_call_refs` 是提出建议/参与推理的 ModelCallRecord 稳定引用；`payload_manifest_refs` 是 PayloadManifest 稳定引用；两类对象尚无正式 source Schema，不假称 UUID |
| 外发状态 | `external_content_status = not_sent | sent | unknown`；not_sent 时 manifest refs 必须为空，sent 必须由至少一个来源/对象/模型调用/manifest/因果稳定引用支撑，unknown 必须有 reason code |
| 来源与修改对象 | v1 legacy `content_origin_refs`是唯一UUID数组；active v2 producer exact ordering/empty规则由IC §6.16逐producer定义。MaterialAuthorizationProjection中的同名字段另按lowercase canonical UUID text UTF-8排序去重，不与Audit数组语义混用。 |
| 执行归因 | `extension_id` 可空；`provider_id` 是本次被审计操作实际使用的 Provider 或 null，不替代提出建议/参与推理的 `model_call_refs`；`causation_ref`、`correlation_id` 可空 |
| 回滚与恢复 | `rollback_capability = compensatable | not_compensatable | unknown`；`stop_fence_generation >= 1` 或 null，与 `recovery_attempt_ref` 一起回答停止/恢复是否影响执行 |
| `policy_context` | null 或审计时上下文快照：matched rule、policy set revision、decision ordering summary、policy mutation authority、authentication evidence refs；不是 PolicyRule 副本 |
| `outcome` | `succeeded`、`failed`、`blocked`、`deferred`、`observed` |
| `reason_codes` / `summary` | 结构化原因数组；可空的人类摘要 |
| `details` | 可配置的结构化正文，允许扩展字段 |

所有 v1 字段均 required；无关联事实时必须显式写 `null` 或空数组，使“已知无事实”区别于“生产者漏字段”。`task.creation_recorded` 的创建快照固定回答“为什么创建任务”，`external_content_status` 与 `payload_manifest_refs` 固定回答“是否发送外部内容”。固定引用闭包同时回答 SECURITY_PRIVILEGE §17 的委托、模型建议、执行者与权限、修改资源、验证、回滚、Policy 解释及 Stop Fence/恢复影响。

双源一致性属于对应 repository 的事务校验：`permission_decision_ref` 非空时 `policy_context` 必须非空，且其中 nullable `matched_rule_ref`、`policy_set_revision` 必须等于该不可变 PermissionDecision；不一致则 Audit 写入失败并整体回滚。`decision_ordering_summary`、mutation authority 与 auth evidence 只是补充审计快照。`rollback_capability` 必须从 ActionRequest.rollback_policy、Verification、Recovery 权威事实投影，不可独立编辑；明确值缺乏权威事实则使用 `unknown`，可解析事实冲突则写入失败。`provider_id` 是实际操作 Provider，`model_call_refs` 是建议/推理引用，两者可并存；若引用对应本次模型操作，未来持久层必须校验 provider 一致。

**当前实现状态必须分开陈述**：`kernel-sqlite` 已实现 Task create/get repository，并在同一事务内生产固定 `task.creation_recorded` AuditRecord 与唯一 `task.created` Outbox；它已机械校验 Task/TaskSpec/ContentOrigin/Audit/Event 的固定 canonical 子事实、共同 `accepted_at`、correlation/causation 与失败整体回滚。通用 Audit Store 也已实现 `external_content_status=sent` 的**单记录支撑引用非空检查**。仍未实现的是 PermissionDecision / `policy_context` 跨对象字段相等、rollback 权威投影、Provider / ModelCall 跨对象一致性，以及其它业务 Audit producer；不得把 sent 单记录检查冒充跨对象外发证明。

Schema 通过顶层 required 字段保证显式事实，并拒绝 `policy_context` 未知字段；`if/then/else` 约束 task creation 上下文、PermissionDecision 非空时的 policy_context、not_sent 空 manifest 与 unknown 非空原因。sent 的“多个候选引用数组至少一个非空”已由 `kernel-sqlite` producer/store 的单记录检查和测试覆盖；它不解析这些引用指向的外部对象。跨对象 PermissionDecision/rollback/provider 一致性当前仍无持久层实现或自动化测试。开放的 `details` 仍无法由 Schema 完全禁止重复归因。

最小默认记录是分类、层级、时间、入口、结果、原因以及适用的稳定归因；正文范围由用户配置或适用 Policy 决定。Secret、Token 或未脱敏正文默认不记录，但不由 Schema 硬编码禁止写入 `summary` / `details`。ModelCallRecord 与 Delegation 的规范名称已存在，但本仓库尚无对应 source Schema，当前字段只承诺 stable ref。业务契约要求审计时，必须遵守 §17.1 的同事务与失败整体回滚规则。

### 18.2 事件总线

内部 Event 使用点号分隔的小写名称，至少包括：

- `task.created` / `task.state_changed`；
- `plan.proposed` / `plan.updated`；
- 其它未来内部Action名称可包括`action.requested` / `action.started` / `action.completed` / `action.failed`，但它们不是首批active Catalog；首批只正式发布通用`action.state_changed`；
- Approval不发布`approval.requested`/`approval.resolved`平行事实；首批只发布`approval.state_changed`。其`ApprovalStateChangedPayloadV1`必须含required `change_kind`，闭集精确为`initial_request|resolution|invalidation_without_replacement|replacement_request`，并以record kind/ref表达request/resolution/invalidation/replacement；四类完整from/to/ref真值表与repository exact-equality边界以`IMPLEMENTATION_CONTRACTS.md` §5.6为唯一权威；
- `delegation.matched` / `delegation.rejected` / `delegation.revoked`；
- `memory_candidate.proposed` / `memory_candidate.committed` / `memory_candidate.rejected`；
- `extension.started` / `extension.stopped` / `extension.crashed`；
- `provider.capability_changed`；
- `security_mode.changed`；
- `stop_fence.activated`。

`snapshot.created`、`snapshot.expired`、`user_takeover.started`、`user_takeover.ended` 等桌面语义不是 Core 公共事件。当前也没有正式 Extension Event Schema、transport、persistence 或 subscription 合同，因此 Kernel 不得声称现已接收、保存、索引、订阅或稳定转发这些事件，更不得把它们写入 Core EventEnvelope / SQLite Outbox。未来若先建立 Profile 私有 Extension Event 合同，其 payload 对 Core 仍是不解释的 typed data；若要晋升公共 Kernel Event，必须再增加正式 payload Schema、Event Catalog、兼容说明与 Conformance。

- 首批对外正式Event Catalog包含`task.created`、`task.state_changed`、`action.state_changed`、`approval.state_changed`、`stop_fence.activated`。Approval不可变链每次current-head成功变化由repository同事务生产一条`approval.state_changed`；atomic replacement只发一个逻辑head事件，不暴露中间invalidation head。其它名称在对应Schema发布前不得以无类型payload冒充已实现API；

事件总线不能成为无界日志缓冲。持久化与实时订阅要分开。所有持久化事件通过 SQLite Outbox 发出，实时订阅从 Outbox 消费。

## 19. Emergency Stop 与 Kernel Stop Fence

### 19.1 Emergency Stop

用户触发的紧急停止，Kernel 级硬中断：

1. 激活 Kernel Stop Fence；
2. 找出所有受 Stop 影响且仍活跃的**副作用** Action，撤销其 Action Lease，并在同一原子状态变更中释放该 Lease 持有的全部 Resource Lock；
3. 撤销所有临时 Permission Lease / Privilege Lease，并使所有后续消费在 lease 边界返回 Stop/lease 失效错误；
4. 向所有 `in_flight` Extension 调用发送 cancel，不等待结果；无副作用只读诊断已经取得的事实不得被误删或强制转为 `unknown_side_effect`，Fence 激活后仍可发起新的只读诊断查看状态；
5. 可安全取消且确认未产生副作用的 Action 按正常取消事实闭合；只有不可安全取消或取消后外部结果仍不确定的副作用 Action 转入 `unknown_side_effect`，禁止自动重放；
6. 活跃 Task 转入 `paused` 或 `waiting_user`；
7. 通知所有 Extension 停止新的副作用；
8. 进入 Security Mode `Restricted`；
9. Emergency Stop 不依赖模型响应，完全由 agentd Kernel 执行。

首批 KCP 的 `stop.activate` 就是 Emergency Stop 的公开触发入口；同一 Kernel 事务先持久化 Fence generation 与 `stop_fence.activated` 事件，再执行其余可取消/通知步骤。重复激活返回当前 generation，不重复创建 Fence。

### 19.2 Kernel Stop Fence

Kernel Stop Fence 是 agentd 内部的通用逻辑栅栏，防止新副作用在停止期间被创建或派发：

- Emergency Stop 触发后立即拉起 Stop Fence；
- Fence 拉起期间：任何创建或推进新副作用 Action 的执行边界都直接返回 `stop_fence_active`，不创建普通 PermissionDecision，也不把 Fence 伪装为 PolicyRule `deny`；已存在 `pending` Action 保持 `pending` 并记录被 Fence 阻断的恢复事实；所有已撤销临时 Permission/Privilege Lease 的后续消费必须拒绝；无副作用只读 Query/诊断继续允许；
- 第一版 Fence 一旦激活就持久保持，Security Mode 切回 Normal **不能**暗中解除；首批 KCP 不提供解除方法；
- 后续只有经独立规范、API、恢复验证和审计定义的显式解除流程才能降下 Fence；
- 只读 Query 在 Fence 期间仍允许，用于诊断和状态查看。

### 19.3 Stop 级别

| 级别 | 触发 | 影响范围 |
|---|---|---|
| Action Cancel | 用户取消单个 Action | 该 Action 进入 cancelled/unknown_side_effect |
| Task Pause | 用户暂停 Task | 该 Task 所有 Action 暂停 |
| Emergency Stop | 用户全局紧急停止 | 所有 Action 停止 + Stop Fence + Restricted Mode |

## 20. 故障边界

### agent-runtime 崩溃

- Task 状态保留；
- 取消其未完成模型请求；
- Provider 副作用不自动重放；
- 重启后重新构建最小 Context Pack。

### Extension 崩溃

- 当前调用失败或未知；
- 未知副作用必须验证；
- Provider Registry 标记降级；
- 允许选择替代 Provider。

### UI 崩溃

- 不影响任务；
- 本机确认等待可超时；
- 恢复后读取 Task/Audit 重建界面。

### 数据库异常

- 进入 Restricted 或 Safe Recovery；
- 禁止新副作用；
- 保留只读诊断与导出。

## 21. 配置层级

从低到高：

1. 系统/平台默认值；
2. 用户全局配置；
3. Profile/Extension 配置；
4. Workspace/Domain 配置；
5. Delegation Contract；
6. Task 临时约束。

配置层级是**可覆盖的默认值叠加**，不是硬上限。唯一不可被任何配置突破的硬边界是：

> **Agent 不得修改 agentd Kernel、Core Identity、Policy Engine 解释器、Audit 完整性机制、Privilege Broker、Emergency Stop 及本文件定义的所有架构依赖不变量。**

用户和 Delegation 可以在上层覆盖默认值；Policy 无匹配规则时 `allow`。Freedom-first 不在配置层级中嵌入隐性"安全硬上限"。

## 22. Headless API

`agentd` 提供本地 Kernel Control Protocol API，第一版约束：

- 默认仅本地 IPC（Unix Socket / Named Pipe）；
- 不假设存在已认证的单一 Owner；第一版**不做 Owner 身份认证**；
- 每个连接携带 `actor` 和Envelope `entry_point`字段。legacy KCP v1 `auth`只能为null；Approval v2的local/system/remote证据通过独立versioned challenge/evidence对象进入Approval/Policy，不得把它们硬塞进v1 Envelope auth；
- 不得虚构"已认证唯一 Owner"身份或基于此假设授权；
- 不将 Extension 协议直接暴露给外部客户端；
- 不允许客户端伪造 Kernel Event；
- 远程开放必须显式配置并启用认证。

## 23. 架构验收

出现以下情况即不合格：

- 同一 Task 状态在多个进程各自维护；
- UI 关闭导致任务丢失；
- Planner 直接调用 Provider；
- Core 启动、KCP、Schema、crate 或基础产品完成声明依赖某个可选 Profile；
- Core 持有 Display、Window、Screenshot、Coordinate、Desktop generation、OperationSnapshot、Input Session Lease 等 Profile 私有模型；
- Profile 缺失、未协商或 probe 失败时仍返回成功或伪造能力；
- Provider 崩溃拖垮 agentd；
- 扩展之间直接互调；
- 失败恢复重放不可逆动作；
- 创建 Extension Host 常驻服务但没有独立隔离需求；
- 为每个模型角色创建服务；
- Action 没有独立状态机或与 Task 状态混淆；
- 补偿绕过 Policy Engine；
- Emergency Stop 依赖模型响应；
- `unknown_side_effect` 的不可逆 Action 被自动重放。
