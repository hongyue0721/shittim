# ADR-0007：Approval v2 不可变联合、身份与失效

- 状态：accepted
- 日期：2026-07-18
- 实现状态：partial；切片1c-i已落地ApprovalRecordV2、PermissionDecisionV2、PolicyRuleV2、SubjectProjectionV1、ApprovalEventAllocationV1 source/manifest/generated/conformance及SubjectProjection pure API/fixture；identity/challenge/evidence/remote signature与repository/CAS/producer仍未实现
- **部分被 [ADR-0009](0009-v2从零构建并取消v1数据迁移.md) superseded**：文中 v1→v2 migration 生产要求、repository `legacy v1 read projection` 作为迁移配套生产义务的效力作废；Approval v2 不可变联合 / current-head CAS / fingerprint / 身份证据决策仍有效。

## 背景

`ApprovalRecord v1` 把 approval type、decision 与一个全部可选的 target 对象混在同一记录中，Schema 甚至无法保证 target exactly-one。`implicit` 与 `delegation` 被表达成 Approval 类型，会把“无需审批”或“既有 Delegation authority”伪装成人类批准。当前 PermissionDecision 只有一个完整 evaluation context hash，无法区分实质授权变化与 Computer Use 等 Profile 的 snapshot/generation/坐标刷新。身份方面，v1 KCP `auth = null`，不存在 Owner 认证，却预留了 owner/local/system/remote 等语义，若没有正式证据合同容易产生虚假授权。

## 决策

### 1. ApprovalRecord v2 是真正的判别联合

每条记录不可变，`record_kind` 精确为：

```text
request | resolution | invalidation
```

`subject` 精确且只能有一个 variant：

```text
operation | task_proposal | plan_revision
```

- `operation`：绑定 `task_id`、`action_id`、Action revision、PermissionDecision ref、material authorization fingerprint，以及 operation/capability/side-effect/资源/关键参数的 material 摘要。
- `task_proposal`：绑定 candidate Task ID、candidate revision 与 proposal hash；适用于尚未成为可执行 Action 的 Task 候选批准。
- `plan_revision`：绑定 Task ID、base plan version、proposed plan version 与 proposed plan hash。

不同 `record_kind` 使用不同 payload：request 记录所需 confirmation mode与challenge；resolution 记录 approved/denied、resolver 与证据；invalidation 记录被失效 head、原因与替代 request（可空）。禁止使用可选字段拼装“差不多”的联合。

### 2. 不可变链与 current-head CAS

每个稳定 `approval_chain_id` 只有一个 current head。新 record 必须带 required-nullable `predecessor_ref`，repository以`(approval_chain_id, expected_head_ref)`做compare-and-swap；`current_head_ref`是repository查询结果/映射字段的规范名，不进入每条record重复存储：

- 初始 request 的 expected head 为 null；
- resolution 的 predecessor 必须是 current request；
- invalidation 的 predecessor 必须是 current request 或 approved resolution；
- CAS 失败返回 `approval_head_conflict`，调用方重新读取，不得覆盖或分叉 current head；
- 历史分支如由损坏/旧实现产生，只可作为 legacy/corrupt 诊断读取，不得选一个“看起来最新”的 head。

v1 仅保留 Schema/fixture 历史验证资产（历史：原文为 legacy read/validation/migration，已被 ADR-0009 supersede）；production write 只能 v2。

### 3. 哪些事实不创建 Approval

- Default Allow、显式 allow、implicit 路径不创建 ApprovalRecord；PermissionDecision/Audit 记录“不需要 Approval”。
- Delegation authority 通过 DelegationContract、匹配证据和 PermissionDecision 记录，不创建 ApprovalRecord。
- 只有 Policy/system mechanism确实要求 confirmation、local presence证明、system authentication或plan revision时，才创建 request chain。

### 4. material 与 observation fingerprint 分离

PermissionDecision v2 至少绑定：

