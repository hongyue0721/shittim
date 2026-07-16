# CORE_ARCHITECTURE.md

> 本文件是运行时拓扑、状态所有权、任务、Action、事件、恢复、并发与 Stop Fence 的唯一事实源。

## 1. 架构目标

系统必须同时满足：

- UI 可关闭和重连；
- 模型 Runtime 可崩溃和重启；
- Extension 可按需启动；
- Provider 可部分缺失；
- 任务状态不依赖某个模型会话；
- 权限不依赖 Prompt；
- 平台差异不进入 Kernel Domain；
- 远程 Channel 不直接接触能力层；
- 故障后不会重复执行不可逆动作。

## 2. 部署单元

### 2.1 常驻

#### agentd

唯一控制面和事实权威。

#### agent-runtime

模型、Pi 和角色运行时。可以重启，不拥有现实状态。

### 2.2 非常驻

#### desktop-client

按用户界面生命周期存在。

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
| Provider 能力档案 | Provider Registry |
| Prompt/模型会话临时状态 | agent-runtime |
| UI 视图状态 | desktop-client |
| 特权执行内部状态 | Privilege Broker，仅动作期间 |

Provider 可以缓存本地对象，但其缓存不是 Kernel 事实。

## 5. 启动

### 5.1 agentd 冷启动

顺序：

1. 确认单实例和数据目录权限；
2. 打开 SQLite，检查 Schema；
3. 恢复未完成事务和 Action Lease；
4. 重建 Task 状态与资源锁；
5. 加载 Extension Registry，但不启动所有扩展；
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

1. 解析 Profile 和权限；
2. 选择兼容 Provider；
3. 启动隔离进程；
4. 协议握手；
5. 进行能力探测；
6. 注册到 Provider Registry；
7. 向等待中的 Task 返回可用性。

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
- 用户通过 desktop-client 看到的是 agentd 持有的 Task 真相，不感知 agent-runtime 重启。

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
- `deadline`：过期时间；Kernel 收到时已过期必须返回 `deadline_exceeded`。处理期间超过 deadline 时取消可取消工作并返回同一错误；对于已开始且不能安全取消的外部动作，不得宣称已取消或回滚，必须持久化为 `unknown_side_effect`/恢复待查后返回 `deadline_exceeded`；不得静默丢弃；
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
| `causation_ref` | 直接原因引用：`{ kind: command_request | event, id }` |
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

### 9.2 Subtask（父子 Task）

Subtask 不是第四种状态机。它就是一个普通 Task，通过 `parent_task_id` 引用父 Task。

规则：

- Subtask 拥有独立的 `id`、`status`、权限上下文和生命周期；
- 父 Task 的 `plan_version` 可以引用子 Task ID 列表；
- Subtask 可以有自己的 Delegation、资源锁和恢复策略；
- 取消父 Task 时子 Task 按各自当前状态执行取消语义，不做级联强制；
- 子 Task 失败不自动导致父 Task 失败；父 Task 的 success criteria 决定是否可接受；
- Subtask 的创建本身是一个 Action，需通过 Policy 评估。

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
  -> cancelled (Policy deny、关联 ApprovalRecord 的 decision=denied，或用户取消)
  -> pending   (Policy confirm：保持状态并创建/关联未决 ApprovalRecord)

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

- Policy 返回 `confirm` 时 Action 必须保持 `pending`，并关联一个 `decision = deferred` 或尚未决议的 ApprovalRecord；不能提前进入 `approved` 或获取资源。ApprovalRecord 批准后 `pending -> approved`，拒绝后 `pending -> cancelled`；
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
- 验证策略可包含：返回码检查、资源状态比对、快照对比、外部查询、用户确认；
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

## 15. 并发与资源锁

资源锁按逻辑资源而不是进程命名：

- desktop session；
- input device/session；
- window；
- file/document；
- account/channel；
- package manager；
- system service；
- memory maintenance；
- extension installation。

默认规则：

- 一个桌面输入会话同一时刻只有一个写入者；
- 同一文件的并发写必须串行或使用版本合并；
- 同一账号的外部发送动作必须有顺序；
- 只读观察可共享，但不得与要求稳定画面的写动作冲突；
- 用户接管 Computer Use 时必须抢占 Agent 输入锁。

锁必须有租约、持有者、超时和恢复语义。

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

