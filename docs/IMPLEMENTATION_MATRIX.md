# Shittim 实现矩阵

> 本矩阵只汇总状态，不取代 `specs/` 中的唯一事实源。

| 领域 | 规范状态 | Schema | 实现 | 自动化测试 | 备注 |
|---|---|---|---|---|---|
| Task/Action 状态机 | 已消歧 | 未开始 | 未开始 | 未开始 | rolling_back、Approval、Lease、补偿语义已定义 |
| Recovery/Verification | 已消歧 | 未开始 | 未开始 | 未开始 | Candidate、Attempt、retry 约束已定义 |
| Policy matcher | 已消歧 | 未开始 | 未开始 | 未开始 | URI glob、Condition v1、Specificity 已定义 |
| ContentOrigin/Actor/EntryPoint | 已定义 | 未开始 | 未开始 | 未开始 | v1 auth 只能 null；owner为未认证预留标签 |
| PermissionDecision | 已定义 | 未开始 | 未开始 | 未开始 | hash/revision/ref 已明确 |
| Event/SQLite Outbox | 已消歧 | 未开始 | 未开始 | 未开始 | cursor 只用 outbox_position |
| KCP Envelope | 已定义 | 未开始 | 未开始 | 未开始 | protocol 1.0；非 JSON-RPC 契约 |
| KCP 首批八个方法 | 已定义 | 未开始 | 未开始 | 未开始 | 无 Stop Fence 清除方法 |
| 首批三个事件 | 已定义 | 未开始 | 未开始 | 未开始 | 点号小写事件名 |
| KCP 本地传输 | ADR accepted | 不适用 | 未开始 | 未开始 | Unix Socket / Windows Named Pipe |
| Rust workspace | ADR accepted | 不适用 | 未开始 | 未开始 | 当前 rustc/cargo 1.97.0 |
| TypeScript workspace | ADR accepted，工具链阻塞 | 不适用 | 未开始 | 未开始 | 需 Node 24 LTS；当前 Node 26.4.0 |
| pnpm workspace | ADR accepted | 不适用 | 未开始 | 未开始 | 选择 pnpm 11.3.0 |
| Desktop client | 方向已定义 | 未开始 | 未开始 | 未开始 | 依赖版本由首次 lockfile 固定 |
| Extension SDK | 规范已有 | 未开始 | 未开始 | 未开始 | 无可安装 SDK 包 |
| Provider | 仅接口边界 | 未开始 | 未开始 | 未开始 | 未实现、不得伪造真实副作用 |

## 状态含义

- **已定义/已消歧**：规范足以进入 Schema 或实现设计，不代表代码存在。
- **ADR accepted**：实施选择已接受，但仍可能尚未落地。
- **工具链阻塞**：现有环境不满足已接受约束，禁止用不符合条件的环境伪称完成。
- **未开始**：没有 Schema、代码或测试产物。

## 相关入口

- [进度](PROGRESS.md)
- [API 文档](api/README.md)
- [SDK 文档](sdk/README.md)
- [ADR 索引](../adr/README.md)
