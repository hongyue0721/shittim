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
9. 将无法安全恢复的任务置为 `waiting_user` 或标记 `failed_recovery` 元数据。

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

请求改变状态，必须带：

- `request_id`：客户端生成，用于去重和关联响应；
- `actor`：发起者身份（见 SECURITY_PRIVILEGE Actor 定义）；
- `entry`：入口点（local desktop / IPC client / remote channel 等）；
- `task_id` / `context`：所属 Task 或上下文；
- `deadline`：过期时间，超时后命令可能被拒绝或取消；
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

所有持久化事件包装在 EventEnvelope 中：

| 字段 | 说明 |
|---|---|
| `event_id` | 全局唯一事件标识 |
| `type` | 事件类型（如 `task.state_changed`） |
| `schema_version` | Event 负载的 schema 版本 |
| `aggregate_id` | 聚合根 ID（如 `task_id`） |
| `sequence` | 聚合内单调递增序号 |
| `occurred_at` | 事件发生时间（Kernel 时钟） |
| `causation_id` | 直接原因事件 ID（Command 或上游 Event） |
| `correlation_id` | 关联 ID，贯通整条因果链 |
| `dedup_key` | 去重键，用于消费者幂等处理 |

## 9. Task 模型

### 9.1 Task

Task 表示一个可解释的用户目标或 AI 主动目标。

必需字段：

- `id`；
- `origin`；
- `actor`；
- `proposer`（user/companion/system）；
- `goal`；
- `constraints`；
- `success criteria`；
- `status`；
- `plan_version`；
- `permission_context`；
- `delegation_ref`；
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
- `permission_decision`；
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

`failed_recovery` 不是 Task 状态，而是 Task 上的元数据标记，表示恢复流程已尝试但未成功。

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
  -> failed             (不可恢复的失败)
  -> cancelled          (用户取消)

waiting_user
  -> running            (用户响应)
  -> cancelled          (用户取消或超时)

paused
  -> running            (恢复)
  -> cancelled          (取消)

partially_completed
  -> running            (继续剩余步骤)
  -> succeeded          (接受部分结果为最终成功)
  -> failed             (声明失败)
  -> cancelled          (取消)

succeeded
  -> archived

failed
  -> archived
  -> planned            (用户要求以新计划重试；plan_version++)

cancelled
  -> archived

rolling_back
  -> rolled_back        (所有可回滚 Action 已回滚)
  -> failed             (回滚中遇到不可恢复情况，保留 failed_recovery 元数据)

rolled_back
  -> archived
  -> planned            (用户要求以新计划重试)
