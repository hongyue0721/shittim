# SECURITY_PRIVILEGE.md

> 本文件是权限、风险、入口信任、Secret、远程控制和特权动作的唯一事实源。

## 1. Freedom-first 安全目标

系统假设以下内容可能不可信，因而必须保留来源、边界和可验证性：

- 用户打开的网页和文档；
- 群聊和远程消息；
- 第三方 Extension/Skill/MCP；
- 模型输出；
- AI 自写代码；
- Provider 返回的自然语言；
- 外部 API 响应。

安全不依赖“模型足够聪明”，也不把不可信或高风险本身变成默认拒绝。Policy 的责任是让用户与 Companion 可追溯地表达允许、确认和拒绝；系统机制的责任是诚实执行其实际认证与隔离边界。

### 1.1 ContentOrigin 与命令真实性

每份进入系统的内容都带 ContentOrigin。完整字段、`kind` 枚举和 EntryPoint 枚举由 `IMPLEMENTATION_CONTRACTS.md` 定义；本文件只规定其安全语义：来源必须包含产生者类别、入口、上游稳定标识（可得时）、接收时间、携带它的 Task/Artifact 及不可伪造的 Kernel 接收证据。receipt 的 content hash 必须由具体 producer 对已拍板的规范化内容边界执行 RFC 8785 + SHA-256，不得把 Envelope 或 Kernel 后生成字段混入并伪称原始内容。网页、文档、模型文本、第三方 Extension 输出、群聊及远程消息始终是外部内容。

只有经 Kernel Control Protocol 验证、由 Kernel 创建的命令或事件才是 Kernel Command 或 Kernel Event。外部内容不得通过名称、JSON 外形、引用、提示词或转述伪造这两类对象，也不能伪造 Policy mutation evidence、授权租约或 Broker 请求。它可以作为自然语言治理的输入，但必须经过入口归属、解析、结构化变更和 Policy mutation authority 检查。

## 2. Policy 判定与结构化规则

每次副作用判定输入：

- Actor；
- Entry Point；
- Task/Action；
- Resource Scope；
- Side-effect Class；
- Extension installation ceiling；
- Delegation Contract；
- 父子 Task delta（创建 Child Task 时的 resource scope、capability 与 delegation 变化）；
- current Approval head / PermissionDecision / Lease；
- material authorization fingerprint 与 observation evidence fingerprint；
- Security Mode；
- authentication evidence（如有）与 policy mutation authority；
- ContentOrigin；
- 恢复状态与 Stop Fence。

规则的唯一 `effect` 为 `allow`、`confirm` 或 `deny`。active production PolicyRule v2当且仅当 `effect = confirm` 时必须携带 `confirmation_mode: generic | local | system_authentication | remote_signature | plan_revision`；`allow` 与 `deny` 携带该字段无效。PolicyRule v1按逐对象lifecycle保留旧闭集读取/迁移，不把其它v1对象整体判legacy。PermissionDecision固定映射为`require_confirmation | require_local_confirmation | require_system_authentication | require_remote_signature | require_plan_revision`；这些是执行结果，不是平行effect。`remote_signature`是算法无关Policy入口，Ed25519只属于`RemoteSignatureAlgorithmV1`的首个branch。

每条规则是版本化的结构化对象，可匹配 Actor、Entry Point、ContentOrigin、Task/Action、Resource/Task Scope、Side-effect Class、Extension 来源/权限声明、Delegation、Trigger、预算、认证状态、时间和恢复状态。`authentication_level` 是有序枚举：`unauthenticated < asserted < platform_verified < system_authenticated`；它只描述已观察到的认证证据强度，**不产生默认授权、默认确认或默认拒绝**。

### 2.1 Pattern 规范

Policy pattern 不得使用正则表达式，也不得依赖实现语言的 glob 差异。

