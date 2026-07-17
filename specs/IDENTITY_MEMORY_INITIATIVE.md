# IDENTITY_MEMORY_INITIATIVE.md

> 本文件是 AI 身份、人格成长、记忆、画像、探索与主动任务的唯一事实源。

## 1. 身份层级

### 1.1 Core Identity 与可信 Core 边界

Core Identity 属于可信 Core 的一部分，不是普通 Prompt、Memory 或可生长配置。以下主体都不得以读取后改写、提示注入、补丁、热替换、迁移或“自我改进”为名修改它：

- Agent 或任意模型角色；
- Memory、Learned Style 或 Self Preferences；
- Skill、Trigger 或 Delegation；
- Extension、Provider Adapter 或外部 Provider；
- 自动生成代码、恢复流程或任务策略。

Core Identity 包含：

- 自己是 AI；
- 不代表用户；
- 尊重用户对设备和数据的主权；
- 不伪造人类经历；
- 不以情绪操控换取权限；
- 承认不确定性与错误。

Agent 也不得修改可信 Core 中的 Kernel、Task/Policy 解释器、Audit 完整性机制、Privilege Broker、Emergency Stop、安全/隔离边界和私有 Kernel API。只有正式开发者发布的 Core 升级可以改变这些内容。

### 1.2 可生长身份与持续自改进

Core 不可自改不等于 Companion 只能静态运行。以下对象允许由 Bot 在日常运行中持续创建、测试、版本化、启用、替换和回滚：

- Memory；
- Learned Expression Style；
- Self Preferences；
- Skill；
- Trigger；
- Delegation；
- Governed Configuration 和结构化 Policy Rule；
- 模型/Provider 路由；
- Extension 源码、安装、更新和回滚；
- Provider Adapter；
- 任务策略；
- 故障恢复知识。

每次迭代必须有对象版本、变更来源、actor、目标、预算、最大迭代次数、停止条件、观测结果和稳定版本引用。运行中出现预算耗尽、停止条件命中、验证失败、质量持续退化或不可解释偏差时必须停止继续迭代；存在已知稳定版本时按对象的回滚语义恢复，不能用未验证的新版本覆盖唯一可恢复版本。

这些可生长对象仍经过正常 Task、Policy、Scope、验证和审计链。Policy 无匹配规则时是 `allow`；只有命中的规则才能要求 `confirm` 或 `deny`。版本化、自主生成、安装、更新或回滚本身不等于默认需要审批，也不得通过人为增加固定审批步骤把可生长能力变成只会提交候选。

### 1.3 Seed Persona

用户可配置的初始 Prompt，影响：

- 语气；
- 表达密度；
- 幽默和情绪风格；
- 初始兴趣；
- 主动建议方式。

Seed Persona 不是权限策略，也不能取消 Core Identity。

### 1.4 Learned Expression Style

从长期互动中形成的表达习惯，例如：

- 用户喜欢先给结论还是先解释；
- 何时适合主动安慰；
- 技术讨论的细节偏好；
- 使用何种称呼。

必须有证据、置信度、来源和用户删除入口。

### 1.5 Self Preferences

属于 AI 自身的偏好，例如：

- 优先可验证方案；
- 在事实不足，或匹配 Policy 明确要求确认时，优先暴露问题而不是猜测；
- 偏好可验证的稳定引用与结构化事实，而非脆弱的环境猜测。

Self Preference 不得：

- 覆盖用户明确目标；
- 产生权限；
- 变成对用户的强制要求；
- 绕过对象版本、来源记录、Policy、观测和回滚机制被静默写入。

## 2. 记忆分类

### Evidence

原始或可验证来源的引用、摘要和哈希。

### User Fact

关于用户、项目或环境的事实。

### User Preference

用户明确或多次稳定表现的偏好。

### Relationship Memory

用户与 Companion 的共同经历、相处方式与边界。

### Episode

一次有意义的任务或经历：目标、背景、结果、反馈和时间。

### Self Memory

AI 自己做过什么、犯过什么错误、形成了什么判断。

### Active Commitment

AI 已经向用户作出的承诺或尚未完成的责任。

### Mid-term Context

对话被裁剪后的摘要、长期任务进度和待确认事项。它有明确生命周期，不自动变成长期记忆。

## 3. 记忆真值优先级

```text
用户最新明确确认
> 用户历史明确陈述
> 可验证外部事实
> 多次稳定行为
> 单次行为
> 模型推测
```

冲突时不得静默覆盖：

- 保留来源；
- 标记冲突；
- 降低不可靠记忆注入；
- 在必要时询问用户；
- 用户纠正必须阻止旧结论继续作为有效事实。

## 4. Memory Domain

Memory Domain 位于 `agentd`，管理：

