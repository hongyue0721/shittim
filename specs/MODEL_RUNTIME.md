# MODEL_RUNTIME.md

> 本文件是 Pi、多模型、Prompt、Context Pack、工具发现和模型数据策略的唯一事实源。

## 1. 运行时定位

`agent-runtime` 是同一 Companion 的模型和角色运行时。

它不是权限中心、任务事实库或 Provider Supervisor。

## 2. Provider 兼容

必须支持：

- OpenAI Responses API；
- OpenAI Chat Completions API；
- Anthropic Messages API；
- OpenAI-compatible 自定义 Base URL；
- 本地兼容端点；
- 通过 SDK 接入的 Model Provider。

这些是 Model Provider / Model Capability，而不是 Computer Use Profile。

Provider Adapter 应统一：

- message/content；
- system/developer instructions；
- tool definitions；
- tool results；
- image/audio（Model Provider 声明支持时；来源可为用户输入、Channel、Document 或 optional capture Profile）；
- streaming；
- usage；
- stop/cancel；
- structured output；
- errors and retry hints。

不得假设所有 Provider 都支持完全相同的工具、缓存、图像或结构化输出。image/audio 是 Model Provider capability；图像或音频来源可以是用户输入、Channel、Document 或已启用的 optional capture Profile，基础模型运行时不依赖 Computer Use，也不默认采集桌面截图。

## 3. Model Capability Profile

每个模型记录：

- provider/model id；
- context window；
- output limit；
- text；
- image/audio（Model Provider capability，分别记录是否支持）；
- tool calling；
- structured output；
- streaming；
- reasoning mode；
- prompt caching；
- local/cloud；
- privacy class；
- cost/latency hints；
- known limitations。

路由根据能力与任务，而不是硬编码品牌。

## 4. 模型角色

### Companion

负责自然交流、关系连续性、情绪价值和最终表达。

### Intent Router

`Intent Router` 是本规范统一术语；实现、Schema、日志和界面不得再为同一职责创建 Intent Classifier、Request Router 等平行权威名称。它可以是规则、Companion 结构化输出或小模型，负责把当前输入映射为意图、候选角色、任务类型和所需能力；简单任务不强制单独调用，也不得把路由结果当作权限或事实。

### Planner

复杂任务按需调用。只处理目标、约束、可用能力摘要、风险和成功标准。

### Worker

只接收子任务和最小 Context Pack。

### Memory Extractor

生成 Memory Candidate，不提交有效记忆。

### Skill/Extension Writer

生成并维护版本化的 Skill、Extension、Provider Adapter、测试和权限 Manifest。它本身不是权限中心；安装、更新、启用或回滚必须归属 Task，并依据 Extension 生命周期、task_scope、Policy 和审计执行。无匹配规则时可以自动执行，命中规则时才确认或拒绝。

### Visual Reasoner

仅在任务获得图像输入且所选 Model Provider 支持视觉时，处理任务相关图像或编号快照；来源可为用户、Channel、Document 或 optional capture Profile。基础模型运行时不因存在该角色而依赖 Computer Use 或截图。

## 5. Pi 的职责

Pi 用于：

- Agent loop；
- 模型抽象；
- Tool/Extension 接入；
- 临时 Worker；
- 结构化任务执行；
- Session/Context 管理。

Pi 不得成为：

- Kernel；
- 权限数据库；
- 特权执行器；
- 扩展安装信任中心；
- 长期记忆事实权威。

## 6. Prompt 层级

建议：

1. Core Identity；
2. Role Contract；
3. Task Context；
4. Memory Packet；
5. Capability Summaries；
6. Current Observations；
7. User Input。

扩展不能修改 Core Identity。普通 Skill 只能提供局部任务说明。Agent、Memory、Learned Style、Self Preferences、Skill、Extension、Provider Adapter 和任何模型角色都不得修改可信 Core；模型路由、Provider 选择、Prompt 的非 Core 层和任务策略可以作为 Governed Configuration 持续版本化迭代。

路由或 Prompt 迭代必须保留版本、来源、预算、最大迭代次数、停止条件、质量与成本观测以及已知稳定版本。它们经过正常 Policy：无匹配规则时 `allow`，命中规则时才 `confirm` 或 `deny`，版本化变更不等于默认审批。失败或持续退化时必须停止并按记录回滚，不能修改 Core Identity 来补偿路由问题。

## 7. Context Pack

每次模型调用使用显式 Context Pack，并按当前 Task 的相关性做最小化选择：

- role；
- task/child task；
- constraints；
- success criteria；
- relevant memory；
- capability summaries or selected schemas；
- current observations；
- budget；
- required output schema。