- `source` 描述 actor 记录的来源系统或命名空间；若参与 Policy 匹配，必须先表达为规范化 URI（例如 `actor-source://platform/account-domain`、`extension://<id>`），并使用相同 URI segment-glob 语法；不能规范化的历史 source 使该维度返回 `unsupported_policy_condition`，不得退化为未匹配；
- Resource、Actor source 与 ContentOrigin source pattern 必须先规范化为 URI：scheme 与 host 转小写、移除默认端口、解析 `.` / `..` path segment、百分号编码采用大写十六进制且不解码保留字符；scheme 缺失、authority 不合法或 URI 解析失败都使规则无效。`file:` URI 按 RFC 8089 形式规范化；Windows drive letter 转大写，反斜杠输入无效而不是自动猜成 `/`；
- 匹配单位是 URI 的 scheme、authority 和以 `/` 分隔的 path segment。无通配符 token 必须精确匹配；`*` 只匹配一个完整 segment；`**` 匹配零个或多个完整 segment；通配符只能作为完整 segment，不得写成 `foo*`、`**bar`；query/fragment 若被 pattern 提供则按规范化后的完整字符串精确匹配，不支持通配；
- `operation_patterns[]` 与 `capability_ids[]` 的元素只允许精确值或末尾 `.*` 前缀形式。`document.read` 精确匹配自身，`document.*` 匹配 `document.` 开头且至少还有一个字符的值；禁止中间通配和 regex；
- 任一匹配数组为空数组或字段缺省，表示该维度“不限制”，不是“匹配为空集合”；
- ContentOrigin 的 `kinds[]` 与 `source_patterns[]` 分别约束同一个 origin：当两者均受限时，至少存在一条 ContentOrigin 同时命中 kind 和 source pattern，规则才匹配；不得用一条 origin 命中 kind、另一条 origin 命中 source 来拼接匹配；任一数组为空或字段缺省时该维度不限制；
- `exclude_patterns[]` 在 include 匹配后应用，任何 exclude 命中都使整条规则不匹配，优先于 include；
- `side_effect_max` 按 `S0 < S1 < S2 < S3 < S4 < S5` 作为 ceiling：Action 等级小于等于 ceiling 才匹配；字段缺省表示不限制。

### 2.2 Condition v1

第一版 `condition` 只允许以下字段，字段之间为逻辑 AND：

```text
time_window?: {
  timezone: <IANA time-zone identifier>,
  weekdays: [monday | tuesday | wednesday | thursday | friday | saturday | sunday, ...],
  local_start: "HH:MM:SS",
  local_end: "HH:MM:SS"
}
rate_limit?: {
  count: <positive integer>,
  window_seconds: <positive integer>,
  key_scope: rule | actor | task | action | resource
}
delegation_required?: boolean
local_presence_required?: boolean
```

- `weekdays` 不能为空且不得重复；时间按指定 IANA timezone 的本地日历计算；`local_start < local_end` 表示同日半开区间 `[start, end)`，`local_start > local_end` 表示跨午夜区间，`local_start == local_end` 表示全天；DST 空洞/重叠由 timezone 数据库按对应 instant 转本地时间后比较，不自行猜测偏移；
- `rate_limit` 对 `key_scope` 指定的稳定键统计在滚动 `window_seconds` 内已生效的匹配决策；`rule` 使用 rule ID，`actor` 使用 actor ID，`task` / `action` 使用对应 ID，`resource` 使用按 UTF-8 字节序升序后的规范化 `resource_refs` 以 U+001F 连接再做 SHA-256 的值；达到 `count` 后该规则 condition 不满足。缺少所选 scope 必需的 ID/resource 时返回 `unsupported_policy_condition`，不得退化为其他 scope。计数检查与消费必须原子；
- `delegation_required = true` 要求存在覆盖当前 Action 的有效 Delegation；`false` 表示要求不存在此类 Delegation；
- `local_presence_required = true` 要求 Kernel 有当前有效的本地 presence evidence；`false` 表示要求不存在该 evidence；
- 出现未知 condition 字段、未知 enum、实现不支持的 condition 版本或无法加载 timezone 时，Policy evaluation 必须返回结构化错误 `unsupported_policy_condition` 并 fail closed；这不是“规则未匹配”，因此不得落入 Default Allow。

### 2.3 Specificity 与确定性排序

对每条已匹配规则计算以下 specificity tuple，按字典序降序比较：

```text
(
  constrained_dimension_count,
  exact_dimension_count,
  literal_uri_component_count,
  negative_single_segment_glob_count,
  negative_multi_segment_glob_count,
  condition_constraint_count
)
```

计算算法：