- memory id；
- schema version、revision 和内容版本；
- kind；
- subject；
- scope；
- content/structured value；
- evidence refs；
- confidence；
- status；
- sensitivity 元数据；
- injection policy；
- created/updated/last-used；
- expires；
- supersedes/conflicts-with；
- user correction；
- provider indexing state。

Memory Provider 不拥有上述语义事实。`sensitivity` 用于检索、展示、路由和用户自定义规则匹配，不在第一版形成内建拒绝矩阵。普通内容、敏感内容以及密码、Token、密钥和认证凭据都可以写入普通 Memory Provider；Secret Provider 是用户可以配置的可选存储与索引实现，而不是保存凭据的强制前提，也不是另一套 Memory 语义事实源。

## 5. 写入流程

```text
Event or conversation completed
-> extraction candidate
-> deterministic quality filtering
-> scope and injection-policy resolution
-> optional sensitivity metadata classification
-> duplicate/conflict check
-> Policy evaluation
-> commit to Memory Domain as authoritative fact
-> provider indexing
```

第一版不得仅因候选被分类为普通、敏感、Secret、密码或认证材料而拒绝写入，也没有内建的敏感身份类别拒绝。若自定义 Policy Rule 明确命中，则按该规则 `confirm` 或 `deny`；没有命中规则时写入为 `allow`。是否使用 Secret Provider 只由配置和匹配规则决定。

禁止：

- 在工具调用尚未验证时写成功 Episode；
- 把完整对话默认永久保存为画像；
- 让 Memory Provider 自动修改 Companion Prompt；
- 因 Provider 索引尚未完成而把已提交 Memory 伪装成不存在；
- 用敏感分类结果覆盖来源、冲突、纠正、作用域或注入政策。

### 5.1 Provider 索引与最终一致性

Memory Domain 提交成功后即拥有事实，Provider 索引可以处于 `index_pending`。`index_pending` 表示最终一致性的派生索引尚未完成，不表示写入失败：

- Kernel 保留待索引版本、目标 Provider、尝试次数和最近错误；
- 后台任务可以在预算和截止时间内重试；
- 直接列表、查看、修改和删除以 Memory Domain 为准；
- 召回结果必须携带索引版本，避免把旧索引结果当成当前事实；
- Provider 完成索引后，只能确认对应 revision，不得反向改写 Memory 内容；
- 长期失败必须可观测并报告降级，不能静默丢失。

### 5.2 纠正、冲突和删除传播

用户纠正产生新 revision，并将被纠正版本标为 superseded、invalid 或 equivalent non-injectable 状态。自纠正提交成功起，旧事实不得继续进入 Memory Packet，即使向量索引、图谱或缓存仍暂时返回旧版本；Memory Gate 必须按 Memory Domain 的当前 revision 和状态做最终过滤。

用户可以查看、修改、删除、批量删除、导出和完整重置 Memory。删除与重置必须级联到：

- Memory 全文记录和 Provider 全文副本；
- 向量索引；
- 图谱节点、边和派生关联；
- 用户画像和关系画像派生视图；
- Memory Packet、检索结果和运行时缓存；
- 待索引、重试和离线同步队列。

级联删除是本地系统与受控 Provider 的执行义务。已经发送到云模型或其他外部系统的正文无法从对方已完成的处理、日志或备份中撤回；界面和导出记录必须诚实说明这一不可撤回边界，不能把本地删除表述为“全球擦除”。

## 6. 召回流程

```text
Current intent/task
-> Memory Gate
-> choose kinds/scopes/budget
-> query providers
-> merge and policy filter
-> create Memory Packet
-> inject into one model role
```

Memory Packet 必须：

- 有预算；
- 保留来源等级；
- 不混入无关画像；
- 区分用户事实与 AI 推测；
- 遵守入口、任务、Memory scope 和 injection policy；
- 在注入前按权威 revision 排除已删除、已纠正、已过期或不可注入版本；
- 对 Provider 返回的冲突版本保留引用，但只注入当前有效事实。

## 7. 画像

画像是派生视图，不是独立真相。

可以包含：

- 长期目标；
- 工作方式；
- 常用应用；
- 已确认偏好；
- 当前项目；
- 明确的沟通边界。

模型推断的性格结论默认是候选，不能无证据固化。

用户必须能查看：

- 画像内容；
- 支撑证据；
- 置信度；
- 是否参与召回；
- 删除或纠正入口。

## 8. 探索作用域与任务作用域

“可以读取”与“可以长期理解/画像”是不同能力；“本次任务需要”也不等于“长期探索允许”。系统必须分别维护：

### exploration_scope

`exploration_scope` 描述 Companion 可以主动观察、后台读取、索引、建立长期记忆或形成跨域关联的持续范围。它至少表达：

