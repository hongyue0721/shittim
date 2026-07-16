# IMPLEMENTATION_CONTRACTS.md

> 本文件定义仓库组织、模块边界、Kernel Control Protocol、参考对象、编码契约和逻辑流程。示例是契约表达，不要求逐字采用代码形式。

## 1. Monorepo

建议：

```text
/
  AGENT.md
  PROJECT_OVERVIEW.md
  specs/
  adr/
  schemas/
  rust/
    agentd/
    crates/
      domain-task/
      domain-policy/
      domain-memory/
      domain-initiative/
      extension-supervisor/
      extension-protocol/
      provider-registry/
      computer-core/
      audit-store/
      object-store/
      platform-common/
  ts/
    agent-runtime/
    desktop-client/
    sdk-typescript/
    mcp-bridge/
  sdk/
    rust/
    typescript/
    python/
    conformance/
  providers/
    linux/
    windows/
    macos/
  workers/
    memorix/
    vision/
  extensions/
    official/
  tests/
```

目录可以调整，但依赖方向必须保持。

## 2. Rust 模块

### domain-task

纯领域：Task、Step、Action、状态机、不变量、Recovery 元数据。

### domain-policy

权限、Side-effect、Approval、Lease、Delegation Match、PolicyRule。

### domain-memory

记忆事实、来源、冲突、候选提交和召回政策。

### domain-initiative

Opportunity、Candidate、预算和调度决定。

### extension-supervisor

进程生命周期、握手、权限装配和健康。

### extension-protocol

Schema、消息、错误、Object Handle。

### provider-registry

Profile、能力档案、选择和降级。

### computer-core

统一桌面模型、Target Resolver、Snapshot 和 Verification。

### audit-store/object-store

审计事实和大对象生命周期。

领域模块不得依赖 UI、Pi 或具体平台 API。

## 3. TypeScript agent-runtime

建议内部模块：

- identity prompt builder；
- model provider adapters；
- role registry；
- context pack builder；
- companion session；
- planner；
- worker runner；
- memory candidate extractor；
- skill/extension author；
- capability discovery client；
- kernel protocol client。

所有现实动作通过 Kernel Client 请求。

## 4. desktop-client

主要视图：

- Conversation；
- Task Center；
- Operation Snapshot；
- Approval Center；
- Memory & Persona；
- Initiative & Delegation；
- Skill/Extension；
- Model Routing；
- Platform Capability/Doctor；
- Audit/Recovery。

UI 不拥有 Task 真相，只订阅并发出 Command。

## 5. Kernel Control Protocol

Kernel Control Protocol 是 agent-runtime、desktop-client 及其他内部客户端与 agentd 通信的唯一协议。它与 Extension RPC **分开**，后者是 agentd 与 Extension/Provider 进程之间的协议。

### 5.1 设计原则

- Versioned：所有消息带 `protocol_version`，Breaking change 升主版本；
- Request/Response：每个 Command 和 Query 有 `request_id`，响应匹配；
- Actor/Entry：每条消息携带调用者身份与入口（见 SECURITY_PRIVILEGE Actor 定义）；
- Task/Context：绑定到 Task 或全局上下文；
- Deadline：每条消息可带超时，超时后 agentd 可拒绝或静默丢弃；
- Idempotency：Command 带 `idempotency_key`，agentd 保证同一 key 只生效一次（见 CORE_ARCHITECTURE 第 8.2 节）；
- Expected Revision：并发控制消息携带对象 `expected_revision`，不匹配时返回冲突错误；
- Auth 预留：消息中预留 `auth` 字段，第一版不强制认证，未来扩展；
- Cursor：查询可带 cursor 实现分页或增量订阅。

### 5.2 Command Envelope

```text
{
  protocol_version: "1.0",
  message_kind: "command",
  request_id: "<uuid>",
  actor: { kind, entry_point, auth_level, ... },
  entry: "local_desktop" | "local_ipc" | "remote_channel" | ...,
  auth: null | { ... },            // 预留，第一版可为 null
  task_id: "<uuid>" | null,
  context: { ... } | null,
  deadline: "<iso8601>" | null,
  idempotency_key: "<string>",
  expected_revision: <number> | null,
  command_type: "<string>",
  payload: { ... }
}
```