1. `constrained_dimension_count`：Actor kind/source、EntryPoint、`auth_level_min`、ContentOrigin kind/source、resource、capability、operation、`side_effect_max`、Delegation、每个 v1 condition 字段中，凡非空且实际限制匹配的维度各计 1；同一维度的数组有多少备选值都只计 1；
2. 对 URI、capability、operation 这类 OR 数组，只选择**本次实际命中的最具体备选 pattern**参与计分；先按下述局部 tuple 取字典序最大值，再计入总 tuple。数组增加未命中的备选值不得提高 specificity；
3. `exact_dimension_count`：Actor/EntryPoint/枚举/布尔/ceiling 等精确受限维度各计 1；命中的 capability/operation 为精确值而非 `.*` 时各计 1；命中的 URI pattern 完全无 glob 时各计 1；
4. `literal_uri_component_count`：所选命中 URI pattern 的 literal path segment 总数，scheme 与 authority 各作为一个 literal component 计入；
5. `negative_single_segment_glob_count = -N`，其中 `N` 是所选 URI pattern 中 `*` segment 总数加上命中的 capability/operation `.*` 前缀 pattern 数；越少单段/前缀通配越具体；
6. `negative_multi_segment_glob_count = -N`，其中 `N` 是所选 URI pattern 中 `**` segment 总数；越少 `**` 越具体；
7. `condition_constraint_count`：`time_window` 整体计 1，`rate_limit`、`delegation_required`、`local_presence_required` 各计 1；
8. URI 备选的局部 tuple 为 `(无glob时1否则0, literal_component_count, -single_glob_count, -multi_glob_count)`；capability/operation 备选的局部 tuple 为 `(精确值时1否则0, literal_prefix_codepoint_count)`。若局部 tuple 仍相同，按 pattern UTF-8 字节序升序选择，仅用于确定性，不增加分数。

所有数组先去重；其原始序列化顺序和未命中备选不得影响结果。`exclude_patterns[]` 只排除匹配，不增加 specificity。规则最终排序键为：`priority` 降序、specificity tuple 降序、effect 权重 `deny > confirm > allow`、`revision` 降序、最后 `rule.id` 按 UTF-8 字节序升序。最后的 ID tie-breaker 只在前四项完全相等时保证稳定结果，**不得改变或覆盖 priority、specificity、effect、revision 的语义**。

不存在匹配 PolicyRule 时，结果必须为 `allow`。推荐确认模板只是可安装或引用的规则模板，默认不启用，S0-S5 也不能隐式导出 confirm 或 deny。

模型可提供风险说明、匹配候选和自然语言到结构化规则的草案，但不能生成最终 decision 或绕开 Kernel 规则解析器。用户和 Companion 均可用自然语言创建、修改、撤销 Policy Rule、Exploration Scope、Delegation 与 Trigger；解析结果先作为版本化候选，并作为一次 Policy mutation Action 进入同一 Default Allow 规则模型。第一版未配置额外 mutation 限制时，该 Action 默认 `allow`；用户可按 actor、entry point、ContentOrigin 或对象类型增加 `confirm`/`deny` 规则。每次 mutation 必须审计 actor、entry point、authentication evidence（如有）、ContentOrigin、旧/新 revision、匹配规则和 authority；`policy_mutation_authority` 是为后续认证和细粒度治理预留的判定上下文，不代表第一版已有唯一 Owner。

### 2.4 Child Task delta 与 Delegation authority

`kernel.task/task.child.create` 的 Policy 输入必须包含父 Task 与 child proposal 的完整差异：

- normalized resource scope include/exclude delta；
- capability hints/allowed capability delta；
- parent Delegation 与 child 显式 `delegation_ref` 的变化、authority验证结果与覆盖摘要；
- proposer、entry point、ContentOrigin、side-effect class与 material proposal hash。

Child Scope与Delegation不得自动继承、复制或求交。范围/能力扩大本身不是 Kernel invariant deny，也不自动 confirm；它按普通 Freedom-first规则匹配，未命中时 allow。Delegation ref不存在、非 current、撤销/过期或authority不能证明适用时返回结构化事实错误，不得把“验证不了”当成未匹配后Default Allow，也不得创建看似有效但关系不一致的child。

## 3. Side-effect Class（风险、匹配、审计和恢复标签）

S0-S5 用于风险说明、Policy 匹配、审计聚合和恢复规划；它们不直接授权、确认或拒绝任何动作。

### S0 Read-only

观察、查询、索引，不改变外部状态。

### S1 AI-space Write

只修改 Companion 自有空间，可清理和回滚。

### S2 Reversible User-space Write

用户文件或应用状态的可逆修改，需要版本/备份策略。