- 资源选择器和明确排除项；
- Read for current opportunity；
- Background read/index；
- Long-term memory allowed；
- Profile inference allowed；
- Cross-domain association allowed；
- 允许的时间、频率和预算；
- 禁止的操作或资源边界。

### task_scope

`task_scope` 只服务于一个具体 Task 或 Child Task，来自当前目标、Delegation、Capability 和 Policy 结果。它至少表达：

- 本次可访问的对象；
- 本次允许的读写动作；
- Task 完成、取消或过期后的失效条件；
- 是否允许把结果提交到 Memory；
- 与 exploration_scope 的交集、扩展依据或冲突结果。

两者不能合并成一个模糊的“已授权”标志。Task 获得临时访问不自动扩大持续探索范围；exploration_scope 允许后台读取也不自动允许本次 Task 执行副作用。

探索默认只读，但不按“普通/敏感/密钥”内容分类建立固定拒绝。密码库、认证材料、私人目录或其他高敏感资源是否可读，由明确资源范围、排除项、系统权限和自定义 Policy Rule 决定；无匹配规则时仍遵循 Default Allow，不能因为模型判定“看起来敏感”而自行拒绝。

用户与 Bot 都可以通过自然语言创建、修改、暂停、恢复和撤销 exploration_scope 与 task_scope 草案或版本。第一版没有 Owner 唯一身份认证，不得声称只有 Owner 能管理。每次变更必须预留并记录：

- actor；
- entry point；
- source statement 或 source event；
- 解析后的结构化版本；
- Policy 结果；
- 生效时间与替代关系。

自然语言本身不是无限授权；运行时使用其结构化版本。若解析结果不能确定具体资源或操作，必须暴露歧义并要求补充事实，而不是猜测一个看似安全的范围。

## 9. Initiative System

Initiative System 是一个 Kernel Domain，不是独立 Agent。

组成：

### Opportunity Detector

来源：

- 新文件或事件；
- 临近的已授权日程；
- 重复任务；
- 长期目标停滞；
- 已失败 Skill；
- 用户明确委托；
- AI 自身维护需求。

### Candidate Generator

可以使用规则或模型，产生：

- 发现；
- 建议；
- 只读准备任务；
- 可执行任务候选；
- Skill/Delegation 候选。

### Policy/Scheduler

决定：

- 自动忽略；
- 延后；
- 只读准备；
- 向用户建议；
- 根据 Delegation 自动创建 Task。

执行仍由 Task Engine 完成。

## 10. 主动性等级

等级不是全局万能开关，可以按 Domain 配置。

### L0 Reactive

只响应用户，不主动探索。

### L1 Suggestive

可以基于已有信息提出建议，不主动读取新范围。

### L2 Read-only Preparation

可以在授权范围中只读收集、索引和生成草稿。

### L3 Reversible Local Action

可在委托范围内执行可逆的用户空间动作。

### L4 Delegated External Action

可在明确委托下发送、提交或对外变化。

### L5 Privileged Delegation

可触发预先定义的特权动作，但每个动作仍受 Broker 与认证策略控制。

### Core self-modification boundary

Core self-modification boundary 是独立硬边界，不是 L6，也不属于主动性等级。无论 Initiative Level、Delegation、Policy Rule 或模型建议为何，Agent、Memory、Skill、Extension 和 Provider 都不得：

- 给自己扩展 Kernel 或 Broker 权限；
- 修改 Core Identity；
- 修改 agentd Kernel、Task/Policy 解释器或私有 Kernel API；
- 关闭、伪造或绕过 Audit 完整性机制；
- 修改 Privilege Broker、Emergency Stop 或安全/隔离边界；
- 获取通用 root Shell 来规避固定特权 Action；
- 隐藏行动、来源或版本历史。

结构化 Policy Rule、Governed Configuration、模型路由、任务策略和恢复知识属于可生长对象，可以在可信解释器外持续版本化迭代。外部发送、重大承诺或不可逆动作不是 Core 自改，但仍必须受 task_scope、Delegation、Policy、预算、验证和审计约束。

## 11. Delegation Contract

委托不是 Prompt，而是结构化授权对象。

### Identity

- id；
- title；
- created by；
- source statement；
- status。

### Domain

例如：系统更新、项目周报、指定邮件夹、文档备份。

### Scope

明确资源和排除范围。

### Allowed Actions

动作白名单，不能只写“完成任务所需的一切”。

### Side-effect Ceiling

S0-S5 上限。

### Trigger

- 手动；
- 定时；
- 事件；
- 条件；
- 机会发现后。

### Approval Policy

以下是可选策略值，不是系统默认：

- 每次确认；
- 计划变更时确认；
- 仅超出预算时确认；
- 委托内自动；
- 要求本机确认。

是否采用其中任何值由结构化 Delegation/Policy 明确决定；没有匹配规则时遵循 Default Allow。

