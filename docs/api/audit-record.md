# AuditRecord v1

> 状态：契约、Draft 2020-12 Schema、Rust 生成类型、示例与运行时校验已完成；`kernel-sqlite` 已实现 canonical immutable Store、读取重验，以及 task.create 的固定 Audit producer。其它业务 producer 与 PermissionDecision/rollback/provider 跨对象一致性仍未实现。

## 定位

`AuditRecord` 是 `agentd` 拥有的不可变本地审计事实。它不带 `revision`，不能原地更新，也不是 `EventEnvelope`：不自动加入公开 Event Catalog、不自动进入 Outbox、不创建公开 KCP audit 方法。

## v1 字段

全部 v1 字段都是 required；没有关联事实时必须显式使用 `null` 或空数组。

| 字段 | 类型/约束与用途 |
|---|---|
| `id` / `schema_version` | UUID；版本固定 `1` |
| `audit_type` | `task.creation_recorded`、`command.accepted`、`permission.evaluated`、`kernel.invariant_blocked`、`event.published`、`recovery.recorded`、`config.changed` 闭集 |
| `level` | `user_activity`、`operational`、`security`、`debug` |
| `actor` | 完整 Actor revision 快照或 `null`；`null` 只允许 `system_internal`，且 producer 必须证明确无可归因注册主体 |
| `entry_point` / `occurred_at` | 既有 EntryPoint；RFC 3339 date-time |
| `task_id` / `action_id` | UUID 或 `null` |
| `task_creation_context` | 仅 `task.creation_recorded` 为严格对象，其余类型必须为 `null` |
| Permission/Approval/Recovery | `permission_decision_ref`、`approval_record_ref`、`recovery_attempt_ref` 为 UUID 或 `null` |
| `delegation_ref` | 非空稳定引用或 `null`；Delegation 尚无 source Schema |
| `model_call_refs` | 唯一非空稳定引用数组；引用提出建议或参与推理的 ModelCallRecord，后者尚无 source Schema |
| `payload_manifest_refs` | 唯一非空稳定引用数组；关联实际外发候选清单，PayloadManifest 尚无 source Schema |
| `external_content_status` | `not_sent`、`sent`、`unknown`，明确回答是否外发 |
| Verification/来源/资源 | `verification_result_refs`、`content_origin_refs` 为唯一 UUID 数组；`artifact_refs`、`resource_refs` 为唯一稳定引用数组 |
| 执行归因 | `extension_id`、`provider_id` 为稳定 ID 或 `null`；`provider_id` 是本次被审计操作实际使用的 Provider |
| 因果关联 | `causation_ref`、`correlation_id` 可空 |
| `rollback_capability` | `compensatable`、`not_compensatable`、`unknown` |
| Stop/Policy | `stop_fence_generation` 可空；`policy_context` 为严格对象或 `null` |
| 结果正文 | `outcome`、`reason_codes`、`summary`、开放结构 `details` |

## 任务创建快照

`audit_type = task.creation_recorded` 时：

- `task_id` 必须是 UUID，不能为 `null`；
- `task_creation_context` 必须完整携带 `task_revision = 1`、非空 `goal`、UUID `origin_ref`、`proposer = user | companion | system`；
- 这些值必须从同一事务创建的 TaskSpec/Task 复制，形成不可变审计快照，用于固定回答“为什么创建任务”；
- 其他 `audit_type` 的 `task_creation_context` 必须为 `null`，不能把 TaskSpec 副本扩散到普通审计记录。

该条件使用 JSON Schema `if/then/else` 表达。生成器将这些关键字视为 validation-only：Rust 字段类型会生成，但条件由 runtime validator 执行。

## 外发事实

- `not_sent` 时 `payload_manifest_refs` 必须为空；
- `sent` 时 producer 必须至少提供一个稳定支撑引用，来源可以是 `content_origin_refs`、`artifact_refs`、`resource_refs`、`model_call_refs`、`payload_manifest_refs` 或 `causation_ref`；
- `unknown` 必须通过非空 `reason_codes` 解释未知原因，可以没有 manifest；
- PayloadManifest 只用 stable ref，不新增或假称已有 source Schema，也不在 AuditRecord 复制正文。

当前 Schema直接校验 `not_sent` 的空 manifest 与 `unknown` 的非空原因；“sent 的多个候选数组至少一个非空”属于跨字段 producer/repository 契约，当前受限生成器不支持所需组合形状，因此只写入 Conformance，不伪装成已有自动化实现。

## 双源一致性

### PermissionDecision 与 policy_context

`permission_decision_ref` 非空时，`policy_context` 必须非空。未来 Audit repository 写入时必须读取该不可变 PermissionDecision，并要求：

- `policy_context.matched_rule_ref` 与 PermissionDecision 的 nullable `matched_rule_ref` 完全相等；
- `policy_context.policy_set_revision` 与 PermissionDecision 完全相等；
- `decision_ordering_summary`、`policy_mutation_authority`、`authentication_evidence_refs` 是补充审计快照，不替代 PermissionDecision 权威字段。

Schema只能约束同一 AuditRecord 内的非空关系，不能跨对象比对。未来不一致必须使 Audit 写入失败并回滚同一事务；当前 SQLite Audit Store 已实现 canonical/immutable 行为，但 PermissionDecision 跨对象比对仍未实现。

### rollback_capability

`rollback_capability` 是审计时从 `ActionRequest.rollback_policy`、Verification 与 Recovery 权威事实投影的只读结果，不能独立编辑。`compensatable` / `not_compensatable` 必须有权威事实；事实缺失或仍不可判定时写 `unknown`；可解析权威事实彼此冲突时 Audit 写入失败并回滚，而不是任选一个值。

### Provider 与 ModelCallRecord

`provider_id` 表示本次被审计操作实际使用的 Provider；`model_call_refs` 表示提出建议或参与推理的 ModelCallRecord。两者可以同时存在，不互为替代。若被审计操作本身是模型操作，且引用的 ModelCallRecord 对应该次操作，则其 provider 必须与 `provider_id` 一致；该检查属于未来持久层。

## 原子性

业务契约要求审计时，Kernel 必须在同一个 SQLite 事务中校验并写入业务事实、AuditRecord 与该业务事实要求的 Outbox。AuditRecord Schema、跨对象一致性或插入失败必须使整个事务回滚，不能留下“业务成功但审计缺失”的状态。`kernel-sqlite` 已为 task.create 实现该原子 producer；其它业务 producer 与 PermissionDecision/rollback/provider 跨对象检查仍未实现。

## Schema 与示例

- `$id`: `https://schemas.shittim.local/v1/audit/audit_record.json`
- source: [`../../schemas/source/audit/audit_record.v1.json`](../../schemas/source/audit/audit_record.v1.json)
- task creation 示例: [`../../schemas/examples/audit/audit_record.valid.json`](../../schemas/examples/audit/audit_record.valid.json)
- system internal 示例: [`../../schemas/examples/audit/audit_record.system_internal.valid.json`](../../schemas/examples/audit/audit_record.system_internal.valid.json)
- Rust 类型由 `schema-tool` 生成，禁止手写生成物。
