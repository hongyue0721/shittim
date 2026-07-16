# AGENT.md — Companion Implementation Constitution

本文件是面向维护者和编码 Agent 的项目宪法，不复制领域字段、枚举或状态机；那些定义只在对应 `specs/` 中存在。

## 1. 全局不变量

- **Kernel Owns Reality**：`agentd` 是 Task、Action、Policy、Audit、Memory 事实、资源锁和恢复的唯一权威；模型、UI、Provider、Extension 输出不是事实或权限。
- **Freedom-first**：Policy 是 Default Allow。没有命中规则时必须 `allow`；`confirm`、`deny`、本地确认、系统认证和计划修订均只能来自匹配规则、底层系统机制或明确恢复状态，不能被风险等级偷偷默认化。
- **User-defined guardrails**：用户与 Companion 都可用自然语言创建、修改、撤销结构化 Policy Rule、Exploration Scope、Delegation、Trigger；第一版没有 Owner 唯一身份认证，不得假称“只有 Owner 能改”。所有变更保留 actor、entry point、来源证据与审计。
- **Core-prohibited/self-modification boundary**：Agent、Skill、Extension、Provider 不得读取后修改、补丁、热替换或重写 agentd Kernel、Task/Policy 解释器、Audit 完整性机制、Privilege Broker、Emergency Stop、Core Identity、安全/隔离边界或私有 Kernel API。正式开发者发布升级可更新 Core。
- **Growth is allowed**：Memory、Learned Style、Self Preferences、Skill、Trigger、Delegation、Governed Configuration、模型/Provider 路由、Extension 源码/安装/更新/回滚、Provider Adapter、任务策略及恢复知识可由 Companion 版本化生成和迭代，受 Policy、预算、停止条件、可观测性和回滚治理；这些不是默认审批。
- **Truthful boundaries**：必须区分 OS-enforced、host-enforced 与 declaration-only 权限，特别是未沙箱 Native Extension；不得把声明伪装为隔离。
- **Verification over assertion**：Provider 成功不等于目标成功。副作用必须归属 Task/Action，并按规则和能力验证、审计、恢复。
- **Identity honesty**：Companion 承认自己是 AI，不冒充用户。机械转发用户原文可不标 AI；AI 起草且用户批准的最终文可按用户授权发送；按 Delegation 自主生成外发必须避免冒充并留存来源/审计。

## 2. 可信边界与依赖方向

允许：`client -> agentd`、`agent-runtime -> Kernel Control Protocol`、`agentd -> domain/extension protocol`、`extension -> extension protocol`、`Broker -> fixed system mechanism`。禁止 UI 或 runtime 直连平台能力、Extension 互调、Extension 直接使用 Broker 凭据、Provider 写 Kernel 事实、模型输出直写权限。

第三方代码不得载入 agentd 地址空间。Extension 不得成为 Core 或绕过 Task、Policy、Scope、Lease、审计链。普通系统 Shell 与 Broker 固定特权 Action 是两条不同通道；Agent 不得用前者规避后者或改可信 Core。

## 3. 编码规则

- Rust stable/Tokio、SQLite + FTS5、版本化 JSON Schema；TypeScript strict、Node LTS、Pi；Tauri 可直接依赖。
- 每个持久对象和协议消息带 `schema_version`；可并发对象带 `revision`。Schema 是单一生成源，禁止手写平行类型。
- 外部调用必须有 deadline、取消、幂等、结构化错误及恢复语义。不可安全重复的外部动作禁止盲目重放。
- 平台差异留在 Provider；Pi、A_Memorix、OmniParser 是可替换依赖或 Provider，MaiBot、Agent-S、UFO 仅作参考；MCP 是兼容桥，不是 Kernel 内部权威协议。
- 修改前读取本文件及相关 spec，先找既有状态所有者；修改后更新测试并运行规定检查。影响新常驻进程、核心协议、状态所有者、Core 边界或特权 Action 时写 ADR。

## 4. 完成条件

实现必须保持唯一状态所有者与依赖方向；所有副作用可取消、超时、验证和审计；停止不依赖模型；删除和纠正传播到派生数据；扩展崩溃不破坏 Kernel；没有用“健壮兜底”掩盖缺失业务事实。详情与可测试枚举见领域 specs。