### 5.3 Query Envelope

```text
{
  protocol_version: "1.0",
  message_kind: "query",
  request_id: "<uuid>",
  actor: { ... },
  entry: "...",
  auth: null | { ... },
  task_id: "<uuid>" | null,
  cursor: "<opaque>" | null,
  limit: <number> | null,
  query_type: "<string>",
  payload: { ... }
}
```

### 5.4 Response Envelope

```text
{
  request_id: "<uuid>",
  status: "ok" | "error" | "conflict" | "rejected" | "stop_fence_active",
  payload: { ... } | null,
  error: {
    code: "<machine_code>",
    message: "<human_summary>",
    details: { ... } | null
  } | null,
  next_cursor: "<opaque>" | null
}
```

### 5.5 Event Stream

订阅者通过 Query 建立 cursor 后，Kernel 通过 Outbox 推送 EventEnvelope（见 CORE_ARCHITECTURE 第 8.5 节和第 17 节）。Event Stream 是 at-least-once，消费者使用 `dedup_key` 去重。

### 5.6 Extension RPC（区分）

Extension RPC 是 agentd 与 Extension/Provider 进程之间的协议，不在本文件定义（见 `extension-protocol` crate 和 EXTENSION_SDK spec）。Extension RPC 有自己的消息格式、错误模型和生命周期管理，不与 Kernel Control Protocol 共享 envelope。

## 6. 参考逻辑对象

### 6.1 持久与并发对象标记

- 所有持久对象必须有 `schema_version`（整数，用于迁移）；
- 可并发修改的对象必须有 `revision`（单调递增，用于乐观并发控制）；
- `schema_version` 变更规则见第 13 节。

### 6.2 Actor

```text
id
kind: owner | known_user | guest | system
source/entry_point
authentication_level
confidence (身份置信度，非授权)
```

第一版的 `owner` 仅是预留或本地配置的 actor 类别，不是已完成认证的唯一 Owner 结论；任何授权仍由 Policy 判定。

### 6.3 TaskSpec

```text
id
origin
actor
proposer: user | companion | system
goal
constraints[]
success_criteria[]
risk_hint
capability_hints[]
delegation_ref?
task_scope_ref
parent_task_id?          // Subtask 指向父 Task
status: TaskStatus       // 见 CORE_ARCHITECTURE 第 10 节
plan_version
schema_version
revision
created_at / updated_at
failed_recovery_meta?    // { attempted: bool, last_attempt_at, failure_reason }
```

### 6.4 PlanStep

```text
step_id
task_id
plan_version
seq_index                // 步骤顺序
intent                   // 自然语言描述
capability_refs[]        // 引用的能力 ID
resource_refs[]          // 预期资源
constraints[]            // 步骤级约束
depends_on_steps[]       // 前置步骤 ID
rollback_step_ref?       // 关联的回滚步骤
status: planned | in_progress | completed | skipped | failed
action_ids[]             // 该步骤产生的 Action ID 列表
created_at / updated_at
```

### 6.5 ActionRequest

```text
action_id
task_id
step_id?
parent_action_id?        // 补偿 Action 引用原始 Action
capability_id
operation
structured_arguments
resource_refs[]
task_scope_ref
side_effect_class: S0 | S1 | S2 | S3 | S4 | S5
idempotency_key
verification_policy: { strategy, expected_outcome, timeout }
rollback_policy?: { compensatable, compensation_action_ref?, auto_rollback_on }
status: ActionStatus     // 见 CORE_ARCHITECTURE 第 11 节
recovery_meta?: {        // Recovery 元数据
  unknown_side_effect_at?
  recovery_attempted: bool
  recovery_action_ids[]
  last_recovery_error?
}
lease?: {
  holder
  expires_at
  max_uses
}
schema_version
revision
created_at / updated_at
```

### 6.6 PermissionDecision