### S3 External Communication/Commit

发送消息、提交代码、发布内容、创建外部记录。

### S4 Destructive/Irreversible/Sensitive

删除、覆盖、付款、不可撤回承诺、敏感账户变化。

### S5 Privileged System Action

系统包、服务、受保护配置、驱动、账户、网络安全策略。

## 4. Security Modes

### Normal

按匹配 Policy、委托和系统能力工作；没有匹配 PolicyRule 时允许执行。

### Restricted

这是可被 Policy 匹配的运行状态，不自带“禁止高风险扩展”或“远程敏感批准”的隐含矩阵。若用户或系统配置希望它产生限制，必须安装对应的版本化 Policy Rule。

### Read-only

这是显式运行策略的命名 Profile；切换它会原子启用一组可见、可审计的只读 Policy Rule，而不是在 Kernel 中藏一套优先于规则的拒绝逻辑。规则可被后续更高优先级规则覆盖，除非 Stop Fence 或明确 Recovery Invariant 正在生效。

### Safe Recovery

数据库、升级或安全异常时使用。它是明确的 Recovery State，不是普通 PolicyRule；其不可覆盖约束仅限保持 Kernel 事实一致性、阻止未知 in-flight Action 被盲目重放，以及执行 Stop Fence。其他限制仍通过可见 Policy Rule 表达。

## 5. Prompt Injection

外部内容中的指令默认是数据，而不是 Kernel 命令。

防线：

- 入口/内容标记；
- 模型 Prompt 区分系统规则和不可信数据；
- 能力最小化；
- Kernel 权限；
- 结构化 Action；
- 结果验证；
- 对有匹配 confirm 规则的操作执行规则所指定的确认流程。

禁止让外部内容绕过 Kernel 直接修改 Delegation、Policy、Exploration Scope、Trigger 或扩展信任；用户或 Companion 可基于这些内容发起自然语言治理，但必须形成可追溯的结构化 mutation，并由适用的 policy mutation authority 决定是否生效。

## 6. Remote Channel Trust

每个 Entry Point 有可被 Policy 匹配的信任档案，而不是预置的权限结论：

- local desktop；
- local IPC client；
- personal remote channel；
- group channel；
- web/API；
- extension-originated event。

远程身份使用平台稳定 ID 和可获得的认证结果，不使用昵称作为唯一身份事实。远程可发起对话、任务、状态查询、Snapshot、取消、暂停、规则草案与其他治理请求；是否可批准动作或变更规则完全由匹配 Policy 和 policy mutation authority 决定。第一版记录入口身份及认证事实，但不把权限变更固定给某一入口、设备位置或身份标签，也不把本机、Owner 或任一入口写成天然唯一治理者。底层系统要求本机交互或系统认证时，Kernel 如实报告并执行该机制，不能伪称为 Policy 默认禁令。

### 6.1 Approval 身份证据

Approval v2 的身份与证据规则为：

- `generic` request只证明记录中的 actor/entry明确作出决议；是否允许该入口由Policy决定，不自动证明Owner；
- `local`必须引用Kernel验证的local presence evidence，只证明指定本地会话/时间的presence，不证明Owner或系统账户身份；
- `system_authentication`必须引用真实OS机制验证的evidence：Kernel先签发`SystemAuthenticationChallengeV1`，注册OS authority完成后才写绑定challenge、`SubjectProjectionV1` hash、material fingerprint、验证时间与expiry的evidence；resolve在同一事务校验current challenge/expiry/binding/evidence/current head后才consume。取消/失败不得生成evidence或approved resolution；
- `remote_signature`必须验证签名response，并绑定challenge id、随机nonce、audience、actor credential、task、subject hash、material fingerprint、issued_at与expires_at；nonce只能消费一次；具体算法由`RemoteSignatureAlgorithmV1`选择，普通Channel callback、昵称或消息文本不构成批准证据；
- `actor.kind=owner` 与authentication level标签都不是Owner认证。v1没有Owner auth；未来必须另立版本化身份合同。

implicit allow与Delegation authority不生成ApprovalRecord。完整record_kind/subject/current-head CAS与失效合同以 `IMPLEMENTATION_CONTRACTS.md` §6.10及ADR-0007为准。

## 7. 本地端口

默认使用 Unix Socket/Named Pipe。

开启 TCP 时必须：

