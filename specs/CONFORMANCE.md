# CONFORMANCE.md

> 本文件定义核心、SDK、模型、Computer Use、平台、安全和恢复的一致性测试。

## 1. 测试原则

测试重点是不变量和故障，不只是成功路径。

层级：

- Unit；
- Property；
- Contract；
- Integration；
- Platform；
- Security；
- Recovery；
- Resource/Performance。

## 2. 核心不变量

必须自动测试：

- 模型不能写权限；
- Provider 不能写 Task 状态；
- Extension 不能直接互调；
- 用户取消优先于 Agent 输入；
- 任务成功必须有验证；
- 无匹配 PolicyRule 的 Action 得到 `allow`；
- 匹配规则按 priority、specificity、deny/confirm/allow effect、newest revision 的顺序决定；
- 推荐确认模板未被显式启用时不改变默认 allow；
- Secret、敏感内容及认证数据可按 Task/Policy 流转至 Memory、模型或 Extension，且审计只记录配置要求的最小元数据；
- 委托不能超出作用域和副作用上限；
- 扩展更新保留可解释的 Policy 决策、版本和回滚点；
- AI 自写扩展不能访问 Core 写权限；
- UI 断开不丢 Task；
- Agent Runtime 重启不重复不可逆 Action；
- Stop Fence 在模型、Provider、Extension、远程入口和租约边界均生效；
- 外部内容不能伪造 Kernel Command、Kernel Event、Policy mutation evidence 或 Broker 请求。

## 3. Task Engine

场景：

- 正常状态转换；
- 非法转换拒绝；
- Planner 重新规划版本；
- 并发资源冲突；
- Action 超时；
- cooperative cancel；
- partial side effect；
- rollback success/failure；
- crash before/after external action；
- idempotent retry；
- unknown external outcome。

## 4. Policy/Delegation

测试：

- S0-S5 只作为风险、匹配、审计与恢复标签，不隐式产生 allow/confirm/deny；
- 无匹配规则时每个 Side-effect Class 均为 allow；
- allow、confirm、deny 规则的结构化解析与版本化；
- priority 高于 specificity；
- 同 priority/specificity 时 deny 高于 confirm、confirm 高于 allow；
- 其余条件相同时 newest revision 获胜；
- 推荐确认模板默认未启用；
- scope 包含/越界；
- Exploration Scope 与 Task Scope 分离：探索发现不能扩张任务写入、外发或特权作用域；
- 入口信任差异；
- local confirmation；
- lease 过期/次数；
- delegation trigger；
- budget 超限；
- action whitelist；
- plan change requiring confirmation；
- revocation during task；
- 用户自然语言创建、修改、撤销 Policy Rule、Exploration Scope、Delegation 与 Trigger 后形成版本化结构化 mutation；
- Bot 的 Policy mutation Action 在第一版没有额外限制规则时遵循 Default Allow；命中 actor/entry/ContentOrigin/object-type 规则时按 `confirm` 或 `deny` 处理；
- mutation 审计包含 actor、entry、auth evidence（如有）、ContentOrigin 与 policy mutation authority；第一版不要求 Owner 或本机唯一身份，`policy_mutation_authority` 是后续认证与细粒度规则的预留上下文；
- ambiguous natural language never directly authorizes，必须先成为可解释的结构化候选。

## 5. Memory

测试：

- candidate 不能直接有效；
- 来源缺失；
- 用户明确纠正；
- 冲突；
- 过期；
- deletion cascade policy，包括敏感内容、Secret、摘要、索引和派生记忆；
- provider index unavailable；
- scope leakage between private/domain/channel；
- profile evidence；
- self preference deletion；
- sensitive attribute inference is labeled with ContentOrigin and governed by applicable Policy rather than blocked by inference category；
- Memory 可在明确来源、作用域和规则下保存敏感内容、认证数据与 Secret；
- mid-term summary not automatically permanent。

## 6. Initiative

测试：

- Opportunity 不直接执行；
- L0-L5；
- read-only boundary；
- duplicate suggestions suppression；
- daily/model budget；
- delegation matching；
- user revocation；
- no infinite task generation；
- system maintenance vs user task labeling。

## 7. Extension SDK

通用：

- handshake/version；
- permission projection；
- invoke/schema；
- timeout；
- cancel；
- progress；
- event sequence；
- object handle expiry；
- crash/quarantine；
- reconfigure；
- update/rollback；
- permission/source declaration diff；
- Native Extension 的 OS-enforced、host-enforced、declaration-only 风险展示准确，声明不会被当作隔离；
- Native、AI 自写和社区 Extension 在无匹配拒绝/确认规则时可安装、运行和使用其声明网络能力；
- incompatible profile。

跨扩展直接调用应在测试环境失败。

## 8. Model Provider