- `material_authorization_fingerprint`：对会改变授权含义的规范化事实做 RFC 8785 + SHA-256，包括 actor/entry、operation、capability、side-effect、关键参数、resource scope、Task/plan material、Delegation authority、目标语义、Protected Surface分类与发送目的等；
- `observation_evidence_fingerprint`：对 snapshot/generation/provider ref/坐标 transform/观察时间与证据 refs 等瞬时观察做独立 fingerprint；
- `policy_set_revision` 与 decision revision仍独立存在。

实质变化使已 approved resolution失效：原子追加 invalidation，旧 PermissionDecision/Lease不可消费，重新评估；如新 decision仍要求批准则创建新 request。纯 observation刷新也使旧 PermissionDecision/Lease失效并要求 reobserve，但 Core 在新观察通过 Profile readiness 后证明 material fingerprint 等价时，可以创建新 PermissionDecision，并引用复用旧 approved resolution；不得让 Profile 自己判断 material 等价或直接延续旧 lease。

### 5. operation subject 绑定

operation Approval 必须绑定具体 Action与 PermissionDecision：

- Action ID/revision、task/plan version；
- PermissionDecision ID/decision revision/policy set revision；
- material fingerprint；
- confirmation mode；
- request expiry/challenge expiry。

Action material、plan、scope、delegation、operation或fingerprint变化后旧 resolution不能用于执行。仅 observation evidence变化时按上节等价复用规则处理。

### 6. plan revision

`plan_revision` resolution只决定某个 proposed plan hash/version是否被批准，不直接批准该计划中的 Action。resolution approved 后：

1. Task 以 expected revision CAS 接受对应 plan version/hash；
2. 所有相关 existing/proposed Action根据新 plan重新计算 material context；
3. 每个 Action重新 Policy evaluation，产生新的 PermissionDecision；
4. 只有 material等价且满足本 ADR复用规则时才可引用已有 operation resolution。

### 7. 身份与证据

以下词是旧概念说明，不是active wire alias；active wire一律使用下一节canonical mode：

- `generic`：按 Policy 允许的入口接受明确决议；它证明该 actor/entry作出了确认，不自动证明 Owner。
- `local`：必须引用 Kernel验证的 local presence evidence；只证明指定时间/会话的本地 presence，不证明 Owner、账户身份或 system authentication。
- `system_authentication`：必须引用由真实OS机制验证的evidence（如polkit/UAC/Authorization Services），先由Kernel签发`SystemAuthenticationChallengeV1`，再由注册OS authority完成绑定challenge/subject/material/expiry的证据；resolve在同一事务校验current challenge、expiry、binding、evidence及Approval current head后才consume。取消/失败不能写evidence或approved。
- `remote_signature`：必须使用签名 response，绑定 challenge id、随机 nonce、audience、actor/credential、task、subject hash、material fingerprint、issued/expiry，并由 Kernel验证签名、nonce单次使用、audience与时间。普通 Channel callback、平台昵称、消息文本或 `authentication_level` 标签不足以构成 remote resolution。
- v1 没有 Owner auth。Actor `kind=owner` 与预留字段不得被解释为认证证据；未来 Owner auth需独立版本化身份合同。

### 7. canonical confirmation、subject hash与算法演进

PolicyRule v2、PermissionDecision v2与Approval request统一使用`generic|local|system_authentication|remote_signature|plan_revision`。PD新增`require_remote_signature`；Approval wire不保存任何旧别名。remote是正式Policy入口，通用mode不硬编码算法。

`subject_hash`固定为完整`SubjectProjectionV1`的JCS/SHA-256，不是subject任意序列化或material hash别名。Approval wire保存完整subject；remote challenge/response/preimage与system evidence统一绑定该hash。字段、规范化和fixtures以IC §6.10.1为准。

### 8. 失效、事件与原子性