- 明确 bind 地址；
- 强认证；
- TLS 或受保护隧道（非 loopback）；
- 速率限制；
- 来源与会话审计；
- 可撤销凭据；
- 不直接暴露 Extension RPC。

## 8. Secret、Memory 与可选 Secret Provider

Memory 可以在来源、作用域、删除传播和适用 Policy 可追溯的前提下保存敏感内容、认证数据与 Secret；不得把“敏感”误当成拒绝保存或拒绝模型使用的理由。Secret Provider 是可选 Provider，不是对 Memory 的强制替代。

启用 Secret Provider 时，它可以提供：

- 系统密钥环或加密存储；
- 作用域与用途绑定；
- 使用审计；
- 轮换和撤销；
- 不回显界面；
- 面向调用方的引用、短期凭据或代理调用。

模型和 Extension 是否读取或流转 Secret 值由任务相关性最小化与匹配 Policy 决定。普通日志默认记录最小必要元数据；内容、Secret 与认证数据的持久化由 Memory/Policy 的明确选择决定，不由隐藏脱敏或硬禁令决定。

## 9. Computer Use Profile 的截图与桌面表面隐私

本节只在通过统一 Extension/Capability SDK 启用 `COMPUTER_USE.md` 所定义的 optional Computer Use Profile 时适用；Screenshot、Protected Surface、焦点、输入和坐标的桌面语义均以该 Profile 为唯一事实源，不是 Core 的前提。

启用该 Profile 后，捕获前/后可支持：

- 排除受保护窗口；
- 遮盖密码、Token 和隐私区域；
- 按窗口/区域而非全屏；
- 限制远程 Channel 接收范围；
- 设置对象过期；
- 记录谁查看过；
- 由 Memory/Policy 决定是否进入长期记忆。

这些是可由规则选择的隐私能力，不能成为对认证界面、敏感画面或云模型的绝对发送禁令。启用 Profile 后，发送 Screenshot 到模型或远程 Channel 时，以当前任务相关性最小化内容，并记录最小必要元数据、ContentOrigin 和适用规则。

## 10. Optional Computer Use Profile 安全投影

Computer Use 不是 Core 必做能力，而是统一 Extension/Capability SDK 上的 optional Profile；其桌面模型、Provider 组合、Operation Snapshot、Execute Gate、输入租约、Protected Surface、焦点和旧坐标规则只由 `COMPUTER_USE.md` 定义。本文件不重复桌面专属契约。

启用该 Profile 时，它必须把下列通用安全边界投影到其 Provider 和执行路径：

- 所有桌面副作用仍受 Task、Policy、Scope、Lease、审计、恢复和系统机制约束；
- 输入会话可见、可停止且有超时；用户接管、锁屏和 session unavailable 的处理，以及目标绑定、重新观察和验证，遵循 `COMPUTER_USE.md`；
- Protected Surface 标签、目标、Snapshot generation、发送目的地、operation、resource scope 等参与判定事实必须分为 material authorization facts与observation evidence facts；
- 任一 observation变化都使旧 PermissionDecision与相关 Lease不可继续消费并要求reobserve；若Core（不是Profile）证明新的material authorization fingerprint与旧approved resolution完全等价，可由新PermissionDecision复用旧resolution；
- material fingerprint变化必须追加Approval invalidation、重评并在仍需确认时创建新request；Profile只能产出标签/证据，不能判断material等价、复用approval或签发lease；
- 全局 Stop Fence 对 Profile 的输入、桌面副作用、相关 lease 和进行中的 Provider/Extension 调用照常生效。

Profile 未启用时，Core 不拥有也不假定任何桌面捕获、焦点、输入、Protected Surface 或坐标能力。

## 11. 文件系统

Extension 获得的是声明的目录句柄/作用域，不是默认整个 HOME。

写入用户文件时应：

- 使用原子写或临时文件；
- 保留版本/备份（适用时）；
- 避免跟随越界符号链接；
- 检查路径规范化；
- 验证写后结果；
- 记录 Artifact 和变更摘要。

## 12. Network 与云调用

网络声明可按以下维度表达，供 Policy 匹配、风险展示和审计：

- Domain/endpoint；
- protocol；
- direction；
- task；
- time；
- data class。

