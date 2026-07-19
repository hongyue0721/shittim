# Shittim Architecture Specification v3

**Shittim** 是本项目与产品品牌；**Companion** 是系统中与用户长期交互的 AI 角色概念，不是仓库或产品的临时名称。Shittim 是可直接编码的长期个人 AI 系统规范，由稳定可信核心、可治理策略与可持续生长的能力组成：核心不会被 Agent 自改；Memory、Learned Style、Self Preferences、Skill、Trigger、Delegation、受治理配置、模型/Provider 路由、Extension 及恢复知识可以在 Policy 约束内持续迭代。

## 权威性与阅读顺序

1. `AGENT.md` 是宪法：全局不变量、可信核心边界、依赖方向与编码规则。
2. `specs/*.md` 是各领域的唯一规范事实源；同一字段、枚举、状态机不得在其他文档复制定义。
3. [`adr/`](adr/README.md) 记录未被规范覆盖的局部实施决策；ADR 状态为 proposed/accepted/superseded，accepted 不等于代码已经完成。ADR-0006 的首批12项 Schema、Task creation纯library与official fixtures/harness已经实现，但repository、handler、child materializer与production cutover尚未完成；ADR-0007 仍为contract-only。
4. 源码、Schema 与测试必须实现本规范，不能反向改变规范。

`PROJECT_OVERVIEW.md` 是非规范产品概览，`FILE_MANIFEST.md` 是非规范元数据（由 `scripts/update-file-manifest.mjs` 从 Git Markdown source set 生成，禁止手改）；二者不定义行为。冲突时，以 AGENT 的硬不变量、再以对应领域 spec 为准。

## 规范索引

- [`specs/CORE_ARCHITECTURE.md`](specs/CORE_ARCHITECTURE.md)：运行时、Task/Action、事件、恢复、Stop Fence。
- [`specs/SECURITY_PRIVILEGE.md`](specs/SECURITY_PRIVILEGE.md)：Freedom-first Policy、入口、来源、特权与停止。
- [`specs/IDENTITY_MEMORY_INITIATIVE.md`](specs/IDENTITY_MEMORY_INITIATIVE.md)：身份、记忆、探索、主动性与可生长对象。
- [`specs/EXTENSION_SDK.md`](specs/EXTENSION_SDK.md)：扩展生命周期、边界、安装、调用与声明性风险。
- [`specs/MODEL_RUNTIME.md`](specs/MODEL_RUNTIME.md)：Pi、模型路由、Context Pack 与云调用记录。
- [`specs/COMPUTER_USE.md`](specs/COMPUTER_USE.md)：未来可选的 Computer Use Extension Profile 事实源，定义桌面观察、Execute Gate、输入租约与验证；核心产品完成不依赖它。
- [`specs/IMPLEMENTATION_CONTRACTS.md`](specs/IMPLEMENTATION_CONTRACTS.md)：版本化对象、Kernel Control Protocol 与编码契约。
- [`specs/CONFORMANCE.md`](specs/CONFORMANCE.md)：必须自动化的验收测试。
- [`specs/REFERENCES.md`](specs/REFERENCES.md)：直接依赖、可选 Provider 与仅参考项目的边界。

## 文档与决策入口

- [`docs/PROGRESS.md`](docs/PROGRESS.md)：中文实现进度与当前阻塞。
- [`docs/IMPLEMENTATION_MATRIX.md`](docs/IMPLEMENTATION_MATRIX.md)：规范、Schema、实现和测试状态矩阵。
- [`docs/api/README.md`](docs/api/README.md)：KCP、事件与错误文档入口；当前Rust已实现legacy TaskCreate v1 create/get与不可连接dispatcher，以及ADR-0006首批Schema、Task creation纯library和official fixtures/harness；active TaskCreate v2 repository/handler、child Action materializer、Approval v2、`agentd`与可连接server尚未实现。
- [`docs/sdk/README.md`](docs/sdk/README.md)：Extension SDK Base 文档入口；Base 是基础产品必做，当前无可发布 SDK。
- [`adr/README.md`](adr/README.md)：已接受架构决策索引。

## 实现目标

基础安装仅有两个常驻后台运行时：Rust `agentd` Kernel 与 TypeScript/Pi `agent-runtime`；桌面客户端是可关闭重连的 Tauri 客户端。所有现实副作用都走唯一链路：`Task -> Policy -> agentd -> Broker/Extension -> system mechanism -> verify/audit`。无匹配用户规则时 Policy **allow**；确认和拒绝来自匹配规则、系统机制或运行恢复需要，不是默认保守矩阵。

项目代码采用 [Apache License 2.0](LICENSE)。

编码前必须读 `AGENT.md` 和受影响 spec；改动领域定义时只改其唯一事实源，并更新 `CONFORMANCE.md` 测试锚点。
