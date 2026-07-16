# Shittim 实现进度

> 状态日期：首批 Rust workspace + Schema 单一生成源落地后。

## 当前阶段

已建立可重复的 Rust workspace、JSON Schema 2020-12 源、`schemas/manifest.json`、受限确定性 codegen（`schema-tool`）以及 `kernel-contracts` 生成类型 / manifest 目录 / Command、Query、Event typed decode / 校验 / RFC 8785 哈希 API。Response 根据原请求方法使用独立 response Schema 解码，不伪称已有方法级 typed Response Envelope。

尚未实现业务状态机、SQLite、KCP server、agentd 运行时、TypeScript workspace 或任何 Provider/Extension。

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
- [x] 创建 Rust workspace（`rust/Cargo.toml`）与 `rust-toolchain.toml`（1.97.0）。
- [x] 创建 `schemas/source/` 首批 common/task/policy/event/kcp JSON Schema 2020-12 源。
- [x] 创建 `schemas/manifest.json`（$id 唯一、source 路径、kind、generation targets）。
- [x] 实现 `schema-tool` CLI：`generate` / `check` / `validate` / `canonicalize`。
- [x] 实现 `kernel-contracts`：生成类型与目录、由 conditional Schema 自动派生的 KCP/Event tagged payload decode、Draft 2020-12 校验、RFC 8785 + SHA-256 小写 API。
- [x] `schema-tool check` 执行官方 Draft 2020-12 meta-schema 校验、跨文件 `$ref` 编译与生成漂移检查。
- [x] JCS 使用 `serde_json_canonicalizer` 0.3.2，并提供可复用 RFC/UTF-16 fixture。
- [x] 提供 `scripts/check-schema.sh` 入口（纯 Rust/cargo，无 Node 依赖），并检查已跟踪生成物无 Git 漂移。
- [x] 添加 Apache-2.0 根许可证。

## 未开始

- [ ] 实现纯领域 Task/Action 状态机与 Policy matcher。
- [ ] 实现 SQLite migration、Task/Action/Policy/Outbox 存储。
- [ ] 实现 Unix Domain Socket / Windows Named Pipe KCP server/client。
- [ ] 实现 conformance 自动化测试全量（当前仅 Schema/契约子集）。
- [ ] 创建 TypeScript workspace 与 pnpm lockfile（Node 24.18.0 已可用，但本轮未建 TS）。
- [ ] 创建 Tauri/React/AntD 客户端。
- [ ] 发布 Extension SDK 生成物和示例。
- [ ] 实现 agentd 进程与业务命令处理。

## 当前阻塞

- Node 24 LTS 阻塞已解除（实际可用 24.18.0 via pnpm user runtime）；但 TypeScript workspace 仍未创建，不得声称 TS 包存在。
- 领域状态机、存储与 KCP 传输尚未实现；API/SDK 文档只能描述 Schema 生成物与规范状态，不能声称可运行服务。

## 下一批建议顺序

1. 在 `kernel-contracts` 之上实现纯领域状态机与 Policy matcher（无 IO）。
2. 实现 SQLite schema、Outbox 与 revision/幂等持久化。
3. 实现 KCP 本地传输（ADR-0003）与首批八方法处理。
4. 按 `specs/CONFORMANCE.md` 扩展契约、属性和恢复测试。
5. 再创建 TypeScript client/SDK 与桌面端，并从同一 Schema 源生成 TS 类型。

## 事实来源

- 全局不变量：[`../AGENT.md`](../AGENT.md)
- 运行时与状态机：[`../specs/CORE_ARCHITECTURE.md`](../specs/CORE_ARCHITECTURE.md)
- Policy：[`../specs/SECURITY_PRIVILEGE.md`](../specs/SECURITY_PRIVILEGE.md)
- KCP/对象/Schema：[`../specs/IMPLEMENTATION_CONTRACTS.md`](../specs/IMPLEMENTATION_CONTRACTS.md)
- 自动化验收：[`../specs/CONFORMANCE.md`](../specs/CONFORMANCE.md)
- Schema 生成命令：[`api/schema-generation.md`](api/schema-generation.md)