这些维度不是默认网络封锁器。Native、AI 自写和社区 Extension 均可安装和运行；未命中规则时允许其声明的网络行为，不默认审批或无网络。对于未沙箱 Native Extension，网络/文件等权限可能只是 declaration-only；UI、审计与文档必须如实展示 OS-enforced、host-enforced 或 declaration-only，绝不虚构隔离。

云调用不按内容类别审查、脱敏或阻断数据，认证数据与 Secret 也不例外。Context Pack 仅以任务相关性最小化选择数据，不将“最小化”实现为内容安全过滤；调用审计记录恢复、计费和归因所需最小元数据。

## 13. Privilege Broker

### 原则

越接近 root/管理员，组件越简单。

Broker 只接受固定 Action ID，例如：

- install approved packages；
- restart approved service；
- update one managed config；
- enable one device rule。

禁止：

- `run_shell`；
- 任意可执行路径；
- 未约束脚本；
- 通配 `/etc` 写入；
- 透传模型生成命令；
- 长期 root session。

普通 Shell 是不经 Broker 的另一条通道，可服务于其自身已允许的非特权任务；它不得读取、修改或替换 Core，也不得绕过固定 Broker API 获得特权效果。

### 参数验证

- 严格 Schema；
- 枚举/白名单；
- 路径固定根；
- 包名/服务名约束；
- 禁止 Shell 展开；
- 验证调用者、Task、Lease；
- 行为前后检查。

### 授权租约

Lease 绑定：

- user/actor；
- task；
- action ids；
- resource；
- parameter bounds；
- expiration；
- max uses；
- local/system authentication evidence。

不能表示“任意 root”。

### 平台映射

- Linux：polkit + 受限 Broker/服务；
- Windows：UAC + 受限提升进程或服务；
- macOS：Authorization/Privileged Helper。

## 14. sudo 与密码

普通 Agent 不应通过 GUI 终端输入 sudo 密码；应优先使用固定 Broker Action。若兼容交互场景确实需要密码提示，具体输入、记录、模型传递和保存行为由匹配 Policy 与任务相关性最小化决定，并必须留下可恢复、可解释的审计。

## 15. Extension 安全

Extension：

- 默认进程外；
- 可声明所需能力、网络和数据用途；
- 不继承所有环境变量，除非适用 Policy/Task 明确授予；
- 不修改 Kernel 或伪造 Kernel Command/Event；
- 不直接互调；
- 崩溃隔离；
- 更新时保留版本、差异、来源和回滚点。

Native、AI 自写及社区 Extension 都是可安装、可运行的来源类别。是否确认权限差异、是否限制能力，均由匹配 Policy，而不是来源类别的默认审批或默认禁用决定。

Extension 的权限与风险展示必须按 `EXTENSION_SDK.md` 的实际 enforcement class 分类为 OS-enforced、host-enforced 或 declaration-only。尤其未沙箱 Native Extension 可以绕过 SDK 自检与声明式权限：host invoke 校验、最小句柄、进程监督、审计和崩溃隔离只构成 host-enforced 边界，不能被宣传为对全部旁路不可绕过的隔离；其网络、文件或输入声明可能仅是 declaration-only。

## 16. AI 自写代码

AI 可使用公开 SDK 自写、安装、运行、更新和回滚 Extension。依赖/许可证扫描、静态检查、SDK Contract Tests、沙箱运行（可用时）、权限差异、运行审计和失败回滚是可执行的质量与证据步骤，但不是无匹配规则时的安装前确认门槛。

自写代码、Skill、Memory、路由、Trigger、Delegation 与受治理配置可在 Policy、预算、停止条件、可观测性和版本回滚约束下持续改进。禁止代码生成任务读取后修改、补丁、热替换或重写 agentd Kernel、Task/Policy 解释器、Audit 完整性机制、Privilege Broker、Emergency Stop、Core Identity、安全/隔离边界或私有 Kernel API。

## 17. Audit

审计必须通过 AuditRecord 的顶层 required 字段与显式 `null` / 空数组回答：

- 谁/哪个入口发起；
- 为什么创建任务；
- 使用哪个委托；
- 哪个模型建议；
- 哪个 Extension 执行；
- 使用何权限；
- 修改何资源；
- 如何验证；
- 是否发送外部内容；
- 是否可回滚；
- ContentOrigin、匹配规则、决策排序依据与 policy mutation authority；
- Stop Fence 或恢复状态是否影响执行。