```text
decision: allow | deny | require_confirmation | require_local_confirmation
         | require_system_authentication | require_plan_revision
reason_codes[]           // 匹配到的规则 ID
matched_rule_ref?        // 命中的 PolicyRule ID
approval_type?: implicit | user_confirm | local_confirm | system_auth | delegation
granted_scopes[]
binding: {               // 决策绑定，防止权限漂移
  action_id
  plan_version
  resource_refs[]
  key_params_hash        // 关键参数哈希，参数变化时决策失效
}
decision_revision         // 决策版本，随 PolicyRule 或 Delegation 变更递增
expires_at?
lease_ref?
schema_version
```

PermissionDecision 适配 Default Allow：无规则命中时 `decision = allow`，`reason_codes = ["default_allow"]`，`matched_rule_ref = null`。

### 6.7 PolicyRule

引用 SECURITY_PRIVILEGE.md 为权限判定权威，此处定义 PolicyRule 存储字段：

```text
id
schema_version
revision
name / description
priority                 // 规则优先级，高优先级先匹配
enabled
actor_match: { kind?, entry_point?, auth_level_min? }
content_origin_match: { kinds[]?, source_patterns[]? }
resource_match: { scope_pattern, exclude_patterns[] }
action_match: { capability_ids[], operation_patterns[], side_effect_max }
condition: { time_window?, rate_limit?, delegation_required?, local_presence_required? }
effect: allow | confirm | deny
confirmation_mode?: generic | local | system_authentication | plan_revision
expires_at?
created_by / updated_by  // actor + entry
created_at / updated_at
source: user_defined | companion_generated | system
```

排序和 Default Allow 语义只由 SECURITY_PRIVILEGE.md 定义。Read-only/Restricted 等命名 Mode 若产生限制，必须投影为可见 PolicyRule；Safe Recovery 与 Stop Fence 只在维护 Kernel 一致性和禁止未知副作用盲目重放的范围内作为不可覆盖 Recovery Invariant，不构成第二套通用权限矩阵。

### 6.8 ExplorationScope

```text
id
schema_version
revision
name
scope_type: read_for_task | background_index | long_term_memory | profile_inference
         | cross_domain_association | prohibited
paths[] | resource_patterns[]
exclusions[]
initiative_level: L0 | L1 | L2 | L3 | L4 | L5
expires_at?
created_at / updated_at
```

### 6.9 TaskScope

TaskScope 是一次 Task 的临时处理边界，不会回写或扩大 ExplorationScope：

```text
id
schema_version
revision
task_id
resource_patterns[]
exclusions[]
allowed_capability_hints[]
source_refs[]            // 用户输入、计划或其他 ContentOrigin
created_by               // actor + entry
expires_at?
created_at / updated_at
```

`TaskSpec` 必须引用 `task_scope_ref`；`ActionRequest.resource_refs[]` 必须落在对应 TaskScope 内，超出时作为新的 Policy 输入处理，不能静默修改长期 ExplorationScope。

### 6.10 ApprovalRecord

```text
id
schema_version
approval_type: user_confirm | local_confirm | system_auth | delegation | implicit
target: { task_id?, action_id?, plan_step_id? }
actor
entry
decision: approved | denied | deferred
evidence_refs[]          // 系统认证 token 引用等
expires_at               // 批准有效期
created_at
```

### 6.11 VerificationResult

```text
id
schema_version
action_id
strategy_used
outcome: verified_ok | verified_failed | inconclusive
verifier_kind
observed_resource_refs[]
before_version?
after_version?
evidence_refs[]
confidence?
verified_at
observations[]: {
  check_type              // return_code | resource_state | snapshot_diff | external_query | user_confirm
  expected
  actual
  passed: bool
  evidence_ref?
}
side_effect_confirmed: bool | null   // null = 无法确认
recommendation: complete | retry | rollback | policy_decision_required
created_at
```

### 6.12 EventEnvelope

```text
event_id
type                    // e.g. "task.state_changed"
schema_version
aggregate_id            // e.g. task_id
sequence                // 聚合内单调递增
occurred_at
causation_id
correlation_id
dedup_key
payload                 // 类型化事件体
```

### 6.13 PayloadManifest

与 MODEL_RUNTIME.md 的规范名和字段语义一致。它描述一次模型调用实际候选 Payload 的对象集合，而不是通用对象存储清单：