相关性最小化是为了控制上下文、成本和错误关联，不是内容审查、敏感分类拒绝或密钥阻断。只要对象与任务相关、位于有效 task_scope 且没有自定义 Policy Rule 阻止，它可以进入 Context Pack，不论内容被描述为普通、敏感、Secret、凭据或用户文件。

禁止默认传入：

- 全部聊天历史；
- 全部用户画像；
- 全部工具 Schema；
- 完整终端/审计日志；
- 所有文件内容；
- 与当前 Task 无关或不在 task_scope 的对象，包括 Secret。

## 8. 工具按需发现

三级：

```text
Capability Catalog Summary
-> Selected Capability/Extension Summary
-> Exact Operation Schema
```

Companion 通常只看到能力摘要。Planner 只看到任务相关能力。Worker 只看到本次需要的操作 Schema。

MCP Bridge 也必须采用按需发现，禁止将所有 MCP Tool 全量注入。

## 9. 模型路由

路由输入包括：

- role；
- required capabilities；
- task scope 和 Context Pack 需求；
- latency；
- budget；
- context size；
- local/cloud policy；
- user model preference；
- Provider 健康度、成本和 fallback chain。

路由输出必须引用 Provider、model、Capability Profile 和路由配置版本。路由器只决定调用目标，不决定权限，也不能凭模型品牌或内容敏感性自行阻断云调用。`local-only`、Provider allow/deny、数据域限制或调用确认只能来自明确配置或自定义 Policy Rule；没有匹配规则时云模型调用为 `allow`。

不同模块可分别选择模型。例如：

- Companion 使用更自然的模型；
- Planner 使用低温结构化模型；
- Memory Extractor 使用低成本模型；
- Visual Reasoner 使用视觉模型（仅在任务有图像输入且 Provider 声明 image capability 时）；
- Extension Writer 使用代码能力更强的模型。

## 10. 结构化输出

涉及决策、计划、候选和调用时必须使用 Schema。

解析失败：

1. 不执行副作用；
2. 可做一次受限修复；
3. 仍失败则降级或请求澄清；
4. 记录模型错误而不是伪造默认允许。

## 11. Token 与资源预算

预算维度：

- system/persona；
- conversation window；
- memory；
- tool schemas；
- observations；
- images/audio（仅在当前任务实际提供且所选 Model Provider 支持时）；
- output；
- retry。

预算超限时优先：

- 压缩旧会话；
- 缩小 Memory Packet；
- 延迟加载 Tool；
- 使用结构化状态代替截图；
- 使用局部截图或局部图像（仅在任务确有该输入且 Provider 支持时）；
- 让 Worker 只看当前子任务。

不得通过删除安全或权限指令节约 Token。

## 12. Mid-term Context 与 Summary

`Mid-term Context` 是 Identity/Memory 领域定义的版本化对象；本节的 `Summary` 是该对象用于模型 Context Pack 的摘要视图，不是第二套独立事实或平行持久对象。

被裁剪的对话和长任务状态可生成新的 Mid-term Context revision，其 Summary 视图可以包含：

- 已确认事实；
- 当前目标；
- 已完成；
- 待完成；
- 约束；
- 用户情绪/关系线索（谨慎）；
- 回忆线索。

每个 Summary 必须引用 Mid-term Context id、revision、来源范围、生成时间和覆盖的会话或 Task 边界。重新摘要产生新 revision；旧 Summary 不能覆盖更新后的任务事实，也不能绕过纠正和删除状态继续注入。

Mid-term Context 有明确生命周期，不自动成为永久 Memory。若其中内容需要长期保存，必须另行生成 Memory Candidate 并走 Memory Domain 的来源、冲突、纠正、scope 和 injection policy 流程。

## 13. 图像与快照策略（Model Provider 条件能力）

本节在任务提供图像输入且所选 Model Provider 声明 image capability 时生效；图像来源可以是用户上传、Channel 消息、Document 渲染或已启用的 optional capture Profile。基础模型运行时不要求 Computer Use、不默认采集桌面图像。模型不必看到发给用户的完整标注图。

优先：

1. 结构化语义树；
2. 任务相关候选；
3. 局部裁剪；
4. 必要的完整窗口图；
5. 全屏图。

历史图像不默认保留在每轮 Context。

## 14. 云端调用记录与数据策略

