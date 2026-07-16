# domain-policy 内部 Rust API

> **非 KCP 外部 API**。本页描述 `rust/crates/domain-policy` 的纯领域 matcher。
>
> 不持久化、不连接 SQLite/Tokio、不生成 UUID/时间/revision，不创建隐藏系统规则。
> PolicyRule、Actor、ContentOrigin、EntryPoint、SideEffectClass、confirmation mode 与 PermissionDecision decision 均直接使用 `kernel-contracts` 生成类型。

## 定位

| 项 | 说明 |
|---|---|
| Crate | `domain-policy` |
| 权威语义 | `SECURITY_PRIVILEGE.md` §1–4；`IMPLEMENTATION_CONTRACTS.md` §6.6–6.7 |
| 依赖 | `kernel-contracts`、chrono/chrono-tz、url、serde、thiserror |
| 禁止依赖 | SQLite、Tokio、KCP server、UI、Extension、`domain-task` |
| 默认 | 无匹配规则为 `allow` / `reason_codes=["default_allow"]` / matched rule 为 null |

## 核心入口

```rust
use domain_policy::{evaluate_policy, PolicyEvaluationResult, RejectRateLimits};

let result = evaluate_policy(&rules, &context, &RejectRateLimits);
match result {
    PolicyEvaluationResult::Allowed(draft) => { /* agentd 持久化完整 decision */ }
    PolicyEvaluationResult::RequiresConfirmation(mode, draft) => { /* 创建审批流程 */ }
    PolicyEvaluationResult::Denied(draft) => { /* 普通匹配 deny */ }
    PolicyEvaluationResult::BlockedByKernelInvariant(block) => { /* Stop Fence / Recovery */ }
    PolicyEvaluationResult::Error(error) => { /* fail closed */ }
}
```

`PolicyEvaluationContext` 只接收 Kernel 已观察或已有事实：Actor、当前 EntryPoint、多条 ContentOrigin、Task/Action ID、plan version、URI resource refs、capability/operation、SideEffectClass、structured arguments、Delegation 覆盖 evidence、本地 presence evidence、UTC instant、security mode 上下文与 `KernelInvariantState`。

`owner`、本机入口、security mode、S4/S5 均不产生默认权限或默认确认。

## Invariant 优先

`KernelInvariantState::StopFence` / `Recovery` 在规则解析和匹配前返回 `BlockedByKernelInvariant`：

- 不是 `PolicyRuleEffect::Deny`；
- 不创建隐藏 Policy rule；
- Stop Fence generation / recovery reason 由 Kernel 输入；
- 即使规则损坏，active invariant 仍先阻断普通副作用边界。

## URI 规范化 API

`domain-policy` 公开唯一的 Policy URI parser/normalizer 表面，供 Kernel 内其他 crate 复用，不暴露 matcher 的 `NormalizedUri`、score 或匹配内部类型：

```rust
use domain_policy::{normalize_uri, normalize_uri_pattern, PolicyError};

fn normalize_inputs(value: &str, pattern: &str) -> Result<(String, String), PolicyError> {
    Ok((normalize_uri(value)?, normalize_uri_pattern(pattern)?))
}
```

- `normalize_uri(&str)`：规范化一条普通 URI；任何 glob token 都返回 `PolicyErrorCode::InvalidUriPattern`。
- `normalize_uri_pattern(&str)`：规范化一条 URI pattern；只允许 path 中完整 segment `*` / `**`，query/fragment 仍为精确字符串，scheme/authority 不允许 glob。
- 两者都执行 scheme/host 小写、默认端口移除、dot segments 消解、百分号十六进制大写、RFC 8089 `file:` 校验与 Windows drive 大写；反斜杠和非法 authority 均 fail closed。
- API 故意只处理单项，不提供数组 normalizer；调用方负责保持数组顺序与重复项，并决定错误如何归属。当前 `task.create` repository 已按契约逐项调用，完整 fixture 与 repository 测试覆盖顺序、重复和 hash。

## Pattern 与排序

- URI value/pattern 都规范化：scheme/host 小写、默认端口移除、dot segments、百分号十六进制大写；保留字符不解码；反斜杠拒绝；file drive 大写。
- URI glob 只有完整 segment `*` / `**`；query/fragment 若 pattern 提供则精确匹配，缺省表示不限制该部分。
- capability/operation 只有精确值或末尾 `.*`。
- 空数组/缺省不限制；resource exclude 优先；resource 维度受限但 Action 无 resource 时不匹配。
- 多 ContentOrigin：若 kind 与 source 都受限，必须由同一 origin 同时满足，不能跨 origin 拼接。
- specificity 只使用本次实际命中的最具体备选；exclude 不加分。
- 最终顺序：priority、specificity tuple、deny > confirm > allow、revision、ID UTF-8 升序。

## Condition v1

- `time_window`：IANA timezone、weekday、本地半开区间、跨午夜、start=end 全天；UTC instant 经 chrono-tz 投影，DST 不猜偏移。
- `delegation_required` / `local_presence_required`：true 与 false 都是精确条件。
- 未知 condition 可用 `parse_policy_rule_json` 在 typed decode 前识别，返回 `unsupported_policy_condition`。
- timezone、时间、重复 weekday、rate-limit scope 所需事实缺失等语义异常同样 fail closed，不变成未匹配。

## RateLimitPort 与原子消费

生产实现必须由 agentd 注入持久化的 `RateLimitPort`，crate 没有全局内存兜底：

1. `preview` 非消费检查所有 otherwise-matching 候选；
2. 完成确定性排序；
3. 只对当前 winner 调用原子 `check_and_consume`；
4. 若并发导致 `Exceeded`，移除该规则并重新选择下一候选；
5. 所有规则耗尽后 Freedom-first Default Allow。

因此输掉的候选永不消费。测试中的 Mutex fixture 仅验证滚动窗口边界和并发原子语义，不是生产实现。

## Decision draft 与哈希边界

`PermissionDecisionDraft` 不是持久对象，不包含：

- PermissionDecision ID；
- `evaluated_at`（只在 canonical input 中保留 Kernel 输入 instant）；
- `decision_revision`；
- `policy_set_revision`；
- 最终 `evaluation_context_hash`。

crate 使用 `kernel-contracts::sha256_canonical`（RFC 8785 + SHA-256）计算 structured arguments 的 `key_params_hash`，并规范化、排序、去重 resource refs。完整判定上下文尚无独立持久 Schema，因此返回 `CanonicalEvaluationInput`；agentd 后续必须加入其拥有的 policy-set revision 等字段，按最终 Schema canonicalize/hash，不得把此草稿冒充持久 PermissionDecision。

## 错误

| code | 含义 |
|---|---|
| `invalid_policy_uri_pattern` | URI/segment glob/file 形式非法 |
| `invalid_policy_action_pattern` | capability/operation pattern 非精确或末尾 `.*` |
| `invalid_policy_timestamp` | expires_at 运行时解析失败 |
| `invalid_policy_rule` | effect/mode、revision 等关键语义异常 |
| `unsupported_policy_condition` | Condition v1 不支持/无效/缺必要事实 |
| `canonicalization_failed` | RFC 8785 输入失败 |
| `rate_limit_failed` | authoritative port 缺失或失败 |

错误均返回 `PolicyEvaluationResult::Error`，不得转成 Default Allow。