- Task 状态改变；
- Action 创建；
- 权限判定引用；
- AuditRecord；
- Artifact/Memory Candidate 元数据。

当某项业务契约要求审计时，对应 AuditRecord 必须与该业务事实以及该事务要求产生的 Outbox 记录在同一个 SQLite 事务中校验并写入。AuditRecord Schema 校验或插入失败时，业务事实和 Outbox 必须整体回滚，不得留下“业务成功但审计缺失”的提交。AuditRecord 本身不因被写入而自动创建 EventEnvelope 或进入 Outbox。

外部副作用不能被 SQLite 回滚，因此必须使用：

```text
Intent persisted
-> external action
-> observation/verification
-> result persisted
```

这是一种可恢复工作流，不是假装跨系统 ACID。

### 17.2 SQLite Outbox

事件发布使用 Outbox 模式，保证业务事实与待投递记录原子提交，并提供 at-least-once 发布：

1. 在同一个 SQLite 事务中：写入业务状态变更 + 插入 EventEnvelope 到 `outbox` 表，并分配全局单调递增 `outbox_position`；
2. 事务提交后，Event Publisher 按 `outbox_position` 升序读取未发布记录；
3. Publisher 成功发布到 Kernel 事件传输层后设置 `delivered_at`（或依保留策略删除记录）；`delivered_at` 只表示 Publisher 已发布，不表示任一或全部订阅者已经消费；
4. 订阅者使用 `dedup_key` 去重，实现幂等消费。

### 17.3 Outbox 字段

- `event_id`、`type`、`schema_version`、`aggregate_type`、`aggregate_id`、`sequence`、`outbox_position`、`occurred_at`；
- `causation_ref`（`kind = command_request | event`）、`correlation_id`、`dedup_key`；
- `payload`（JSON）；
- `delivered_at`（可空，仅表示 Publisher 发布完成）。

### 17.4 订阅者 Cursor

- 所有持久事件订阅、轮询和重连 cursor **只能**使用 `outbox_position`；不得使用 `(aggregate_type, sequence)`、`event_id`、时间戳或订阅者本地计数作为协议 cursor；
- 订阅者保存最后确认处理的 `outbox_position`，重连时请求严格大于该位置的记录；
- 聚合内已提交事件仍按 `sequence` 严格有序：首条为 `0`，后续严格连续 `+1`；事务回滚的暂分配不占号；`outbox_position` 提供全局投递顺序，但不声明跨聚合领域因果关系；
- at-least-once 发布允许同一 `event_id` / `outbox_position` 对应的记录被重复投递，但每条 Outbox 记录的 `outbox_position` 必须全局唯一且只分配一次；消费者仍须按 `dedup_key` 或 `event_id` 幂等处理。

### 17.5 瞬时 UI 事件

- 桌面 UI 订阅 Kernel 事件通过本地 IPC；
- UI 重连时从 cursor 恢复，丢失窗口内的事件通过拉取当前 Task/Action 状态补偿；
- 瞬时 UI（如 toast 通知）是 best-effort，不保证送达。

### 17.6 Initiative 去重

Initiative System 在生成 Candidate 时使用 `dedup_key` 防止同一发现的重复建议：

- `dedup_key` 由 `(detector_type, resource_ref, reason_hash)` 组成；
- 已存在的 Candidate（不论状态）在去重窗口内阻止重复生成；
- 去重窗口过期后可重新建议。

## 18. Audit Store 与事件总线

### 18.1 AuditRecord

`AuditRecord` 是由 `agentd` 拥有的不可变本地审计事实，不带 `revision`，不允许原地更新。它与 EventEnvelope 是两类对象：创建 AuditRecord **不会**自动使其成为首批公开 Event Catalog 成员，也不会自动写入 Outbox；若未来需要公开审计事件，必须另行增加正式 Event payload Schema、Catalog、权限与保留契约，不能直接把本地审计正文当作公开事件。

AuditRecord v1 固定字段：

