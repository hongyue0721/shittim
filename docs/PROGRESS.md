# Shittim 实现进度

> 状态日期：规范基线 `5be2018` 之后的首批 Kernel 契约修订工作树。

## 当前阶段

当前只有架构规范、契约和 ADR，代码实现尚未开始。仓库内还没有 Rust crate、TypeScript package、JSON Schema 源文件、生成类型、可运行的 `agentd`、`agent-runtime`、桌面客户端或 SDK。

本轮完成的是编码前消歧：Task/Action 恢复状态、Policy pattern/condition/specificity、Event/Outbox、ContentOrigin、KCP 首批目录、Schema 生成规则与本地传输决策。

## 已完成

- [x] 建立 Freedom-first、Kernel Owns Reality 与 Core 不可自改规范基线。
- [x] 明确 Task `rolling_back` 入口与外部补偿不是 SQLite rollback。
- [x] 明确 Action confirm、ApprovalRecord、Lease 过期及补偿 Action 语义。
- [x] 明确 Policy pattern、Condition v1、Specificity 与稳定排序。
- [x] 明确 EventEnvelope、全局 `outbox_position`、cursor 与首批事件目录。
- [x] 明确 KCP v1 Envelope、首批八个方法、错误目录、版本字段与 auth/deadline 行为。
- [x] 明确 Shittim 首批 `owner` 仅为未认证预留标签，第一版不产生 Owner 权限。
- [x] 明确 `stop.activate` 即 Emergency Stop 入口，Fence 第一版持久且不可由 Security Mode 暗中解除。
- [x] 接受首批工具链、Schema 生成和 KCP 本地传输 ADR。

## 未开始

- [ ] 创建 Rust workspace 与 `agentd` 基础 crate。
- [ ] 创建 `schemas/source/`、Schema manifest 与生成器。
- [ ] 生成 Rust/TypeScript 类型与 validator。
- [ ] 实现 SQLite migration、Task/Action/Policy/Outbox 存储。
- [ ] 实现 Unix Domain Socket / Windows Named Pipe KCP server/client。
- [ ] 实现 conformance 自动化测试。
- [ ] 创建 Node 24 LTS 环境、TypeScript workspace 与 pnpm lockfile。
- [ ] 创建 Tauri/React/AntD 客户端。
- [ ] 发布 Extension SDK 生成物和示例。

## 当前阻塞

- 当前环境 Node 为 `26.4.0`，不满足项目要求的 Node 24 LTS。开始 Node/TypeScript workspace 前必须提供并锁定 Node 24 LTS；详见 [`../adr/0001-shittim工作区与工具链.md`](../adr/0001-shittim工作区与工具链.md)。
- Schema 源和生成命令尚未实现，因此 API/SDK 文档只能描述规范状态，不能声称已有可导入包或可运行服务。

## 下一批建议顺序

1. 落地 `schemas/source/` 与可重复生成工具链。
2. 建立 Rust workspace，先实现纯领域状态机和 Policy matcher。
3. 实现 SQLite schema、Outbox 与 KCP transport。
4. 按 `specs/CONFORMANCE.md` 增加契约、属性和恢复测试。
5. Node 24 LTS 可用后再创建 TypeScript client/SDK 与桌面端。

## 事实来源

- 全局不变量：[`../AGENT.md`](../AGENT.md)
- 运行时与状态机：[`../specs/CORE_ARCHITECTURE.md`](../specs/CORE_ARCHITECTURE.md)
- Policy：[`../specs/SECURITY_PRIVILEGE.md`](../specs/SECURITY_PRIVILEGE.md)
- KCP/对象/Schema：[`../specs/IMPLEMENTATION_CONTRACTS.md`](../specs/IMPLEMENTATION_CONTRACTS.md)
- 自动化验收：[`../specs/CONFORMANCE.md`](../specs/CONFORMANCE.md)