```text
id
schema_version
task_id
subtask_id? / action_id?
role
intent_router_result_ref?
provider_id / model_id
context_pack_ref / context_pack_revision
objects[]: {
  type
  stable_ref
  revision?
  hash
  size_bytes
  media_type?
}
total_size_bytes
estimated_input_tokens
routing_config_revision
policy_evaluation_ref
created_at
```

Manifest 不保存 Payload 正文，也不承担审批或内容审查。

### 6.14 ModelCallRecord

```text
id
schema_version
manifest_id
task_id
call_type: plan | chat | memory_extraction | initiative | code_gen | verify
provider_request_id?
provider_id / model_id
endpoint_config_revision
started_at / finished_at
latency_ms
input_tokens / output_tokens
cost
result_status
output_object_ref? / output_hash?
stop_reason?
error_code?
retry_of? / fallback_of?
cancel_or_timeout_result?
```

`ModelCallRecord` 是 MODEL_RUNTIME.md 的规范名；不得另建 `ModelCall` 平行类型。

### 6.15 DelegationContract

```text
id
schema_version
revision
domain / scopes
actions                  // 动作白名单
side_effect_ceiling
trigger
approval_policy
budget
notification
verification
rollback
expiration
status: active | revoked | expired | superseded
version
created_by
created_at / updated_at
```

### 6.16 MemoryCandidate

```text
id
schema_version
kind
subject
content/value
scope
evidence_refs[]
confidence
sensitivity
suggested_expiration
reason
status: proposed | committed | rejected | superseded
```

### 6.17 CapabilityDescriptor

```text
id / profile / extension
summary
input / output schema
permissions
side_effect
reliability
cancellation
verification hints
cost / platform
```

### 6.18 OperationSnapshot

```text
snapshot_id
desktop_generation
task_id
frame_handles[]
window / context
candidates[]
transforms[]
created_at / expires_at
```

## 7. 消息路由逻辑

```text
normalize input
-> authenticate actor/entry point
-> load minimal session and relevant memory
-> deterministic command checks (stop, cancel, approvals)
-> Companion/Intent decision
-> chat reply OR Task candidate
-> Kernel validates Task
-> direct execution OR Planner
```

用户新增消息可能：

- 普通对话；
- 修改当前 Task 约束；
- 创建新 Task；
- 回答审批；
- 接管/取消；
- 委托候选确认。

不能全部追加到 Planner 对话末尾。

## 8. 计划逻辑

```text
Task context
-> capability catalog summaries
-> Planner structured plan
-> Kernel normalize and version
-> policy preflight
-> acquire resources per step
-> execute next action
-> verify
-> update/replan
```

Kernel 可以拒绝或拆分 Planner Step。

## 9. Action 执行逻辑

```text
assert task runnable
assert plan version
resolve resources
policy evaluate
approval/lease
acquire lock
persist action intent
invoke extension
persist raw result refs
observe/verify
commit action result
release lock
emit events
```

Extension 返回自然语言"成功"不跳过 verify。

## 10. Delegation Match

匹配时检查：

- active 且 version 最新；
- domain / scope 覆盖；
- actor / entry_point 匹配或包含；
- action 在白名单内；
- side_effect_class 不超过 ceiling；
- trigger 条件满足；
- budget 未耗尽；
- approval_policy 允许；
- expiration 未过期；
- required Skill/Provider version 满足。

### 10.1 越界处理

当 Action 超出 Delegation 白名单或 scope 时：

1. **不自动强制确认或拒绝**；
2. 将 Action 重新提交 Policy Engine 完整评估；
3. Policy Engine 按优先级匹配PolicyRule（包括其他 Delegation、用户全局规则、系统规则）；
4. 无规则命中时按 Default Allow 放行（`decision = allow`）；
5. Delegation 不是唯一授权来源；用户可能有其他匹配规则或默认允许覆盖。

任一超出即**不**做模糊相似授权，也**不**自动降级为"必须确认"，而是走标准 Policy 评估。

## 11. Memory Commit

```text
candidate
-> evidence available?
-> sensitivity/scope check (按 Policy 规则评估)
-> duplicate/conflict?
-> Policy 评估是否需要确认（无规则时 allow，即默认不要求确认）
-> commit memory fact
-> index via provider
```

要点：