| 字段 | 约束与语义 |
|---|---|
| `id` / `schema_version` | UUID；schema_version 固定为 `1` |
| `audit_type` | v1 闭集：`task.creation_recorded`、`command.accepted`、`permission.evaluated`、`kernel.invariant_blocked`、`event.published`、`recovery.recorded`、`config.changed`；闭集是可编码分类，不声称所有生产路径已实现 |
| `level` | `user_activity`、`operational`、`security`、`debug` |
| `actor` | Actor 的完整 revision 快照或 `null`；Schema 仅能约束 `null` 只用于 `entry_point = system_internal`，生产者还必须证明确无可归因注册主体，否则选择完整 actor |
| `entry_point` / `occurred_at` | 既有 EntryPoint；Kernel 时间 |
| 核心对象引用 | `task_id`、`action_id`、`permission_decision_ref`、`approval_record_ref`、`recovery_attempt_ref` 可空；`delegation_ref` 是非空稳定引用或 null，Delegation 尚无正式 source Schema |
| `task_creation_context` | 仅 `task.creation_recorded` 为严格对象：固定 `task_revision=1`、非空 `goal`、UUID `origin_ref`、`proposer=user|companion|system`；从同事务 TaskSpec/Task 复制且不可变，其他 audit_type 必须为 null |
| 建议与外发引用 | `model_call_refs` 是提出建议/参与推理的 ModelCallRecord 稳定引用；`payload_manifest_refs` 是 PayloadManifest 稳定引用；两类对象尚无正式 source Schema，不假称 UUID |
| 外发状态 | `external_content_status = not_sent | sent | unknown`；not_sent 时 manifest refs 必须为空，sent 必须由至少一个来源/对象/模型调用/manifest/因果稳定引用支撑，unknown 必须有 reason code |
| 来源与修改对象 | `content_origin_refs` 是唯一 UUID 数组；`artifact_refs`、`resource_refs` 是唯一非空稳定引用数组。`resource_refs` 回答修改何资源，现有资源引用不全是标准 URI，故不强加 URI format |
| 执行归因 | `extension_id` 可空；`provider_id` 是本次被审计操作实际使用的 Provider 或 null，不替代提出建议/参与推理的 `model_call_refs`；`causation_ref`、`correlation_id` 可空 |
| 回滚与恢复 | `rollback_capability = compensatable | not_compensatable | unknown`；`stop_fence_generation >= 1` 或 null，与 `recovery_attempt_ref` 一起回答停止/恢复是否影响执行 |
| `policy_context` | null 或审计时上下文快照：matched rule、policy set revision、decision ordering summary、policy mutation authority、authentication evidence refs；不是 PolicyRule 副本 |
| `outcome` | `succeeded`、`failed`、`blocked`、`deferred`、`observed` |
| `reason_codes` / `summary` | 结构化原因数组；可空的人类摘要 |
| `details` | 可配置的结构化正文，允许扩展字段 |

所有 v1 字段均 required；无关联事实时必须显式写 `null` 或空数组，使“已知无事实”区别于“生产者漏字段”。`task.creation_recorded` 的创建快照固定回答“为什么创建任务”，`external_content_status` 与 `payload_manifest_refs` 固定回答“是否发送外部内容”。固定引用闭包同时回答 SECURITY_PRIVILEGE §17 的委托、模型建议、执行者与权限、修改资源、验证、回滚、Policy 解释及 Stop Fence/恢复影响。

双源一致性属于未来 Audit repository 的事务校验：`permission_decision_ref` 非空时 `policy_context` 必须非空，且其中 nullable `matched_rule_ref`、`policy_set_revision` 必须等于该不可变 PermissionDecision；不一致则 Audit 写入失败并整体回滚。`decision_ordering_summary`、mutation authority 与 auth evidence 只是补充审计快照。`rollback_capability` 必须从 ActionRequest.rollback_policy、Verification、Recovery 权威事实投影，不可独立编辑；明确值缺乏权威事实则使用 `unknown`，可解析事实冲突则写入失败。`provider_id` 是实际操作 Provider，`model_call_refs` 是建议/推理引用，两者可并存；若引用对应本次模型操作，未来持久层必须校验 provider 一致。

Schema 通过顶层 required 字段保证显式事实，并拒绝 `policy_context` 未知字段；`if/then/else` 约束 task creation 上下文、PermissionDecision 非空时的 policy_context、not_sent 空 manifest 与 unknown 非空原因。跨对象 PermissionDecision/rollback/provider 一致性，以及 sent 在多个候选引用数组中至少存在一个支撑引用，由 producer/repository 与 Conformance 约束，当前不声称已有持久层测试。开放的 `details` 仍无法由 Schema 完全禁止重复归因。