```

### 10.3 约束

- `succeeded` 只能由 Kernel 根据成功标准和验证结果设置；
- `cancelled` 不代表已发生副作用被撤销；
- `partially_completed` 必须列出已产生的副作用；
- `failed` 必须保留可恢复信息（包括 `failed_recovery` 元数据标记）；
- 重规划必须增加 `plan_version`，不覆盖原计划证据；
- Task 的 `failed_recovery` 是元数据布尔标记 + 恢复尝试记录，不是状态机节点。

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
  -> approved  (Policy allow)
  -> cancelled (策略拒绝或用户取消)

approved
  -> leased    (获取租约和锁成功)
  -> cancelled (超时或取消)

leased
  -> in_flight (派发执行)
  -> cancelled (租约到期或取消)

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

- `unknown_side_effect` 是 Action 专属状态，用于恢复流程。不可逆动作进入此状态后**禁止盲目重放**，必须先查询外部状态；若仍无法确定，则创建恢复决策候选并重新经过 Policy，无匹配规则时不得暗中强制确认；
- `rollback_failed` 是 Action 的终态，意味着该 Action 的回滚补偿已尝试但失败，Task 上记录 `failed_recovery` 元数据；
- 补偿（Compensation）必须作为**新 Action** 创建，重新经过 Policy 评估、获取租约和资源锁，不得绕过正常执行链路；
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
- `failed_recovery` 是 Task 上的元数据标记，表示自动恢复已尝试但未成功；
- 恢复过程本身产生新 Action（查询或补偿），这些 Action 经过完整的 Policy→Lease→Execute→Verify 链路。

## 13. Action Lease、In-flight 与 Reconcile

### 13.1 Lease

Action 在 `leased` 状态下持有一个时间有限的租约：

- Lease 绑定：`action_id`、`task_id`、`holder`（Extension/Provider 实例）、`expires_at`、`max_uses`（默认 1）；
- Lease 过期后 Action 自动回到 `approved` 状态，资源锁释放；
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
4. 对不可逆 Action 的 `unknown_side_effect`：**禁止自动重放**，进入 `waiting_user`。

### 13.4 补偿是新 Action

- 回滚某个 Action 意味着创建一个新的补偿 Action；
- 补偿 Action 具有自己的 `id`、`idempotency_key`、`side_effect_class`；
- 补偿 Action 同样经过 Policy Engine 评估（可能有不同于原始 Action 的权限需求）；
- 补偿 Action 同样经过 Approval → Lease → Execute → Verify 完整链路；
- 补偿失败 → 补偿 Action 进入 `rollback_failed`，原始 Action 也标记 `rollback_failed`。

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
- Audit Event；
- Artifact/Memory Candidate 元数据。

外部副作用不能被 SQLite 回滚，因此必须使用：

```text
Intent persisted
-> external action
-> observation/verification
-> result persisted
```

这是一种可恢复工作流，不是假装跨系统 ACID。

### 17.2 SQLite Outbox

事件发布使用 Outbox 模式，保证原子性和 at-least-once 投递：

1. 在同一个 SQLite 事务中：写入业务状态变更 + 插入 EventEnvelope 到 `outbox` 表；
2. 事务提交后，Event Publisher 按 `sequence` 顺序读取 `outbox` 中未投递事件；
3. 投递到订阅者后标记为已投递（或删除，依保留策略）；
4. 订阅者使用 `dedup_key` 去重，实现幂等消费。

### 17.3 Outbox 字段

- `event_id`、`type`、`schema_version`、`aggregate_id`、`sequence`、`occurred_at`；
- `causation_id`、`correlation_id`、`dedup_key`；
- `payload`（JSON）；
- `delivered_at`（可空）。

### 17.4 订阅者 Cursor

- 每个订阅者维护自己的 cursor：`(aggregate_type, last_sequence)` 或全局 `last_event_id`；
- 订阅者重连时从 cursor 之后继续消费；
- 聚合内事件按 `sequence` 严格有序；
- 跨聚合事件顺序不保证。

### 17.5 瞬时 UI 事件

- 桌面 UI 订阅 Kernel 事件通过本地 IPC；
- UI 重连时从 cursor 恢复，丢失窗口内的事件通过拉取当前 Task/Action 状态补偿；
- 瞬时 UI（如 toast 通知）是 best-effort，不保证送达。

### 17.6 Initiative 去重

Initiative System 在生成 Candidate 时使用 `dedup_key` 防止同一发现的重复建议：

- `dedup_key` 由 `(detector_type, resource_ref, reason_hash)` 组成；
- 已存在的 Candidate（不论状态）在去重窗口内阻止重复生成；
- 去重窗口过期后可重新建议。

## 18. 事件总线

内部 Event 至少包括：

- TaskCreated/StateChanged；
- PlanProposed/Updated；
- ActionRequested/Started/Completed/Failed；
- ApprovalRequested/Resolved；
- DelegationMatched/Rejected/Revoked；
- MemoryCandidateProposed/Committed/Rejected；
- ExtensionStarted/Stopped/Crashed；
- ProviderCapabilityChanged；
- SnapshotCreated/Expired；
- UserTakeoverStarted/Ended；
- SecurityModeChanged。

事件总线不能成为无界日志缓冲。持久化与实时订阅要分开。所有持久化事件通过 SQLite Outbox 发出，实时订阅从 Outbox 消费。

## 19. Emergency Stop 与 Kernel Stop Fence

### 19.1 Emergency Stop

用户触发的紧急停止，Kernel 级硬中断：

1. 立即停止所有 Computer Input 租约；
2. 取消所有 `in_flight` Action（发送取消信号，不等待结果）；
3. 将所有受影响 Action 转入 `unknown_side_effect`（不自动重放）；
4. 活跃 Task 转入 `paused` 或 `waiting_user`；
5. 撤销所有临时权限租约；
6. 通知所有 Extension 停止副作用；
7. 进入 Security Mode `Restricted`；
8. Emergency Stop 不依赖模型响应，完全由 agentd Kernel 执行。

### 19.2 Kernel Stop Fence

Kernel Stop Fence 是 agentd 内部的逻辑栅栏，防止新副作用在停止期间被创建：

- Emergency Stop 触发后立即拉起 Stop Fence；
- Fence 拉起期间：新的 Action 创建请求被拒绝（返回 `stop_fence_active` 错误）；Policy Engine 对 `pending` Action 统一返回 `deny`；
- Fence 降下需要用户显式解除或 Security Mode 恢复到 Normal；
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
- 每个连接携带 `actor` 和 `entry` 字段，预留 `auth` 字段供未来认证机制使用；
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