- 不再有硬编码的"forbidden sensitivity"黑名单；sensitivity 由用户 Policy Rule 和 ExplorationScope 控制；
- 不再有全局"必须确认"默认；确认需求来自匹配的 Policy Rule（如 `require_confirmation`），无规则时默认 `allow`（自动提交）；
- Provider 索引失败不丢失 Kernel 事实，标记 `index_pending`。

## 12. Provider 选择

Provider Registry 根据：

- Profile；
- platform；
- capability completeness；
- permission availability；
- reliability；
- current health；
- user preference；
- cost；
- task requirements。

允许组合多个 Provider，选择结果写入 Action/Audit。

## 13. Schema 版本

所有持久对象和协议消息必须有版本。

规则：

- 向后兼容新增字段：minor version bump；
- 删除/改变含义：major version bump；
- 未知字段应合理忽略（forward compatibility）；
- 未知 enum 不可默认映射为 allow（安全敏感）；
- 数据迁移有 preflight、backup、verify、rollback；
- `schema_version` 存储在每条持久记录中。

### 13.1 并发对象 revision

- 所有可并发修改的持久对象额外携带 `revision`（单调递增整数）；
- 更新操作通过 `expected_revision` 实现乐观锁；
- revision 冲突返回 `conflict` 状态，由调用者重新读取后重试；
- 不可并发修改的对象（如不可变 Event）不需要 revision。

## 14. 日志

### 14.1 层级

- User Activity Summary；
- Operational Log；
- Security Audit；
- Debug Trace。

日志使用 Trace/Task/Action/Extension ID 关联。

### 14.2 默认策略

- 默认记录**最小元数据**：操作类型、时间、Task/Action ID、结果状态、错误码；
- 不默认记录 Secret、密码、Token、完整未脱敏敏感文档、完整模型 Prompt；
- 用户可通过 Policy Rule 进一步限制或扩大日志内容；
- 日志保留策略由用户配置（默认保留天数、存储上限）。

### 14.3 不硬禁

Secret 和未脱敏正文**不被硬编码禁止**写入日志（某些诊断场景可能需要），但：

- Debug Trace 级别在默认配置中关闭；
- 启用完整日志可由用户或 Bot 配置，并必须记录风险与来源；
- Security Audit 层默认使用脱敏摘要，以减少审计副本；这是展示/复制策略，不是对 Memory、模型 Payload、Extension 数据流或云发送的阻断，配置可选择保留更完整内容。

## 15. ADR

以下变化必须 ADR：

- 新常驻进程；
- 新 SDK 传输；
- 新状态所有者；
- 核心技术栈改变；
- 权限等级改变；
- Core Identity 改变；
- 新特权动作类别；
- 删除平台抽象；
- 允许扩展进程内运行。

ADR 必须说明替代方案和迁移。

## 16. 编码规范

- Rust 核心禁止 `unwrap` 处理可恢复外部错误；
- 所有外部调用有 timeout/cancel；
- TypeScript 开启 strict；
- Schema 生成类型，不手工复制多份；
- 平台特定代码不进入 domain crates；
- Side effect API 名称必须清晰；
- 权限失败不做静默 fallback；
- 错误保留 machine code 与用户可解释摘要。

## 17. AI 编码代理提交模板

每个变更应报告：

- 目标；
- 权威规范；
- 受影响状态所有者；
- 权限/副作用；
- 失败和恢复；
- 新增/变更 Schema；
- 测试；
- ADR（如需要）；
- 未完成或不确定点。

## 18. 禁止模式

- UI 调用 OS API；
- Agent Runtime 执行 Shell；
- Provider 写 Task 状态；
- Extension 写权限数据库；
- Memory Provider 自动拼接 Prompt；
- 角色各自独立服务；
- 九套 SDK；
- Extension Host 无理由独立常驻；
- 通过环境变量把所有 Secret 传给子进程；
- 使用当前焦点作为 Computer Use 唯一目标；
- 在恢复中重放未知外部副作用；
- Kernel Control Protocol 与 Extension RPC 混用同一 envelope；
- Delegation 越界自动降级为确认而不重新评估 Policy；
- `unknown_side_effect` 被当作"可能成功"而乐观继续后续 Action。