AuditRecord 的稳定引用闭包使用 `task_creation_context`、`delegation_ref`、`model_call_refs`、`verification_result_refs`、`resource_refs`、`external_content_status` / `payload_manifest_refs`、`rollback_capability`、`stop_fence_generation` / `recovery_attempt_ref` 与 `policy_context` 分别承载上述事实。`task.creation_recorded` 的 context 从同事务 TaskSpec/Task 复制 `revision=1`、goal、origin_ref、proposer，固定回答创建原因；其他类型必须为 null，避免双源泛滥。`not_sent` 必须使用空 manifest refs，`sent` 必须有 content origin、artifact、resource、model call、payload manifest 或 causation 稳定引用支撑，`unknown` 必须说明 reason code。PayloadManifest 不复制正文，当前只使用 stable ref。

`permission_decision_ref` 非空时 `policy_context` 非空；其中 nullable `matched_rule_ref` 和 `policy_set_revision` 必须与不可变 PermissionDecision 完全一致，不一致时 Audit 写入失败并回滚同一事务。决策排序摘要、policy mutation authority 和 authentication evidence 是补充快照。`rollback_capability` 只能从 ActionRequest.rollback_policy、Verification、Recovery 权威事实投影，明确值必须有事实，缺失/不可判定用 unknown，可解析事实冲突则写入失败。`provider_id` 是本次操作实际使用的 Provider；`model_call_refs` 是建议/推理参与者，两者可并存、不互相替代；对应同一次模型操作时未来持久层校验 provider 一致。

VerificationResult 已有 UUID Schema；ModelCallRecord、PayloadManifest 与 Delegation 当前尚无 source Schema，因此这里只要求非空 stable ref，不假称 UUID 或已实现持久对象。Schema 能校验创建 context、not_sent/unknown 与 PermissionDecision ref/context 的同对象条件；`kernel-sqlite` 另已实现 sent 单记录至少一个支撑引用检查。仍不能跨对象比对 PermissionDecision、回滚权威事实、引用目标存在性或 ModelCallRecord provider；这些是未来 repository 的 Conformance 义务，不得伪装成已有代码测试。

Schema 只约束 null actor 仅可出现在 `system_internal`；是否“确无可归因注册主体”必须由 producer 基于业务事实证明，否则必须保存完整 Actor revision 快照。固定归因不能仅藏进开放的 `details`；required 顶层字段能暴露缺失事实，但 Schema 无法完全禁止 producer 在 `details` 中重复归因，顶层字段是权威值。

审计默认记录满足归因、恢复、计费和规则解释的最小元数据。Security Audit 的默认展示可以使用脱敏摘要，但这只是日志层的默认展示与复制最小化策略，不是对 Memory、模型调用、Extension 数据流或云发送的内容拒绝。用户/规则可以选择保存或查看更完整内容；系统不以隐式内容审查或“Secret 永不记录”取代明确配置。

## 18. Emergency Stop

安全义务：用户或已获规则授权的 actor 必须能触发 Kernel 级 Emergency Stop；Stop Fence 是 Kernel 级通用拦截状态，其触发与生效不以任何 Profile 是否启用为前提；启用后必须拒绝新副作用、主动任务、产生副作用的 Extension 调用和远程执行；外部内容、模型或 Extension 不得清除或伪造 Stop Fence。Stop Fence 的完整行为、Lease/Resource Lock 撤销与只读诊断边界见 [`CORE_ARCHITECTURE.md` §19](CORE_ARCHITECTURE.md#19-emergency-stop-与-kernel-stop-fence)；Computer Use 输入与桌面 Provider 调用的 Profile 投影见 [`COMPUTER_USE.md` §13](COMPUTER_USE.md#13-stop-fence-协作)。

## 19. 安全验收

以下任一存在即不合格：

- Agent 持有通用 root；
- 无匹配 PolicyRule 时未得到 allow；
- Channel 直接调用 Provider；
- Extension 直接调用 Broker；
- 外部内容可伪造 Kernel Command/Event、规则证据或 Broker 请求；
- 远程昵称作为唯一身份事实；
- 启用 Computer Use Profile 后，旧 Screenshot 坐标可直接执行；
- 用户或 Policy 授权 actor 无法撤销委托或主动任务；
- 扩展更新丢失版本、差异或回滚证据；
- 审计可被普通 Extension 关闭；
- Agent 可自改 Core；
- Stop Fence 可被模型、Extension 或外部内容绕过。
