# Shittim 实现矩阵

> 本矩阵只汇总状态，不取代 `specs/` 中的唯一事实源。

| 领域 | 规范状态 | Schema | 实现 | 自动化测试 | 备注 |
|---|---|---|---|---|---|
| Task/Action 状态机 | 已消歧 | 已有首批类型 Schema | `domain-task` 纯领域实现 | NxN、证据、proptest | plan_version=0；success 字符串多重集合；parent_action_id 是补偿唯一事实 |
| Recovery/Verification | 已消歧 | Candidate/Attempt/Verification Schema | 验证摘要与 retry_original 合法性 | completed/failed/unknown/retry 测试 | 其它恢复候选只做枚举层接受，不代表授权或执行 |
| Policy matcher | 已消歧；ContentOrigin 多值同一-origin 语义已补充 | PolicyRule/PermissionDecision/ApprovalRecord Schema | `domain-policy` 纯领域实现 | URI/glob、排序、Condition v1、rate-limit 并发/proptest | Default Allow；Stop/Recovery 独立 Blocked；无持久化 rate-limit 实现 |
| ContentOrigin/Actor/EntryPoint | 已定义 | Schema + 生成类型 | 类型与运行时校验 | enum/未知字段/auth 测试 | owner 是未认证预留标签 |
| PermissionDecision | 已定义 | Schema + 生成类型 | `domain-policy` 生成非持久 draft/binding/canonical input | decision mapping、default allow、RFC 8785 key params | ID/evaluated_at/decision revision/policy set revision/最终 context hash 由未来 agentd 持久层拥有 |
| AuditRecord | 已定义：任务创建原因、外发状态/manifest refs、PD/policy context、权威 rollback projection、provider/model 分工及同事务失败回滚 | AuditRecord v1 + 2 个 wrapper 示例 + 生成 Rust 类型 | 仅 Schema 内运行时校验；无 Audit Store/跨对象 repository | creation context 正反、external not_sent/sent/unknown、required、唯一数组、未知字段、system null actor | PD 字段相等、rollback/provider 双源一致性仅 Conformance；ModelCallRecord/PayloadManifest/Delegation 无 source Schema；SQLite 未开始 |
| Event/SQLite Outbox | 已消歧；sequence 首条已提交为 0、后续连续 +1、回滚不占号 | EventEnvelope + 3 payload；task.created 示例 sequence=0 | typed decode；domain-task 只产 EventIntent | payload/cursor/闭集/sequence=0 合法测试 | 无真实 sequence/outbox_position 分配 |
| KCP Envelope | 已定义 | command/query/response/error | Command/Query/Event typed decode | auth/protocol/错配/ok-error | Response 根据原请求方法使用独立 response Schema |
| KCP 首批八方法 | 已定义 | 8 组 request/response Schema | 尚无 server/handler | 方法闭集与 payload 绑定 | 无 Stop Fence 解除方法 |
| 首批三个事件 | 已定义 | 3 个 payload Schema | 尚无发布器 | 类型与 payload 错配测试 | 点号小写 |
| KCP 本地传输 | ADR accepted | 不适用 | 未开始 | 未开始 | Unix Socket / Windows Named Pipe |
| Schema 生成链 | ADR accepted | 41 个 source + manifest | schema-tool + kernel-contracts | meta/$ref/drift/JCS | 当前只生成 Rust |
| Rust workspace | ADR accepted | 不适用 | kernel-contracts、schema-tool、domain-task、domain-policy | fmt/clippy/workspace test | rustc/cargo 1.97.0 |
| TypeScript workspace | ADR accepted | 尚无 TS 生成物 | 未开始 | 未开始 | Node 24.18.0 已可用 |
| Desktop client | 方向已定义 | 未开始 | 未开始 | 未开始 | 将使用 Tauri/React/AntD，蓝白配色 |
| Extension SDK | 规范已有 | 未开始 | 未开始 | 未开始 | 当前无可安装 SDK 包 |
| Provider/平台能力 | 仅接口边界 | 未开始 | 未开始 | 未开始 | 不伪造支持 |

## 状态含义

- **纯领域实现**：只计算规则和意图，不拥有持久化或外部副作用。
- **类型与运行时校验**：有 Schema/生成类型，不代表业务状态所有者已实现。
- **未开始**：没有对应实现或真实能力。

## 相关入口

- [进度](PROGRESS.md)
- [API 文档](api/README.md)
- [domain-task API](api/domain-task.md)
- [domain-policy API](api/domain-policy.md)
- [AuditRecord v1](api/audit-record.md)
- [Schema 生成](api/schema-generation.md)
- [SDK 文档](sdk/README.md)
- [ADR 索引](../adr/README.md)
