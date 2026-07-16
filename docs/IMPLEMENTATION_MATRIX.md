# Shittim 实现矩阵

> 本矩阵只汇总状态，不取代 `specs/` 中的唯一事实源。

| 领域 | 规范状态 | Schema | 实现 | 自动化测试 | 备注 |
|---|---|---|---|---|---|
| Task/Action 状态机 | 已消歧 | 已有首批类型 Schema | 未开始 | 未开始 | rolling_back、Approval、Lease、补偿语义已定义；状态机逻辑未编码 |
| Recovery/Verification | 已消歧 | 已有 Schema | 未开始 | Schema 校验子集 | Candidate、Attempt、VerificationResult 类型已生成 |
| Policy matcher | 已消歧 | PolicyRule/PermissionDecision/ApprovalRecord Schema 已有 | 未开始 | confirm/allow/deny 条件约束已测 | URI glob/specificity 未实现 |
| ContentOrigin/Actor/EntryPoint | 已定义 | 已有 Schema + 生成类型 | 类型/校验 | 未知 enum/字段拒绝已测 | v1 auth 只能 null；owner 为未认证预留标签 |
| PermissionDecision | 已定义 | 已有 Schema | 类型/校验 | 未开始业务评估 | hash/revision/ref 字段已建模 |
| Event/SQLite Outbox | 已消歧 | EventEnvelope + 3 payload Schema | typed decode；未开始存储/发布 | payload 错配、cursor、事件闭集已测 | cursor 只用 outbox_position |
| KCP Envelope | 已定义 | command/query/response + error Schema | command/query/event typed decode；response 仅 envelope 校验与开放 payload | auth/protocol/version/错配 payload/ok-error 已测 | protocol 1.0；非 JSON-RPC |
| KCP 首批八个方法 | 已定义 | 8 方法 request/response Schema | 未开始 server | 方法 enum 闭集已测 | 无 Stop Fence 清除方法 |
| 首批三个事件 | 已定义 | payload Schema | 未开始发布 | 示例校验 | 点号小写事件名 |
| KCP 本地传输 | ADR accepted | 不适用 | 未开始 | 未开始 | Unix Socket / Windows Named Pipe |
| Schema 生成链 | ADR accepted | 源 + manifest 已落地 | types + generated catalog + typed decode | generate twice / meta-schema / drift / JCS | 自有受限 codegen；JCS 用成熟库；仅 Rust |
| Rust workspace | ADR accepted | 不适用 | 已落地 | fmt/clippy/test | rustc/cargo 1.97.0；无假 agentd |
| TypeScript workspace | ADR accepted | 不适用 | 未开始 | 未开始 | Node 24.18.0 可用，本轮未建 TS |
| pnpm workspace | ADR accepted | 不适用 | 未开始 | 未开始 | 选择 pnpm 11.3.0 |
| Desktop client | 方向已定义 | 未开始 | 未开始 | 未开始 | 依赖版本由首次 lockfile 固定 |
| Extension SDK | 规范已有 | 未开始 | 未开始 | 未开始 | 无可安装 SDK 包 |
| Provider | 仅接口边界 | 未开始 | 未开始 | 未开始 | 未实现、不得伪造真实副作用 |

## 状态含义

- **已定义/已消歧**：规范足以进入 Schema 或实现设计，不代表代码存在。
- **ADR accepted**：实施选择已接受，但仍可能尚未落地。
- **类型/校验**：有 JSON Schema、生成类型与运行时校验，无业务状态所有者。
- **未开始**：没有对应业务实现或传输实现。

## 相关入口

- [进度](PROGRESS.md)
- [API 文档](api/README.md)
- [Schema 生成](api/schema-generation.md)
- [SDK 文档](sdk/README.md)
- [ADR 索引](../adr/README.md)
