# Error Catalog

> 本页只镜像并索引权威合同，不定义独有wire事实。唯一事实源是 [`IMPLEMENTATION_CONTRACTS.md` §5.7](../../specs/IMPLEMENTATION_CONTRACTS.md#57-v2权威-error-catalog)；code、固定message、details键、retryable或适用场景有任何差异时，以IC为准且文档漂移检查必须失败。

## 分类索引

- Preflight：`invalid_request`、`unsupported_protocol_version`、`unsupported_schema_version`、`unsupported_method`、`unsupported_auth_schema`；其中`unsupported_auth_schema`仅适用于实际执行Value preflight auth判定的层，official root fixture的raw Schema拒绝固定投影为`invalid_request`。
- 通用/存储：`deadline_exceeded`、`internal_error`、`revision_conflict`、`idempotency_conflict`、`sqlite_busy`、`sqlite_full`、`sqlite_corrupt`、`stored_data_invalid`。
- Task/Event/Origin：`task_not_found`、legacy-only `parent_task_not_found`、`parent_origin_not_found`、`origin_not_found`、`invalid_scope_pattern`、`invalid_cursor`、`unsupported_event_type`、`subscription_not_found`。
- Policy/Action/Lease：`stop_fence_active`、`unsupported_policy_condition`、Delegation三类、Action/PD/Approval-required、Lease/fence generation、`child_materialization_conflict`、Verification/material/observation错误。
- Approval/Identity：Approval record/head/invalidated/expired/subject/evidence，Challenge四终态与CAS expiry、signature/audience/Credential/evidence/local/system要求。

## 关键消歧

- 同Action同hash且bundle完整为replay成功；同Action不同proposal/material为`child_materialization_conflict`；同execution idempotency key绑定不同Action为`idempotency_conflict`；mapping不完整为`stored_data_invalid`。旧的“already materialized”错误不属于v2。
- `approval_not_found.details`固定为`{approval_record_ref,active_approval_chain_id}`。
- `invalid_cursor`同时覆盖Task list与Event cursor，但`cursor_kind`必须指出上下文；task.list cursor具体编码仍是独立开放ADR，不在这里猜测。
- 未知remote算法/对象版本是`invalid_request`或`unsupported_schema_version`；已知algorithm branch内签名不正确才是`signature_invalid`。
- `retryable=true`只授权在重读事实后按原幂等身份重试，不授权换Action、chain或idempotency key。

## MethodVersionBinding

八方法active/legacy/request-response映射由manifest未来生成的`MethodVersionBinding`提供，权威结构与表见IC §13.5。本文不维护运行时副本。