规范对象固定为§6.10的完整wire合同：两层tagged union、required-nullable predecessor、operation subject中的Task/plan/Action/PermissionDecision/policy/material/capability/operation/side-effect/resource/key参数绑定，以及request/resolution/invalidation各自actor/evidence/expiry字段。ADR不再以省略字段的概念对象替代该合同。

remote identity固定为`CredentialRefV1`、`RemoteApprovalChallengeV1`、`RemoteApprovalResponseV1`、`RemoteApprovalSignaturePreimageV1`和Ed25519 pure v1；system identity固定`SystemAuthenticationChallengeV1`与`SystemAuthenticationEvidenceV1`。两种Challenge的get只读当前事实；resolve/consume在issued且到期时以expected-issued CAS持久化expired、写identity/security Audit并返回`challenge_expired`。nonce由Kernel CSPRNG产生，JCS bytes、purpose、audience、TTL、状态机与复合事务以§6.10.3为准；Challenge expiry不产生`approval.state_changed`。local/system evidence不能用opaque字符串证明成功。

以下任一发生时，Kernel必须在相关业务事务中追加 invalidation或拒绝消费：material变化、subject revision/hash变化、Policy set造成决议要求变化、Delegation撤销/过期、认证/challenge/evidence过期、actor credential撤销、Stop Fence、resolution expiry。

Approval current-head每次成功变化必须与业务事实同事务写正式`approval.state_changed` Event；initial request、resolution、invalidation及atomic replacement各恰好一个逻辑head事件，不发布中间head。producer、payload、sequence与causation以IC §5.6为准。

invalidation、Action/Task current approval ref更新、Permission/Privilege Lease撤销及必要的新 request必须在同一事务提交；失败整体回滚。仅 Event发布通知在 durable Outbox commit后异步进行。

### 9. Repository 与查询

Approval repository必须提供：

- append initial request（仅新链，expected head必须null；不得用于replacement）；
- resolve current request；
- invalidate current request/approved resolution and optionally append replacement in the same transaction；
- get record；
- get current head by chain/subject；
- validate usable approved resolution against subject/material/evidence/expiry；
- list immutable history；
- legacy v1 read projection（不得写回 v1）。

不得提供 update/delete ApprovalRecord API。retention不能删除仍被 PermissionDecision、Audit、Action、Task或迁移 provenance引用的链记录。

## 拒绝的替代方案

- **修补 v1 target 为 exactly-one但保留同一平面对象**：拒绝；request/resolution/invalidation语义仍混杂。
- **implicit/delegation写一条 approved record**：拒绝；伪造并不存在的批准主体。
- **每次 snapshot刷新都重新要求人批准**：拒绝；把观察坐标变化误当成授权意图变化。
- **Profile判断 material等价**：拒绝；Profile不是权限权威。
- **local desktop或 owner标签自动视为Owner**：拒绝；v1没有这种认证事实。
- **远程“点了按钮”即 approved**：拒绝；缺少challenge、nonce、audience、material和签名绑定。

## 实现影响

后续必须新增 Approval v2、PermissionDecision v2与authentication evidence/challenge Schema，Approval/PermissionDecision repository、current-head CAS、Lease消费验证、KCP/API错误、migration与Conformance（历史：migration 要求已被 ADR-0009 supersede，无 v1 数据迁移）。现有 v1 generated type与领域代码只代表legacy合同，不因本ADR自动成为v2实现。

## 验收

1. Schema/typed API无法构造多 subject、无 subject或混合 record kind字段；
2. 并发 resolution/invalidation只有一个CAS成功；
3. implicit/delegation路径无 Approval记录；
4. material变化必失效，纯 observation刷新只有Core证明material等价后可复用旧resolution；
5. local presence不能冒充Owner，system auth必须有OS证据，remote必须完成challenge签名验证；
6. plan revision approved后所有相关Action重新评估；
7. v1仅保留Schema/fixture历史验证资产，无production read（历史：原文 legacy read，已被 ADR-0009 supersede）；production write拒绝。