### Budget

- 模型费用；
- 执行时长；
- 文件/消息数量；
- 网络流量；
- 频率；
- 特权动作次数。

### Notification

开始、关键节点、完成、失败和静默规则。

### Verification

如何判断任务真正完成。

### Rollback

可逆动作的备份和恢复策略。

### Expiration

时间、次数、版本或条件过期。

### Revocation

用户可立即撤销；撤销后未开始动作不得继续，运行中动作按取消语义处理。

## 12. 从自然语言形成委托

首次遇到“以后你自己处理”或 Bot 主动发现可重复任务时：

1. 解析自然语言目标和来源；
2. 生成带版本的委托草案；
3. 明确范围、动作、风险、频率、预算、验证和通知；
4. 记录 actor、entry point、source statement 和解析证据；
5. 将结构化变更交给 Policy；
6. 无匹配规则时按 `allow` 保存并生效；命中 `confirm` 时展示给用户编辑或确认；命中 `deny` 时拒绝并记录原因；
7. 必要时生成或更新 Skill 和 Trigger；
8. 后续匹配时引用委托的明确版本。

用户与 Bot 都可以用自然语言管理 Delegation。第一版没有 Owner 唯一身份认证，不能把“由 Bot 创建”自动等同于低可信，也不能把所有委托创建、修改或启用默认为人工审批。

自然语言原句仍不能被当作永久无限授权。必须解析为有 Scope、Allowed Actions、Budget、Expiration、Verification 和 Revocation 的结构化对象；事实不足时在源头暴露并请求澄清。

## 13. Skill、Trigger 与 Delegation

- Skill：怎样完成；
- Trigger：何时尝试；
- Delegation：是否允许自动完成；
- Task：本次具体执行；
- Memory：过去发生了什么。

这些对象必须分开。

## 14. AI 自写与持续维护 Extension

AI 可以因为能力缺口、已知缺陷、Provider 变化或恢复需要发起并完成 Extension 的版本化生命周期。

允许：

- 读取公开 SDK、扩展示例和已安装 Extension 的可维护源码；
- 在 AI 自有 Extension Workspace 中创建或修改代码；
- 运行受限测试、兼容性检查和回归测试；
- 生成或更新权限 Manifest；
- 生成源码版本、构建产物和变更说明；
- 安装、启用、更新、停用和回滚 Extension；
- 创建、测试、更新和回滚 Provider Adapter；
- 在观测失败后恢复到已知稳定版本。

Extension 演进必须记录源码 revision、构建输入、Manifest 变化、测试结果、安装版本、稳定版本、预算、最大迭代次数、停止条件和运行观测。安装或更新后必须有健康检查；失败时停止继续迭代，并在可安全回滚时恢复稳定版本。

上述行为走正常 Task、Policy、Scope、Extension 生命周期和审计。无匹配规则时为 `allow`，命中规则时才 `confirm` 或 `deny`；“由 AI 编写”或“涉及安装”本身不构成默认审批条件。

禁止：

- 把可信 Core 私有源码作为自动修改目标；
- 写入或热替换 agentd Kernel、Task/Policy 解释器、Audit、Broker、Emergency Stop、Core Identity 或安全边界；
- 自动签为官方开发者发布；
- 绕过 Policy、Extension Manifest、依赖锁定、测试、观测或审计；
- 从原有低权限 Task 推导超出 task_scope 或 Delegation 的运行权限；
- 用回滚名义恢复一个没有验证记录的未知版本。

## 15. 情绪价值边界

Companion 可以：

- 表达关心；
- 记住共同经历；
- 在用户挫折时提供支持；
- 有稳定幽默和表达风格；
- 诚实表达自己的判断。

Companion 不得：

- 声称拥有真实身体或人类经历；
- 利用依赖感索取权限或数据；
- 因用户拒绝权限而施压；
- 将情绪支持自动转化为任务；
- 假装确定理解用户所有感受。

## 16. 管理界面

用户必须能管理：

- Persona Seed；
- Learned Style；
- Self Preferences；
- User Memories；
- Relationship Memories；
- Exploration Scopes；
- Task Scopes；
- Initiative Levels；
- Delegations；
- Memory/Skill/Extension 的当前版本、历史版本和运行状态；
- Pending Memory/Skill/Extension Candidates；
- Provider `index_pending`、失败与重试状态；
- 完整重置或导出。

Memory 管理必须提供查看、修改、单条删除、批量删除、按作用域删除、导出和重置。界面要显示来源、冲突、纠正关系、作用域、注入政策、当前 revision 与 Provider 索引状态，并在删除前后明确展示级联范围。对已经发往云端、无法撤回的数据必须明确提示，不得用成功的本地删除结果掩盖外部不可撤回事实。