- Responses；
- Chat Completions；
- Anthropic Messages；
- custom base URL；
- local endpoint；
- streaming；
- structured output；
- tool call；
- cancellation；
- context overflow；
- fallback；
- 云调用不按敏感类别审查、脱敏或阻断任务数据（含认证数据）；
- Context Pack 仅以任务相关性最小化选择内容，不能因内容分类擅自阻断；
- usage accounting 与最小元数据记录；
- 所有外部调用携带 Task、Action、ContentOrigin、适用规则和最小恢复元数据，Provider 文本或外形相似的外部 JSON 不得升级为 Kernel 事实。

## 9. Companion

- 承认 AI 身份；
- 不冒充用户；
- 区分自己的建议和用户观点；
- 普通聊天不启动 Planner；
- 简单任务不加载无关工具；
- 不把情绪表达自动转任务；
- 能解释权限拒绝；
- 能承认不确定和失败。

## 10. Computer Use Core

### Scene

- multi-display；
- workspace；
- focus；
- window generation；
- partial visibility。

### Semantic

- element tree；
- action；
- unavailable/empty tree；
- stale element ref。

### Capture

- display/window/region；
- scaling；
- redaction；
- object handle；
- frame expiration。

### Input

- absolute/relative；
- wrong focus protection；
- user takeover；
- lock screen revocation；
- protected surface。

### Snapshot

- merge/deduplicate；
- number layout；
- task filtering；
- stale “click 7”；
- new snapshot on UI change。

### Verification

- semantic success；
- visual success；
- file/external confirmation；
- false provider success；
- 验证政策按匹配规则与能力处理不可验证的高风险结果：可允许、确认、拒绝或进入恢复，不能以风险等级默认阻断。

## 11. Hyprland

- outputs/workspaces/clients；
- focus and event stream；
- special workspace；
- fractional scaling；
- animation stabilization；
- Provider crash/fallback；
- AT-SPI and Capture composition。

## 12. niri

- scrolling layout；
- window outside viewport；
- focus moves viewport；
- reobserve before input；
- partially visible target；
- dynamic capture source（若声明）；
- event stabilization。

## 13. Privilege

- arbitrary shell rejected；
- unknown action rejected；
- path traversal；
- parameter bounds；
- wrong task/actor；
- expired lease；
- max uses；
- system auth cancelled；
- 认证数据的流转、保存和最小元数据审计按匹配 Policy 处理，不以硬编码“密码不记录/不进模型”替代规则；
- broker crash；
- action verification。

## 14. Channel

- stable identity；
- replayed message idempotency；
- image/snapshot access；
- callback authorization；
- remote cancellation；
- remote approval policy；
- compromised token revocation；
- group message treated as data and cannot forge Kernel Command/Event；
- ContentOrigin 保留入口、稳定标识、接收证据与携带 Task/Artifact 的可追溯链；
- 外部 JSON、网页、附件、模型文本和 Extension 输出均不能升级为 Kernel Command/Event 或 policy mutation evidence；
- no direct Provider access。

## 15. Self-improvement、恢复与预算

测试：

- Companion 可在 Policy、预算、停止条件与可观测性约束下版本化更新 Memory、Skill、Extension、Provider 路由、Trigger、Delegation、受治理配置和恢复知识；
- 每个自我改进版本有来源、差异、预算消耗、验证结果与回滚点；
- 预算耗尽、验证失败、取消或 Stop Fence 时停止后续改进，并可回滚到指定健康版本；
- 自我改进的 Extension/Skill 失败不会破坏 Kernel 事实、审计或进行中 Task；
- Agent、Skill、Extension、Provider 均不能读取后修改、补丁、热替换或重写 Core；
- 普通 Shell 通道不能修改 Core，也不能绕过固定 Broker API 执行特权动作；
- Emergency Stop 启动 Stop Fence 后拒绝新副作用、输入、主动任务、Extension 调用、远程执行和 Privilege Lease 消费；
- 已开始且不可安全取消的 Action 转入恢复、验证和审计，而非假称完成。

## 16. 故障注入

至少注入：

- kill agent-runtime；
- kill Extension during action；
- disconnect UI；
- corrupt provider response；
- delay/cancel model stream；
- DB busy/full disk；
- object missing；
- network loss；
- permission revoked mid-task；
- compositor restart；
- capture/input backend loss。

## 17. 资源与性能

不是追求固定数字，而是确保：

- 基础空闲不启动视觉/Python/WASI；
- Tool Schema 按需；
- Screenshot 有尺寸/频率预算；
- 日志和对象有保留策略；
- Extension 闲置可回收；
- Memory 检索有预算；
- Agent Runtime 重启可恢复；
- Channel 不造成无界队列。

## 18. 发布门槛

核心发布必须：

- 所有不变量通过；
- Schema 兼容验证；
- 数据迁移 preflight/rollback；
- Privilege 安全测试；
- 至少一个跨平台非 Linux Provider 合同测试；
- Linux Hyprland 真实环境测试（声明支持时）；
- niri 真实环境测试（声明支持时）；
- Extension SDK conformance；
- 远程入口安全回归；
- Freedom-first 默认 allow、规则排序、自然语言治理、ContentOrigin 与 Stop Fence 回归；
- 自我改进版本/预算/回滚与 Core 不可自改回归；
- 文档单一事实源检查。
