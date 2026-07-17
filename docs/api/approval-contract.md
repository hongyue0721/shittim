# Approval v2 与 PermissionDecision 授权合同

> 状态：accepted contract-only；legacy v1仅供读取/迁移。字段、hash、CAS、identity repository、challenge终态、事件和错误唯一事实源为 [`IMPLEMENTATION_CONTRACTS.md`](../../specs/IMPLEMENTATION_CONTRACTS.md) §5.3.1、§5.6–5.7、§6.6、§6.10.1–6.10.6。

## Canonical confirmation mode

全链统一：`generic | local | system_authentication | remote_signature | plan_revision`。PolicyRule v2使用该闭集；PD映射为`require_confirmation | require_local_confirmation | require_system_authentication | require_remote_signature | require_plan_revision`；Approval request wire直接保存canonical mode。remote mode算法无关，首版算法由`RemoteSignatureAlgorithmV1`的唯一`ed25519` branch表达。

## Approval tagged unions与subject hash

ApprovalRecord v2外层`record_kind=request|resolution|invalidation`，内层`subject_kind=operation|task_proposal|plan_revision`。公共字段全部required，初始request的`predecessor_ref=null`，其余链关系按repository CAS。

`subject_hash`只等于完整`SubjectProjectionV1`的RFC8785 JCS/SHA-256。Approval wire仍保存完整subject；remote challenge/response/preimage与system authentication evidence共同绑定该hash。

## 链操作

- `append_request`只创建新链，expected head必须null；三个CAS方法均消费上层分配的正式`ApprovalEventAllocationV1 {event_id,correlation_id,dedup_key,changed_at,causation_ref}`。
- replacement只能由`invalidate_and_optionally_replace`在同一事务完成；invalidation与replacement互相引用并形成新current head。
- `resolve`只解析current request。
- 不可update/delete，不按时间或ID猜head。

每次成功head变化与Audit、Action/PD关系及Lease更新同事务写恰好一条`approval.state_changed`；atomic replacement不发布中间invalidation head。

## Material/observation

Material projection排除PD id/revision/self-reference；PD revision仍进入operation subject freshness。`content_origin_refs`按lowercase canonical UUID文本UTF-8排序去重；Task child capability hints只来自TaskScope hints。Observation projection为`not_applicable|observed` tagged union，不伪造provider。

纯observation重评可产生新PD；Core重新构造material projection并证明hash相同后，才可复用旧current approved resolution。

## Evidence矩阵

- generic/plan_revision：专属evidence ref均null，`evidence_refs=[]`。
- local：仅local presence ref，数组精确包含该ref。
- system_authentication：仅system auth ref；认证取消/失败不写approved resolution。
- remote_signature：仅remote response ref，approved/denied都要求合法签名response。

## Identity、Challenge与证据

Identity repositories分别拥有Credential register/rotate/revoke/get、Challenge issue/**只读**get/revoke/consume/`expire_challenge_with_expected_state`、local/system evidence insert/get。`get`不得因观察到过期写状态；remote与system resolve或任何consume在同一`BEGIN IMMEDIATE`内发现`issued && now>=expires_at`时，以expected `issued` CAS持久化`expired`、写identity/security Audit并返回`challenge_expired`。CAS loser重读`expired`仍返回同码；sweeper只能复用此helper，非正确性依赖。Challenge不是Approval chain，expiry**不**产生`approval.state_changed`。

remote验签/expire/consume/resolve与system evidence校验/expire/consume/resolve都通过同一transaction-bound helper完成。system mode先由Kernel签发`SystemAuthenticationChallengeV1`，注册OS authority成功后写绑定的`SystemAuthenticationEvidenceV1`；resolve同一事务校验challenge current/expiry/binding、evidence和current head后才consume并append approved resolution。OS取消或失败不写evidence、不consume、也不写resolution。
