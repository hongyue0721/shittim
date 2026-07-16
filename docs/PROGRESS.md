# Shittim 实现进度

> 状态日期：`domain-task` 纯领域状态机通过独立验收后。

## 当前阶段

已完成 Rust/Schema 契约基座与 `domain-task` 纯领域 Task/Action 状态机。当前仍没有 SQLite、真实 Outbox、KCP server、`agentd`、TypeScript workspace、桌面客户端或 Provider。

`domain-task` 只计算状态图、不变量、revision/plan_version 和待持久化意图，不保存事实，也不分配真实 EventEnvelope。

## 已完成

### 规范与工程基线

- [x] 建立 Freedom-first、Kernel Owns Reality、Core 不可自改规范基线。
- [x] 补齐 Task/Action/Recovery、Policy、Event/Outbox、KCP 首批可编码契约。
- [x] 明确 `owner` 只是未认证预留标签；`stop.activate` 是首批 Emergency Stop 入口。
- [x] 接受工作区、Schema 生成和 KCP 本地传输 ADR。
- [x] 添加 Apache-2.0 根许可证。

### Schema 与 Rust 契约

- [x] 创建 Rust workspace 与 `rust-toolchain.toml`（1.97.0）。
- [x] 创建 40 个 Draft 2020-12 Schema 和 `schemas/manifest.json`。
- [x] 实现 `schema-tool generate/check/validate/canonicalize`。
- [x] 从 Schema 自动生成 Rust 类型、catalog 及 Command/Query/Event typed decode。
- [x] 执行 meta-schema、跨文件 `$ref`、生成漂移和未知关键字检查。
- [x] 使用 `serde_json_canonicalizer` 实现 RFC 8785，并提供共享测试向量。
- [x] `scripts/check-schema.sh` 覆盖重复生成、fmt、Clippy、workspace tests 和生成物 Git 漂移。

### Task/Action 纯领域状态机

- [x] 新增 `rust/crates/domain-task`，直接使用生成的 TaskStatus/ActionStatus，不复制状态枚举。
- [x] 实现 CORE §10 Task 状态图、revision 和 plan_version 规则；兼容 `task.create` 的 `plan_version=0`。
- [x] `succeeded` 按 `TaskSpec.success_criteria` 完整字符串**多重集合**精确覆盖，每个 occurrence 均需 `verified_ok`。
- [x] `partially_completed` 和 `rolling_back` 均要求明确副作用引用，不凭状态猜测事实。
- [x] 实现 CORE §11 Action 状态图；confirm 是 pending metadata update，不是假装 approved。
- [x] `completed`/`failed` 要求 Verification 事实；不确定结果要求 crash/timeout/ambiguous 等结构化原因。
- [x] Lease 过期与确定未派发取消返回绑定 action_id 的原子释放意图。
- [x] 补偿身份只由 `ActionRequest.parent_action_id` 推导，不存在平行 ActionRole。
- [x] `retry_original` 仅在副作用明确未发生且幂等保障成立时合法。
- [x] 新增 NxN 矩阵、证据测试与 proptest；`domain-task` 共 47 项测试。
- [x] 新增 [`api/domain-task.md`](api/domain-task.md)；本批无外部 SDK API 变化。

## 未完成

- [ ] 实现 Freedom-first Policy matcher、URI pattern、specificity 和 Condition v1。
- [ ] 实现 SQLite migration、Task/Action/Policy/Outbox repository。
- [ ] 实现请求幂等、乐观锁、Event cursor 和原子 Outbox。
- [ ] 实现 Unix Domain Socket / Windows Named Pipe KCP server/client。
- [ ] 实现 `agentd` 组合根和首批八个 KCP 方法处理。
- [ ] 创建 TypeScript workspace、SDK client 与 Pi `agent-runtime`。
- [ ] 创建 Tauri/React/Ant Design 蓝白桌面客户端。
- [ ] 实现 Extension SDK、Provider、Memory、Initiative、Computer Use 与 Broker。
- [ ] 完成 `specs/CONFORMANCE.md` 全量自动化测试。

## 当前阻塞

- Node 24 LTS 已可用（24.18.0，pnpm user runtime），TypeScript 工具链不再受版本阻塞。
- 真实模型 Provider、远程 Channel、跨平台 Provider 与 Privilege Broker 仍需要后续真实环境和用户选择；当前没有伪造支持。

## 下一步

1. 实现纯 `domain-policy` matcher 和完整规则排序测试。
2. 建立 SQLite migration、repository 与原子 Outbox。
3. 实现 KCP 本地传输和 Task 创建/查询/Event 轮询纵切。
4. 再建立 TypeScript client/SDK 和 Ant Design 桌面端。

## 最近验证

```text
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path rust/Cargo.toml --workspace
./scripts/check-schema.sh
git diff --check
```

全部通过；`domain-task` 47 项测试，当前 workspace 共 89 项测试。

## 事实来源

- 全局不变量：[`../AGENT.md`](../AGENT.md)
- 状态机与恢复：[`../specs/CORE_ARCHITECTURE.md`](../specs/CORE_ARCHITECTURE.md)
- 实现契约：[`../specs/IMPLEMENTATION_CONTRACTS.md`](../specs/IMPLEMENTATION_CONTRACTS.md)
- 验收：[`../specs/CONFORMANCE.md`](../specs/CONFORMANCE.md)
- Schema：[`api/schema-generation.md`](api/schema-generation.md)
- 状态机 API：[`api/domain-task.md`](api/domain-task.md)