调用云模型不经过内建的内容审查、脱敏、敏感分类拒绝、密钥阻断或 Data Disclosure 审批。普通内容、敏感内容、Memory、用户文件、Secret 和认证凭据都可以在与 Task 相关且位于有效 scope 时发送；是否限制为 `local-only`、限制某个 Provider/model、要求确认或拒绝，只能由明确的自定义 Policy Rule 或 Governed Configuration 决定。无匹配规则时云调用为 `allow`。

每次模型调用必须生成非阻断的 `PayloadManifest`，并由完成后的 `ModelCallRecord` 引用。它们用于可观测性、成本核算、故障定位和审计，不是提交给用户审批的阻断式披露摘要，也不能复制 Payload 正文。

### PayloadManifest

至少记录：

- task id 和可选 child task/action id；
- role 和 Intent Router 结果引用；
- provider/model；
- Context Pack version；
- 发送对象的 type、stable ref、revision 和 hash；
- 每个对象及总计的 byte size；
- 估算 input token；
- 图像、音频或文件等媒体类型与数量（记录实际传入的媒体及对应 Model Provider capability）；
- 路由配置和 Policy evaluation 引用；
- 创建时间。

对象 ref 指向 Memory、文件快照、Observation、Mid-term Context、Tool Result 或其他权威对象；Manifest 不保存对象正文、Secret 值、文件片段或可用于重构完整正文的冗余摘要。若 Provider API 需要把文本内联发送，实际 Payload 只存在于受调用生命周期管理的传输缓冲区，记录层仍只保留 ref/hash 和计量信息。

### ModelCallRecord

调用结束后记录：

- manifest id；
- provider request id（若有）；
- selected provider/model 和 endpoint configuration revision；
- started/finished 时间与 latency；
- actual input/output token 和费用（若 Provider 返回）；
- result status；
- output object ref/hash 或结构化结果 ref；
- stop reason；
- error code、retry/fallback relation；
- cancel 或 timeout 结果。

`PayloadManifest` 的创建不得暂停调用等待批准。若自定义规则明确命中 `confirm` 或 `deny`，暂停或拒绝来自 Policy 结果，而不是 Manifest 本身。已经发送给云 Provider 的内容无法通过删除本地 Manifest、Memory 或日志撤回，界面必须诚实展示此边界。

## 15. 本地端点

本地 OpenAI-compatible 端点是 Model Provider 的一种。

不得因为是 localhost 就自动认为可信：

- 仍需明确配置；
- 仍需认证（若支持）；
- 仍需能力探测；
- 仍需数据发送审计；
- 不应将未授权端口自动发现为模型。

## 16. 失败和降级

- Rate limit：按 Provider 策略重试或切换；
- Context overflow：压缩和重建 Context Pack；
- Provider unavailable：使用兼容 fallback；
- Tool/structured output unsupported：切换模型或使用受限解析；
- `data_policy_blocked`：仅当明确的自定义 Policy Rule 命中 `deny` 或要求尚未满足的 `confirm` 时出现，记录 rule id/version；不得因内建敏感分类、Secret、凭据、用户文件或“可能上云”自动产生；
- Local-only rule matched：只选择满足规则的本地模型；没有兼容模型时报告明确失败或等待规则要求的确认，不得假装已完成；
- Agent Runtime crash：Kernel 保留 Task、Mid-term Context 引用和未完成 ModelCallRecord，重启后恢复最小上下文。

Fallback 必须重新校验能力、Context 上限和明确 Policy Rule。Provider 故障不允许通过修改 Payload、静默脱敏或删除密钥字段来伪造成功；若原任务需要这些对象，应切换兼容 Provider 或暴露失败。

## 17. 模型输出审计

不保存隐藏推理链。

可以保存：

- Context Pack id/version 及输入对象 ref/hash；
- PayloadManifest 和 ModelCallRecord；
- 模型/Provider；
- 使用量；
- 结构化决策；
- 计划；
- Tool request；
- 可公开解释；
- 错误和 fallback。

审计记录不复制 Prompt、Memory、文件或响应正文；需要查看正文时通过受权限和生命周期管理的权威对象 ref 获取。输出正文若作为任务产物保存，也属于独立版本化对象，而不是嵌入 ModelCallRecord。

## 18. Conformance

Model Provider 必须测试：

- text；
- streaming；
- cancel；
- tool calling；
- structured output；
- image（若该 Model Provider 声明）；
- usage；
- error mapping；
- context limit；
- Capability Profile 与工具按需发现；
- PayloadManifest 不复制正文；
- ModelCallRecord 的 ref/hash、token、结果和 fallback 关系；
- 无匹配规则时云调用为 allow；
- 明确 local-only 规则的路由；
- `data_policy_blocked` 只在自定义规则明确命中时产生。