最小默认记录是分类、层级、时间、入口、结果、原因以及适用的稳定归因；正文范围由用户配置或适用 Policy 决定。Secret、Token 或未脱敏正文默认不记录，但不由 Schema 硬编码禁止写入 `summary` / `details`。ModelCallRecord 与 Delegation 的规范名称已存在，但本仓库尚无对应 source Schema，当前字段只承诺 stable ref。业务契约要求审计时，必须遵守 §17.1 的同事务与失败整体回滚规则。

### 18.2 事件总线

内部 Event 使用点号分隔的小写名称，至少包括：

- `task.created` / `task.state_changed`；
- `plan.proposed` / `plan.updated`；
- `action.requested` / `action.started` / `action.completed` / `action.failed`；
- `approval.requested` / `approval.resolved`；
- `delegation.matched` / `delegation.rejected` / `delegation.revoked`；
- `memory_candidate.proposed` / `memory_candidate.committed` / `memory_candidate.rejected`；
- `extension.started` / `extension.stopped` / `extension.crashed`；
- `provider.capability_changed`；
- `snapshot.created` / `snapshot.expired`；
- `user_takeover.started` / `user_takeover.ended`；
- `security_mode.changed`；
- `stop_fence.activated`。

首批对外正式 Event Catalog 只包含 `IMPLEMENTATION_CONTRACTS.md` 明确定义 payload Schema 的事件；其余名称在对应 Schema 发布前不得以无类型 payload 冒充已实现 API。

事件总线不能成为无界日志缓冲。持久化与实时订阅要分开。所有持久化事件通过 SQLite Outbox 发出，实时订阅从 Outbox 消费。

## 19. Emergency Stop 与 Kernel Stop Fence

### 19.1 Emergency Stop

用户触发的紧急停止，Kernel 级硬中断：

1. 激活 Kernel Stop Fence；
2. 立即停止所有 Computer Input 租约；
3. 取消所有 `in_flight` Action（发送取消信号，不等待结果）；
4. 将所有受影响 Action 转入 `unknown_side_effect`（不自动重放）；
5. 活跃 Task 转入 `paused` 或 `waiting_user`；
6. 撤销所有临时权限租约；
7. 通知所有 Extension 停止副作用；
8. 进入 Security Mode `Restricted`；
9. Emergency Stop 不依赖模型响应，完全由 agentd Kernel 执行。

首批 KCP 的 `stop.activate` 就是 Emergency Stop 的公开触发入口；同一 Kernel 事务先持久化 Fence generation 与 `stop_fence.activated` 事件，再执行其余可取消/通知步骤。重复激活返回当前 generation，不重复创建 Fence。

### 19.2 Kernel Stop Fence

Kernel Stop Fence 是 agentd 内部的逻辑栅栏，防止新副作用在停止期间被创建：

- Emergency Stop 触发后立即拉起 Stop Fence；
- Fence 拉起期间：任何创建或推进新副作用 Action 的执行边界都直接返回 `stop_fence_active`，不创建普通 PermissionDecision，也不把 Fence 伪装为 PolicyRule `deny`；已存在 `pending` Action 保持 `pending` 并记录被 Fence 阻断的恢复事实；
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
- 每个连接携带 `actor` 和 Envelope `entry_point` 字段，`auth` 第一版只能为 null，非 null 返回 `unsupported_auth_schema`；
- 不得虚构"已认证唯一 Owner"身份或基于此假设授权；
- 不将 Extension 协议直接暴露给外部客户端；
- 不允许客户端伪造 Kernel Event；
- 远程开放必须显式配置并启用认证。

## 23. 架构验收

出现以下情况即不合格：

- 同一 Task 状态在多个进程各自维护；
- UI 关闭导致任务丢失；
- Planner 直接调用 Provider；
- Provider 崩溃拖垮 agentd；
- 扩展之间直接互调；
- 失败恢复重放不可逆动作；
- 创建 Extension Host 常驻服务但没有独立隔离需求；
- 为每个模型角色创建服务；
- Action 没有独立状态机或与 Task 状态混淆；
- 补偿绕过 Policy Engine；
- Emergency Stop 依赖模型响应；
- `unknown_side_effect` 的不可逆 Action 被自动重放。
