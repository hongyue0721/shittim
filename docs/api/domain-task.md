# domain-task 内部 Rust API

> **非 KCP 外部 API**。本页描述 `rust/crates/domain-task` 的纯领域状态机接口。
>
> 不持久化、不分配 `EventEnvelope` / `outbox_position`、不连接 SQLite / Tokio / Policy matcher / Extension。
>
> 状态枚举与完整闭集唯一来源：`kernel-contracts` 生成的 `TaskStatus` / `ActionStatus` 及其 Schema declaration-order `ALL`；`domain-task` 不维护手写状态目录。

## 定位

| 项 | 说明 |
|---|---|
| Crate | `domain-task` |
| 依赖 | `kernel-contracts`、serde、thiserror（测试：proptest） |
| 禁止依赖 | SQLite、Tokio、KCP、UI、domain-policy、Extension |
| 权威转换表 | `specs/CORE_ARCHITECTURE.md` §10（Task）、§11（Action） |
| 不变量 / 恢复 | `CORE` §10.3 / §11.4 / §12–13；`IMPLEMENTATION_CONTRACTS` §6.3–6.5 / §6.11–6.13 |

`ActionRequest.permission_decision_ref`初建pending可空，首次评估后指向current PermissionDecision v2。Policy confirm时持久层还必须关联Approval v2 chain/current request；当前Rust API中的单一`approval_record_ref`只是legacy字段适配，不能表达request/resolution/invalidation head，后续实现必须升级。

`PolicyEvaluationOutcome`及其`approval_record_ref`是legacy Rust v1适配对象；active v2 API必须以`approval_chain_id`和可消费`approval_resolution_ref`表达，禁止继续创建deferred ApprovalRecord。下面示例仅用于当前crate回归，不是active wire/domain shape。

### Task

```rust
use domain_task::{
    apply_task_transition, is_task_transition_allowed,
    TaskTransitionCommand, SuccessCriterionEvidence,
};
use kernel_contracts::TaskStatus;

// task.create 事实：status=candidate, plan_version=0, revision=1
let outcome = apply_task_transition(&TaskTransitionCommand {
    current_status: TaskStatus::Candidate,
    current_revision: 1,
    current_plan_version: 0,
    expected_revision: Some(1),
    target_status: TaskStatus::Planned,
    reason: "direct_plan".into(),
    replan: false,
    required_success_criteria: vec![], // TaskSpec.success_criteria 完整字符串列表
    success_criteria_evidence: vec![],
    produced_side_effect_refs: vec![],
    rollback_required_side_effect_refs: vec![],
})?;
```

`succeeded` 时按 **字符串多重集合** 精确覆盖：

- `required_success_criteria`：直接来自 `TaskSpec.success_criteria` 的完整字符串数组（非 ID）；
- `SuccessCriterionEvidence.criterion`：同一完整字符串内容；
- 用 `BTreeMap<String, count>` 比较出现次数；重复标准允许且各需一份 evidence；
- 每条 evidence：`satisfied` 且 `verification.outcome = verified_ok`；
- 缺次数 / 多次数 / 空内容均失败。错误信息称 **criterion content**。

`rolling_back` 时：`rollback_required_side_effect_refs` 至少一个非空 ref。

### Action

```rust
use domain_task::{
    apply_action_transition, apply_policy_evaluation_outcome,
    ActionTransitionCommand, ActionEvidence, PolicyEvaluationOutcome,
    PolicyEvaluationEffect,
};
use kernel_contracts::ActionStatus;

// confirm 不是图边
assert!(!is_action_transition_allowed(ActionStatus::Pending, ActionStatus::Pending));

let out = apply_policy_evaluation_outcome(
    "action-uuid",
    None, // parent_action_id: None=原始, Some=补偿
    ActionStatus::Pending,
    1,
    None,
    &PolicyEvaluationOutcome {
        effect: PolicyEvaluationEffect::Confirm,
        permission_decision_ref: "pd-…".into(),
        approval_record_ref: Some("ar-deferred-…".into()), // legacy v1 current crate only
        reason: "matched confirm rule".into(),
    },
)?;
```

命令字段要点：

- 非空 `action_id`（写入所有 `LeaseReleaseEffect.action_id`）；
- `parent_action_id: Option<String>` 镜像 `ActionRequest`：
  - `None` = 原始 Action（可进入 rolling_back / rolled_back / rollback_failed）；
  - `Some(非空且 ≠ action_id)` = 补偿 Action（普通链至 completed|failed|unknown）；
- `in_flight -> unknown` 需要 `uncertain_outcome_reason`；
- `target Failed` 需要验证摘要（`verified_failed`，或 `inconclusive` 且 `side_effect_confirmed=Some(false)`）。

**每次成功领域更新**（含 confirm）`revision + 1`；confirm 仅 status 不变且无 status event intent。

### Recovery / Compensation

- 仅 `retry_original` 有硬事实校验；其它 candidate kind 枚举层接受，不表示授权。
- `validate_compensation_action_draft`：新 Action，不同 id/idempotency，`parent_action_id` 指向原 Action，status=pending。

## 错误

| code | 含义 |
|---|---|
| `illegal_transition` | 不在 CORE 图中 |
| `expected_revision_conflict` | revision 不匹配 |
| `invariant_violation` | 不变量失败（含 criterion content 多重集合） |
| `missing_evidence` | 缺少业务事实 |
| `illegal_recovery_candidate` | retry_original 事实不合法 |
| `illegal_compensation_draft` | 补偿草稿不合法 |
| `invalid_input` | 输入非法（含 parent_action_id 空/等于自身） |

图合法但缺证据 → `missing_evidence` / `invariant_violation`。

## 状态闭集与测试边界

`domain-task` 的 NxN、terminal 与 proptest 直接遍历 `TaskStatus::ALL` / `ActionStatus::ALL`。生成层负责“有哪些状态”，领域层只负责合法边、证据和不变量；因此不再导出 `TASK_STATUS_CATALOG` / `ACTION_STATUS_CATALOG` 或平行 exhaustiveness helper。CORE 权威合法边数组与稳定边数断言仍保留，它们编码的是状态机业务语义，不是完整状态目录。

## 非目标

SQLite / Outbox / Policy matcher / KCP / agentd / 生成 UUID / 猜测证据 / 平行状态枚举 / criterion ID / ActionRole。
