# COMPUTER_USE.md

> 本文件是跨平台 Computer Use 领域的**唯一规范事实源**：Unified Desktop Model、Provider 组合、坐标与 Snapshot、Target Resolution、动作路径、Execute Gate、Profile-local Input Session Lease、验证、平台 Provider、Visual Grounding、Application Adapter、远程监督与降级。
>
> **规范状态**：Computer Use 是 **optional / claim-conditional Extension Profile**，**不是** Shittim Core 能力，也**不是**基础产品完成项。未来通过统一 Extension / Capability SDK 作为可选 Profile 实现与分发。
>
> **源码 / Schema 依赖**（import、生成与类型所有权）：
>
> ```text
> Computer Use Profile package  →  Extension SDK public contracts  →  Core public contracts
> ```
>
> **运行时宿主控制 / 调用**（negotiation、grant、invoke、cancel）：
>
> ```text
> Core host  →  SDK host boundary  →  negotiated Computer Use Profile implementation
> ```
>
> 两张图方向相反但语义不同；不得合并为一条含糊的“单一依赖方向”。Shittim Core / Extension SDK Base **不得**反向引用 Profile 领域类型。
>
> Profile **不得**创建 Task、Policy、Approval、Stop、Audit、Recovery 的平行权威；**不得**直连 Privilege Broker。

## 1. 定位与边界

### 1.1 是什么

Computer Use 是统一桌面观察与动作系统，不是一个截图点击工具。当且仅当本 Profile 被合法启用时，它提供：

- 统一桌面模型（Display / Workspace / Window / Accessible Element / Capture Frame / Visual Candidate）；
- Provider 组合与能力探测；
- Target Resolution 与动作路径选择；
- Snapshot / Operation Snapshot；
- 坐标体系与 transform；
- Profile Readiness Gate 与（引用 Core 的）Execute Gate 协作；
- Observation-Action-Verification 循环；
- Profile-local Input Session Lease 与控制权状态；
- Protected Surface 标记（作为 Policy 输入）；
- 与 Kernel Stop Fence 的协作；
- 平台 Provider、Application Adapter、Visual Grounding、远程监督与降级。

### 1.2 不是什么

- **不是 Core**：不进入 Core Schema、KCP Catalog、Task/Action 顶层固定字段、Audit 固定字段，也不是 Core 发布门槛。
- **不是默认产品能力**：未安装或该安装/发行不提供时返回 `unsupported`；已支持但当前 disabled、quarantined、健康丢失或平台会话暂不可用时返回 `unavailable`；版本不兼容返回 `incompatible`；能力存在但当前调用未获 grant 返回 `capability_not_granted`。不得伪装为“部分 Core 能力”。
- **不是平行权限系统**：副作用仍走 Core 路径 `Task → Policy → agentd → Extension invoke → system mechanism → verify/audit`。
- **不是 Broker 客户端**：不得持有、转发或直连 Privilege Broker 凭据；特权需求只可返回 CapabilityNeed / privilege request 候选，由 Kernel 解释。

### 1.3 与 Core / SDK 的权威划分

| 权威 | 归属 | 说明 |
|---|---|---|
| Task / Action 状态机 | Core | Profile 只消费，不重定义 |
| Policy 匹配与 effect | Core / SECURITY | Freedom-first；Profile 只提供判定输入 |
| Action Lease / Resource Lock / Stop Fence | Core | Profile 依赖并服从 |
| ApprovalRecord / PermissionDecision | Core | Profile 不自建平行审批 |
| Audit 固定字段与事件目录 | Core | Profile 可贡献细节到开放通道，不得扩张固定字段 |
| Extension 生命周期 / handshake / invoke | Extension SDK 规范；运行时未实现 | Profile 作为 SDK Profile 之一 |
| Desktop model / Snapshot / Coordinate / Input Session Lease | **本 Profile-local** | 不得倒灌 Core |

## 2. 启用条件与诚实可用性

### 2.1 启用前提（全部满足才可声称“可用”）

Computer Use capability 只有在以下条件**同时**成立时，才可对调用方报告为 discovery-visible；只有另行满足调用级条件时才可 invocable：

1. **Extension 已安装且 enabled**：对应提供具体 `computer.<facet>` 的扩展处于可运行生命周期（非 disabled / quarantined / incompatible）；
2. **SDK / Profile 版本协商成功**：握手完成，协议版本与声明的 Computer Use Profile/facet 集合在双方兼容范围内；
3. **正式 versioned operation Schema 存在且兼容**：该 operation 的 payload、result，以及声明事件能力时的 event Schema 均已发布、可解析，并绑定到已协商版本；
4. **实际 capability probe 已注册**：扩展完成 probe，Kernel Registry 中存在**实测**能力档案，而非仅 Manifest 文本；满足 1–4 才可 discovery-visible；
5. **调用级 grant 与 Core 边界通过（invocable 条件）**：副作用调用必须有 capability grant、Task/Action、scope、Action Lease、Resource Lock、Policy 与 Stop Fence 检查；无副作用的只读观察不需要虚构 Task grant 或 Action Lease，但仍必须满足 1–4、调用身份/scope 及 operation Schema 校验。

`discovery-visible` 只表示 Registry 可诚实发现，`invocable` 才表示当前调用可以执行；不得把两者混为 `available`。

### 2.2 Manifest 声明 ≠ 支持

- Manifest 中的 `profiles` / `requested permissions` / platform 列表只是**安装级上限与意图声明**。
- 未 probe、probe 失败、运行时降级、平台 API 拒绝、会话不可用时，必须以**实际能力**为准。
- 禁止用“Manifest 写了 computer.input”对外或对 Planner 宣称输入已支持。

### 2.3 未启用时的诚实错误

| 情况 | 必须返回 | 禁止 |
|---|---|---|
| 当前安装、发行物或已协商 facet 集合不提供该 operation | `unsupported` | 静默 no-op、假成功 |
| 已支持，但扩展 disabled / quarantined、健康丢失、平台会话或 backend 当前不可用 | `unavailable` | 当作 allow 继续 |
| SDK/Profile/operation Schema 版本无兼容交集 | `incompatible` | 回退到未协商的私有协议 |
| capability 已发现且 operation 兼容，但当前调用没有所需 grant | `capability_not_granted` | 用 Manifest、probe 或 Policy Default Allow 冒充 grant |
| payload/result Schema 缺失或未协商；事件能力的 event Schema 缺失或未协商 | `incompatible`（已有 operation id 但版本无兼容合同）或 `unsupported`（当前集合根本不提供） | 当作 Policy Default Allow |
| Policy 无匹配规则 | Freedom-first：`allow`（仅在能力与合同已成立时） | 用“能力未协商”冒充 Policy allow |

**关键区分**：

- **Policy Default Allow（Freedom-first）**：能力合同已成立、Action 已进入合法评估时，无匹配规则 → allow。
- **合同/能力缺失 fail closed**：当前集合不提供 → `unsupported`；已支持但当前不可用 → `unavailable`；版本不兼容 → `incompatible`；已存在但当前调用未获 grant → `capability_not_granted`。这些都**不是** Policy 未匹配，不得落入 Default Allow。

## 3. Claim 记录、成熟度与对外声明

### 3.1 语言中立规范概念

本阶段**不新增 JSON Schema、crate 或生成类型**，但所有 Manifest、Conformance 报告、发行配置与文档声明必须能投影为同一组语言中立记录：

```text
ProfileClaim {
  claim_id: string
  maturity: contract-only | schema/SDK | composition | provider contract | real-platform
  distribution_asserted: boolean
  evidence_refs: [stable-ref, ...]
}
```

字段语义：

- `claim_id`：第 4 节登记的精确稳定 id；禁止通配符、自然语言别名和临时拼接 id；
- `maturity`：该 claim 已达到的实现/验证阶段，**只**允许上面五个 token；
- `distribution_asserted`：是否由发行物、安装包、Manifest、用户文档或发布说明向外承诺支持；它是与 `maturity` **正交**的布尔事实，不是第六个成熟度；
- `evidence_refs`：支持该成熟度及套件结果的稳定证据引用；空引用不能支撑 `schema/SDK` 及以上成熟度或任何对外支持声明。

同一发行评审只能读取一个去重后的 `ProfileClaim` 集合 `C`；同一 `claim_id` 最多一条记录。若多个输入源对同一 id 给出不同事实，生成发布报告前必须合并为最高被证据证明的 maturity，并对 `distribution_asserted` 做逻辑或；冲突或证据不足时失败，禁止任选一份较宽声明。

### 3.2 成熟度

| maturity | 含义 | 可声称的内容 | 不可声称的内容 |
|---|---|---|---|
| **contract-only** | 本规范文本与概念合同存在 | 领域词汇、门控结构、对象边界已定义 | 任何可运行支持 |
| **schema/SDK** | Profile Schema、Extension SDK Base Profile 面、错误模型已接线 | 握手/invoke 形状兼容 | 真实桌面操作 |
| **composition** | 多 Provider 组合、Target Resolver、Snapshot 融合逻辑存在 | 在 mock/fixture 上组合正确 | 某 OS/compositor 真支持 |
| **provider contract** | 某一 Provider 通过合同测试（可 mock 平台） | 该 Provider 接口与语义符合合同 | 真机稳定性 |
| **real-platform** | 在声明的 OS/compositor/会话上通过真实环境测试 | 该平台实测能力与限制 | 未测平台的同等能力 |

规则：

- 每个 capability（如 `computer.scene`、`computer.input`）与每个 platform claim（如 Hyprland、niri、Windows）独立记录 maturity；
- Conformance 的 `IF_CLAIM(id)`、`IF_REAL(id)` 和发布门只读取集合 `C`，不得再从散文、包名或 family 名推断支持；
- 任一 `computer.*` 记录都先触发 `CONFORMANCE.md` §11.0；`IF_CLAIM(id)` 在 `C` 中存在该精确 id 时成立；`IF_REAL(id)` 仅在同一记录 `maturity = real-platform` 时成立；
- `distribution_asserted=true` 使该记录及其依赖闭包成为该发行物硬门；它不能把较低 maturity 自动升级，也不能用 `contract-only` 文本冒充可运行支持；
- Core 发布物默认不包含任何 `distribution_asserted=true` 的 Computer Use claim。

### 3.3 依赖闭包与“完整 Computer Use”

Claim 依赖闭包由本规范固定，发布工具未来可以机械实现；本阶段先以规范算法验收：

- `computer.snapshot` → 至少一个实际观察来源：`computer.scene | computer.semantic | computer.capture | computer.visual_grounding | computer.application`；
- `computer.operation.verify` → `computer.operation.execute`；
- 任何实际输入执行路径 → `computer.operation.execute` + `computer.input`；
- `computer.backend.capture` → `computer.capture`；`computer.backend.input` → `computer.input`；
- 任一 platform claim 只证明该平台记录，不自动补全 scene/capture/input 等 facet；所宣传 facet 必须逐项存在；
- 所有闭包成员必须各自有 `ProfileClaim`，并具有足以通过映射套件的 maturity/evidence；禁止用一个 `ALL(...)` 字符串、family 通配符或父 claim 代替成员记录。

对外使用“完整 Computer Use”“full desktop control”或等价无保留措辞时，集合 `C` 必须机械包含且 `distribution_asserted=true`：

1. 全部 facet：`computer.scene`、`computer.semantic`、`computer.capture`、`computer.input`、`computer.event`、`computer.visual_grounding`、`computer.application`；
2. 全部操作组合：`computer.snapshot`、`computer.operation.execute`、`computer.operation.verify`；
3. 至少一个明确 supervision mode claim；
4. 至少一个明确 platform claim，且该 platform 记录达到 `real-platform`；
5. `computer.backend.capture` 与 `computer.backend.input`。

上述是集合/依赖闭包校验，不是一个可含糊填写的 `ALL` 标签。缺任一成员、证据或条件时只能声明实际子集，不能对外写“完整”。当前仓库没有任何 Computer Use `ProfileClaim` 达到 `schema/SDK`，更没有 distribution assertion。

## 4. Profile 与 Capability 面

Computer Use 通过 Extension SDK 的 Profile 族表达（与 `EXTENSION_SDK.md` 对齐），而不是 Core 内置模块：

| Profile | 职责 |
|---|---|
| `computer.scene` | 显示器、工作区、窗口、焦点、布局、窗口事件 |
| `computer.semantic` | 无障碍语义树与元素动作 |
| `computer.capture` | 窗口/显示器/区域/持续画面捕获 |
| `computer.input` | 键盘、指针、滚动、触控 |
| `computer.event` | 桌面或应用事件合同；当前仅定义未来 Profile operation/event 语义，不代表已有 Extension Event transport、持久化或订阅 |
| `computer.visual_grounding` | 从画面产生视觉候选；**不**直接拥有输入权限 |
| `computer.application` | Application Adapter：通过浏览器 DOM/CDP、编辑器插件、办公软件 API 等应用专用接口提供高可靠性观察与动作候选；仍服从统一 Target Resolver、Core 边界与完整 Execute Gate |

运行时可组合多个 Provider / 多个 Profile。一个 Provider 不必实现全部能力。Application Adapter 可作为更高可靠性路径接入同一 Capability 解析链。

### 4.1 能力探测档案

不得仅根据桌面名称判断能力。能力档案至少描述：

- window enumerate / focus / move / resize；
- workspace enumerate / activate；
- semantic tree / action / value；
- display / window / region capture；
- keyboard / pointer / scroll / touch；
- absolute / relative input；
- desktop / application events；
- persistent permission（平台授权状态）；
- multi-display；
- fractional scale；
- reliability 与已知限制。

桌面名称只用于选择专有增强 Provider，不用于伪造能力。

### 4.2 非 facet 的稳定 claim id

以下 claim id 用于 Conformance 门控，不扩张上面的 facet Catalog：

| Claim id | 含义 |
|---|---|
| `computer.snapshot` | Snapshot / Operation Snapshot 组合 claim；必须同时声明至少一个实际观察来源 facet |
| `computer.operation.execute` | 发行物声明至少一个 Computer Use 副作用 operation 可执行 |
| `computer.operation.verify` | 发行物声明执行结果的 Profile 验证路径 |
| `computer.supervision.automatic` | automatic 远程监督模式 |
| `computer.supervision.approval` | approval 远程监督模式 |
| `computer.supervision.cooperative` | cooperative 远程监督模式 |
| `computer.supervision.observe_only` | observe-only 远程监督模式 |
| `computer.platform.hyprland` | Hyprland Provider claim |
| `computer.platform.niri` | niri Provider claim |
| `computer.platform.windows` | Windows Provider claim |
| `computer.platform.macos` | macOS Provider claim |
| `computer.backend.capture` | 产品化 capture backend claim |
| `computer.backend.input` | 产品化 input backend claim |

新增平台或模式 claim 必须先在本节登记精确 id，再由 `CONFORMANCE.md` 引用；不得在门控表达式中临时写省略号、通配符或自然语言别名。

## 5. Profile-local 对象模型（禁止倒灌 Core）

以下对象属于 **Computer Use Profile-local**。它们可以：

- 出现在 Profile Schema、Extension invoke payload、Object Handle、Profile 侧事件与诊断中；
- 被 Audit / Memory **引用**（stable ref / artifact handle），或写入开放 `details`（若实现允许）；

它们**不得**：

- 成为 Core JSON Schema 的顶层必选对象；
- 进入 KCP Envelope / 首批 command·query Catalog 的固定字段；
- 扩张 Task / Action / Approval / PermissionDecision / AuditRecord 的**固定字段**集合；
- 被伪装成 Kernel 权威状态机的一部分。

### 5.1 Display

- stable id；
- name；
- logical geometry；
- physical size / pixels；
- scale；
- transform / rotation；
- primary；
- capture mapping。

### 5.2 Workspace

- id；
- name / index；
- output relation；
- active / visible；
- platform semantics。

Workspace **不能**假设所有平台都是二维虚拟桌面。

### 5.3 Window

- provider identity；
- application identity；
- title；
- process / app metadata；
- workspace / output；
- logical geometry；
- focus；
- visibility / interactability；
- state；
- generation / version。

### 5.4 Accessible Element

- stable-within-snapshot id；
- provider ref；
- role；
- name / description / value；
- states；
- actions；
- parent / children；
- window ref；
- geometry with coordinate space；
- reliability。

### 5.5 Capture Frame

- frame id；
- source display / window / region；
- pixel dimensions；
- transform to logical space；
- timestamp；
- content hash；
- privacy redactions；
- object handle。

### 5.6 Visual Candidate

- candidate id；
- bounding region；
- label / description；
- interactability probability；
- model / provider；
- relation to semantic elements；
- confidence。

### 5.7 Snapshot

Snapshot 是同一时间窗口内融合后的桌面观察（Profile-local）：

- desktop generation；
- windows；
- selected semantic tree；
- capture frames；
- visual candidates；
- coordinate transforms；
- candidate numbers；
- expiration policy。

### 5.8 Operation Snapshot

面向用户的操作快照（Profile-local）包含：

- 图像；
- 目标窗口 / 应用；
- 当前任务意图；
- 相关候选编号；
- 简短语义；
- 可靠性与风险；
- 建议动作；
- Snapshot id / generation；
- 过期提示。

### 5.9 Coordinate 与 Transform

任何矩形必须带 coordinate space 与 transform version。坐标系至少区分：

- physical pixel；
- logical display；
- compositor / global（若存在）；
- capture frame；
- window-local；
- content / client area；
- semantic provider coordinates。

禁止假设：

```text
AT-SPI rect == screenshot pixels == input coordinates
```

旧坐标在以下情况后失效：

- 工作区 / viewport 变化；
- 窗口移动、缩放、动画；
- DPI / scale 变化；
- Capture Source 改变；
- 应用布局变化；
- 新 Snapshot generation。

### 5.10 Profile-local Input Session Lease

见第 12 节。该对象**不是** Kernel Permission Lease / Privilege Lease / Core Action Lease。

## 6. 可靠性等级

建议从高到低：

- **R5**：应用专用 API 已确认；
- **R4**：原生语义动作 / 状态已确认；
- **R3**：语义目标 + 可靠几何；
- **R2**：规则 / 快捷键；
- **R1**：视觉 Grounding；
- **R0**：未验证坐标猜测。

### 6.1 R0 与风险等级的定位

- R0 仍可作为可靠性事实记录：表示未验证坐标猜测、证据最弱；
- 是否允许 R0 或任何低可靠性路径执行副作用，由 **Policy 与 verification 配置**决定；
- 不得写“R0 默认禁止用于副作用”；
- 不得把高风险动作“默认阻断”或“低视觉默认禁止”写进本规范；
- 风险等级与可靠性等级是判定输入，不是偷偷默认化的 deny 矩阵；
- Policy 可匹配规则要求更高可靠性或更强验证；**无匹配规则时遵循 Freedom-first（allow）**——前提是第 2 节启用条件与能力合同已成立。

## 7. Target Resolver

候选来源：

- 应用 Adapter；
- Semantic Provider；
- Scene；
- 快捷键规则；
- Visual Grounding；
- 用户指定编号。

Resolver 负责：

- 去重；
- 可见性；
- 可交互性；
- 当前任务相关性；
- 可靠性排序；
- 歧义检测；
- 是否需要局部重新观察。

模型可以参与相关性排序，但不能伪造平台事实。Resolver 本身不授权、不绕过 Policy、不替代 Execute Gate。

## 8. 动作选择

在 Profile 已启用且相关 capability 已 probe 的前提下，选择顺序建议为：

1. 应用专用 API；
2. Accessibility Action；
3. Window / Workspace Action；
4. 已知快捷键；
5. Semantic Geometry + Input；
6. Visual Candidate + Input；
7. 停止并请求用户。

选择必须考虑：

- side-effect class；
- reliability；
- provider / capability availability（实测，非 Manifest）；
- current focus（**仅作候选排序信号或 Prepare 提示，不得作为唯一目标绑定**）；
- protected surfaces（按 Policy）；
- user supervision mode；
- verification availability；
- **Core Execution Boundary** 与 **Profile Readiness Gate** 结果。

不可用的更高路径应降级到下一可用路径，或进入 observe-only / 请求用户。任何仍会改变目标状态的降级路径须通过完整 Execute Gate（第 10 节）；observe-only 只读降级不触发副作用 Execute Gate，但仍须满足观察 operation Schema、scope 与数据策略。

## 9. Observation-Action-Verification

### 9.1 Observe

建立 Snapshot，不假设旧状态有效。若 capture / scene / semantic 未启用，只能在已 probe 的子集上观察，并报告限制。

### 9.2 Resolve

选择目标和候选替代项。

### 9.3 Prepare

- 聚焦正确窗口；
- 切换工作区 / viewport；
- 等待动画和布局稳定；
- 确认没有遮挡；
- 获取 Core Resource Lock（如 input session / window）与 **Profile-local Input Session Lease**（若将输入）。

### 9.4 Reobserve after Prepare

Prepare 可能改变焦点、布局、viewport 或遮挡关系。Prepare 完成后必须 reobserve 相关目标，再进入 Execute Gate 的最终检查。禁止把 Prepare 前的坐标 / generation 直接当作成熟执行依据。

### 9.5 Execute

通过完整 Execute Gate 后执行最可靠动作。

### 9.6 Observe Successor

重新获取相关状态，而不是仅相信 Provider 返回成功。

### 9.7 Verify

根据 Task 成功标准判断：

- 元素状态变化；
- 窗口出现 / 消失；
- 文件 / 文档变化；
- 应用反馈；
- 视觉差异；
- 外部系统确认。

验证失败可：重新观察、换路径、回滚、请求用户或失败——由 Task 策略、恢复事实与匹配 Policy 决定。

### 9.8 焦点不是目标身份

- 当前焦点窗口或元素不得作为唯一目标绑定，也不得把“仍然有焦点”当作稳定身份；
- 焦点只可用于候选排序，或作为 Prepare 阶段“需要聚焦/切换”的提示；
- 执行目标必须绑定 stable provider ref、semantic ref、Snapshot/generation ref、用户明确选择的 candidate ref，或 Profile Schema 定义的其他稳定引用；
- Prepare 后必须 reobserve 并重新解析稳定引用；若调用只携带焦点或无法解析稳定目标，返回结构化 `stale_state` / `target_not_found`，且 Profile Readiness Gate 失败。

### 9.9 stale_state

在以下情况必须视为过期并返回 `stale_state`（或先 reobserve 再判定）：

- Snapshot / desktop generation 过期；
- window generation 变化；
- Prepare 后未 reobserve 却仍用旧证据；
- 工作区 / viewport、DPI / scale、capture source 变化导致坐标空间失效；
- provider ref 失效且无法安全重定位；
- Input Session Lease 期间目标窗口已切换到不兼容上下文。

禁止直接使用旧图 x/y 或过期 generation 执行。

## 10. Execute Gate（双层，不可互相替代）

执行桌面副作用前必须通过 **完整 Execute Gate**。它由两层组成；**任一失败则不得执行输入或改变目标状态的动作**。

```text
Complete Execute Gate
  = Core Execution Boundary
  + Profile Readiness Gate
```

### 10.1 Core Execution Boundary（引用 Core，权威在 Core）

本层**不**由 Computer Use 重新定义，只引用并遵守 `CORE_ARCHITECTURE.md` / `SECURITY_PRIVILEGE.md` / `EXTENSION_SDK.md`：

1. **Task / Action**：存在合法 Task 与 Action；状态允许派发；幂等与副作用语义明确；
2. **Scope**：目标资源落在 TaskScope / resource scope 内；
3. **Policy**：当前 Actor / EntryPoint / Task / Action 的 Policy 判定为 allow，或已满足 confirm / lease 条件；
4. **Approval**：若 Policy 要求 confirmation / local confirmation / system authentication，则存在有效 ApprovalRecord；
5. **Action Lease**：Core Action Lease 未过期，绑定正确 task / action / holder；
6. **Resource Lock**：所需 opaque stable resource refs 的锁已持有且未超时；desktop session、input device/session、window 等映射由 Profile readiness / Input Session Lease 章节定义，Core 不解释这些桌面类型；
7. **Stop Fence**：Kernel Stop Fence 未激活；激活期间本层必须失败；
8. **capability grant**：将产生副作用的对应精确 `computer.<facet>` operation 已 discovery-visible 且当前 granted；host invoke 校验通过。只读 observation 不虚构副作用 grant，但仍须符合第 2.1 节。

本层失败时返回 Core / SDK 语义错误（如 `permission_denied`、`approval_required`、`lease_expired`、`scope_violation`、`capability_not_granted`、Stop 相关拒绝等），**不得**用 Profile 内部“看起来能点”覆盖。

### 10.2 Profile Readiness Gate（本 Profile 专属）

本层只在 Core Execution Boundary 已通过（或并行求值但最终必须同时通过）的前提下有意义；**不能替代** Core 边界：

1. **Snapshot / window generation**：目标绑定的 Snapshot 与 window generation 仍有效、未过期；
2. **Provider ref**：目标仍可通过 provider ref 或等价稳定引用定位，不是裸旧坐标；
3. **可见且可交互**：目标 visible / interactable；遮挡、最小化、off-screen、锁屏等导致失败；
4. **Coordinate / transform**：动作坐标空间与当前 transform version 一致；
5. **Provider / backend 可用**：所需 scene / semantic / capture / input backend 处于 ready，而非 degraded 到不可执行；
6. **Resource refs 与 Input Session Lease（若输入）**：desktop session、window、input device/session 等仅作为 Profile 映射到 Core 的 opaque stable resource refs；Core 不理解这些桌面类型。相应 Resource Lock 已持有，Profile-local Input Session Lease 未闭合、未过期，且绑定正确 task / action / scope；
7. **稳定目标绑定**：目标不是仅凭当前焦点；必须有 provider / semantic / Snapshot generation / user-selected candidate 等稳定引用，且当前仍可解析；
8. **控制权状态**：不在 `user_control` / `waiting_user` / `paused` / `unavailable` 等禁止 Agent 输入的状态；
9. **Protected Surface 证据新鲜度**：Profile 只负责产出当前表面的标签、分类置信度、来源与证据引用，并证明这些事实仍对应当前 Snapshot/generation；本层不得把标签解释成 Policy allow / confirm / deny；
10. **Reobserve 义务**：Prepare 后已 reobserve；niri 等平台的 `make target interactable → wait → reobserve` 已完成。

Protected Surface标签、目标、Snapshot/generation、发送目的地、operation、resource scope或其他参与判定的事实发生变化时，Kernel必须把事实分成两类：

- **material authorization facts**：授权对象/语义、operation、资源、目的地、Protected Surface实质分类等；
- **observation evidence facts**：snapshot/generation/provider ref/坐标transform/观察时间与证据refs等瞬时定位事实。

任一observation变化都使旧PermissionDecision与相关Permission/Privilege Lease不可继续消费，并要求reobserve与新decision。若且仅若Core（不是Profile）证明新的material authorization fingerprint与旧approved Approval v2 resolution绑定值完全等价，且旧resolution未失效/过期，新PermissionDecision可引用复用该resolution。material fingerprint变化必须追加invalidation、重评并在仍需确认时创建新request。Profile只能报告变化和证据，不能判断material等价、保留旧allow、复用Approval或签发lease。

### 10.3 失败语义

任一检查失败：

- 不得执行输入或改变目标状态的动作；
- 返回结构化错误（常见 `stale_state`、`permission_denied`、`approval_required`、`target_not_found`、`lease_expired`、`unavailable`、`unsupported`、`capability_not_granted` 等）；
- 可触发重新观察、换路径、请求用户或失败，取决于错误语义与 Task 策略；
- **不得**将 Profile Readiness 失败改写成 Policy deny，也**不得**将 Core 拒绝改写成“再试一次点击”。

### 10.4 用户回复“点 7”

流程：

1. 解析 Operation Snapshot 与 candidate（Profile-local）；
2. 检查授权入口与 Task（Core）；
3. 重新观察目标窗口；
4. 通过 provider ref / 语义重新定位；
5. 若变化过大，生成新 Snapshot；
6. 重新评估 Policy / Approval / Action Lease / Input Session Lease；
7. 通过完整 Execute Gate 后执行与验证。

禁止直接使用旧图 x/y。

## 11. 用户图与模型图分离

用户可获得完整清晰标注图。

模型优先获得：

- 结构化元素；
- 候选摘要；
- 局部裁剪；
- 必要截图。

这使远程可视化不等同于每一步都消耗大图 Token。

是否将某帧发送云模型，由 Policy 与数据 / 隐私策略决定（见 Protected Surface）。Context Pack 仅以任务相关性最小化选择内容，不因内容分类擅自阻断。

### 11.1 编号策略

默认只标 3–12 个任务相关候选。

模式：

- **focused**：任务相关；
- **explore**：当前区域主要元素；
- **debug**：尽量完整，仅开发者使用。

标注应避免重叠，并对低置信度候选做不同提示。

## 12. 控制权与 Profile-local Input Session Lease

### 12.1 控制权状态（Profile-local 观察 + 与 Core 锁协作）

状态：

- `agent_control`；
- `user_control`；
- `waiting_user`；
- `paused`；
- `unavailable`。

用户接管时，Core 要求抢占 Agent 对 input session 的 **Resource Lock**；Profile 必须同步闭合或挂起 Input Session Lease。

### 12.2 Input Session Lease ≠ Kernel Permission Lease

**Profile-local Input Session Lease** 是桌面输入会话的 Profile 合同对象，用于：

- 绑定一次或一组输入动作到 task / action / actor / entry / 目标 scope；
- 携带超时、最大动作次数、创建时 Snapshot / generation 提示；
- 在用户接管、锁屏、Stop、Policy 撤销等条件下闭合。

它**不是**：

- Kernel **Permission Lease** / **Privilege Lease**；
- Core **Action Lease**（Action 派发租约）；
- 可替代 Policy / Approval 的授权证明。

它**依赖**：

- Core **Action Lease**（派发与 holder 生命周期）；
- Core **Resource Lock**（input device/session、window 等）；
- Kernel **Stop Fence**（激活即禁止新输入副作用）。

### 12.3 Lease 字段（Profile-local）

- lease id；
- task / action 绑定；
- actor / entry point；
- 目标 scope（session / window / resource）；
- 超时与最大动作次数（按配置）；
- 创建时 Snapshot / generation 提示；
- 撤销条件。

### 12.4 闭合条件

- 正常完成并 release；
- 超时；
- 用户接管；
- Stop Fence / 紧急停止；
- 锁屏 / session unavailable；
- Policy 撤销；
- Task 取消或失败恢复；
- 底层 Action Lease 失效或 Resource Lock 丢失。

闭合后不得继续发送输入；恢复时必须重新观察并重新获取 Input Session Lease（若仍需要），并再次通过完整 Execute Gate。

### 12.5 用户接管（Profile 合同 + Core 锁）

用户接管时：

- Agent 输入调用必须取消或完成当前原子单位；
- Kernel / host 将输入 **Resource Lock** 转移给用户；
- Agent 不再发送输入；
- Input Session Lease 闭合或挂起为不可用；
- 恢复时重新观察，并通过完整 Execute Gate。

### 12.6 锁屏与 session unavailable（Profile 合同）

屏幕锁定或会话不可用后：

- Input Session Lease 默认失效；
- 新的输入动作不得执行；
- 状态进入 `unavailable` 或等价；
- 解锁后必须 reobserve，不得沿用锁前坐标。

## 13. Stop Fence 协作

Computer Use **服从**全局 Kernel Stop Fence / Emergency Stop（权威在 Core）：

- 停止后不得发起新的输入或 privileged desktop 副作用；Fence 后仍可新发无副作用只读诊断；
- Core 撤销所有受 Stop 影响且仍活跃的副作用 Action Lease，并在同一原子状态变更中释放它们持有的全部 Resource Lock；Profile-local Input Session Lease 随底层 Action Lease/Lock 失效而闭合；
- Core 撤销所有临时 Permission Lease / Privilege Lease 并拒绝任何后续消费，Profile 不得缓存或复用旧租约；
- Core 向所有 in-flight Extension 调用发 cancel，包括只读调用；取消只读调用不得删除已经取得的诊断事实或把它们强制标为 `unknown_side_effect`；
- 可安全取消且确认未产生副作用的动作正常闭合；只有不可安全中断或取消后结果仍不确定的副作用 Action 标记恢复待查；
- 停止不依赖模型合作；
- Fence 激活期间 **Core Execution Boundary** 对副作用必须失败，因而完整 Execute Gate 失败；
- 第一版 Fence 持久保持且没有解除 API，未来解除只能由独立恢复契约定义。

Provider 只执行取消 / 停止指令，不拥有 Fence 权威状态。

## 14. Protected Surfaces

Protected Surface 不是写死的“认证界面绝不上云”硬禁清单 alone，而是由 Policy 决定对特定表面的：

- 观察（observe / capture）；
- 输入（input / action）；
- 云发送（send frame / content to cloud model or remote channel）。

系统应识别并标记常见敏感表面，作为 Policy 输入，例如：

- Agent 宿主 / 调试控制台；
- Companion 主控制界面；
- 权限审批窗口；
- 系统认证 / 密码界面；
- Secret 管理界面；
- 紧急停止控件。

规则：

- Profile只产出标签、分类置信度、来源、Snapshot/generation与证据引用；不得决定或缓存Policy allow/confirm/deny；
- Kernel将事实分为material authorization fingerprint与observation evidence fingerprint；observation变化使旧decision/lease失效并reobserve，只有Core证明material等价后新decision可复用旧approved resolution；material变化必须invalidation+重评；
- 默认行为由Policy配置；本规范不保留“认证界面绝不上云”的不可配置硬禁作为唯一安全模型；
- 实现可提供安全的出厂推荐规则，但推荐规则不是规范级硬编码 deny；
- 无匹配规则时 Freedom-first：allow（能力已启用、合同成立且 Kernel 已对当前上下文完成评估的前提下）；匹配规则可 deny / confirm / require_local 等；
- Computer Use 不得从受保护表面提取 Secret 到普通日志或未授权通道；这属于 Secret / 数据策略，与“是否允许观察存在”分开。

## 15. 平台 Provider（声明条件生效）

以下平台深度**仅在**对应 Extension 安装启用、Profile 协商成功、capability probe 注册，且（若对外宣传）对应 `ProfileClaim.distribution_asserted=true` 时生效。未声明时不得假装完整支持。

### 15.1 Linux 通用层

- AT-SPI；
- XDG Portal；
- PipeWire；
- 可组合 Input；
- 通用桌面事件；
- X11 兼容 Provider（可选）。

### 15.2 Hyprland Scene Provider

负责：

- outputs；
- workspaces；
- clients / windows；
- focus；
- special workspace；
- layout / animation stability；
- events；
- window activation and placement。

不重复实现 AT-SPI 和视觉模型。

### 15.3 niri Scene Provider

负责：

- outputs / workspaces / windows；
- scrolling layout / viewport；
- visible / partially-visible / interactable；
- focus causing layout movement；
- dynamic capture target integration（可用时）；
- event-driven stabilization。

niri 操作前必须 `make target interactable → wait → reobserve`。该 reobserve 是 Profile Readiness Gate 的前置事实来源。

### 15.4 Generic Wayland

缺少专用 Scene Provider 时，可以提供语义、截图和输入的部分能力，但必须报告窗口管理限制，不得假装完整支持。

### 15.5 Windows

Provider 可使用：

- UI Automation；
- Win32 / DWM window facts；
- Windows capture APIs；
- semantic action and input；
- UAC / privileged path 与普通 Computer Use 分离。

高完整性窗口可能拒绝普通输入，必须返回 permission / integrity 错误。

### 15.6 macOS

Provider 可使用：

- Accessibility / AX；
- ScreenCaptureKit；
- 原生窗口和事件接口；
- CGEvent 等输入；
- 系统 Accessibility / Screen Recording 权限状态。

Spaces 和后台窗口限制必须作为能力事实返回。

## 16. Application Adapter — `computer.application`

应用专用 Adapter 可优先使用：

- Browser CDP / DOM；
- 编辑器插件；
- 办公软件 API；
- 专业应用脚本接口。

规则：

- 仍通过 Computer Use / Capability SDK 接入；
- 不得绕过 Task、Policy、Action Lease、Resource Lock、Stop Fence；
- 不得直连 Broker；
- 其更高可靠性路径仍须通过完整 Execute Gate。

## 17. Visual Grounding

OmniParser 等只产生候选，不直接执行输入。

视觉候选必须与语义元素去重，并携带模型版本和置信度。

视觉无法保证找全，因此目标是找到当前任务需要的元素，而不是枚举所有可点击区域。

低视觉置信度是可靠性与 verification 输入；是否允许执行由 Policy 与配置决定，不得规范级默认阻断。

`computer.visual_grounding` Profile **不**隐含 `computer.input` 授权。

## 18. Event 合同 — `computer.event`

Computer Use 的事件能力当前仅是未来 Profile 合同：`computer.event` operation/event payload 必须先有正式 versioned Schema，并在 SDK/Profile negotiation 中协商。当前没有正式 Extension Event Schema、transport、persistence 或 subscription 合同，因此不得声称 Kernel 可保存、索引、稳定转发或通过 Core EventEnvelope / Outbox 发布 `snapshot.created`、`user_takeover.started` 等事件。若未来建立该协议，仍不得自动晋升为 Core Event Catalog。

## 19. 远程监督模式

在 Profile 已启用且声明对应 supervision mode claim 的前提下：

- **automatic**：在 Policy 与完整 Execute Gate 允许时自动执行，关键节点汇报；
- **approval**：当 Policy 或用户配置要求时，在执行前发送 Operation Snapshot 等待确认；
- **cooperative**：用户通过编号和方向逐步指挥；
- **observe-only**：只截图 / 状态，不输入；该只读模式不得被要求通过 `computer.input`、副作用 Execute Gate、Action Lease 或远程执行 grant，但仍需观察 operation Schema、scope 与数据策略成立。

“approval 模式”是监督配置，不是“高风险默认阻断”矩阵。高风险 / 低置信度是否需要 Snapshot 确认，由 Policy 与 verification 配置决定。未声明远程监督模式时，不运行该模式套件。

远程入口仍受 Core Channel / ContentOrigin 约束：外部内容不能伪造 Kernel Command、Event、Policy mutation evidence 或 Broker 请求。

## 20. 降级

示例（均以**已声明且 probe 的 capability**为边界）：

- Semantic Action 不可用 → 快捷键或语义几何；
- Window Capture 不可用 → Display Capture + crop；
- Scene 不完整 → 用户手动聚焦或兼容模式；
- Visual 不可用 → 仅语义；
- Input 不可用 → observe-only；
- Verification 不可用 → 按 Policy 请求确认、换路径或继续（若规则允许）；不得在本规范写死“拒绝高风险动作”。

规则：

- 仅观察的降级不触发副作用 Execute Gate；任何将改变目标状态的降级路径仍须通过完整 Execute Gate；
- 降级必须反映到能力档案与用户 / 审计可见状态（degraded），不得静默假装完整；
- 无任何可用观察/执行路径时返回 `unavailable`，不是 Policy deny。

## 21. Provider 定位与参考实现

现成组件可作为 Provider 或参考（在声明对应 Profile 时）：

- computer-use-linux：Linux 语义、窗口、截图与输入；
- pi-computer-use：Windows / macOS 和状态式操作参考；
- OmniParser：视觉候选；
- Set-of-Mark：标注方法。

核心（Shittim Core）与 Extension SDK **不得**被任一组件的数据模型绑定。Computer Use Profile-local 模型可适配这些组件，但不能把第三方字段倒灌为 Core 固定 Schema。

## 22. 与 Policy / Kernel / SDK 的关系（总结）

- Computer Use 是 optional Extension Profile，不是 Core；
- 不自建平行 Task / Policy / Approval / Stop / Audit / Recovery 权威；
- 源码/Schema 依赖是 `Profile package → SDK public contracts → Core public contracts`；运行时控制流是 `Core host → SDK host boundary → negotiated Profile implementation`，不得混称；
- 所有副作用仍走 `Task → Policy → agentd → Extension → system mechanism → verify/audit`；
- Provider 成功不等于目标成功；
- 风险等级、可靠性等级、Protected Surface 标签与证据都是判定输入；Profile 不决定 Policy effect；
- **Freedom-first**：能力与合同已成立且无匹配规则时 allow；confirm / deny 只来自匹配规则、系统机制或恢复需要；
- **Fail closed on missing contract**：当前集合不提供 → `unsupported`；已支持但当前不可用 → `unavailable`；版本不兼容 → `incompatible`；已存在但调用未获 grant → `capability_not_granted`；它们都**不是** Default Allow；
- Profile-local 对象（Desktop model、Snapshot、Operation Snapshot、Coordinate、Input Session Lease 等）不得倒灌 Core Schema / KCP / Task / Action 顶层 / Audit 固定字段；
- `computer.event` 当前只是未来合同职责；没有正式 Extension Event Schema/transport/persistence/subscription 时，不进入 Core EventEnvelope / Outbox。

## 23. 文档交叉引用

| 主题 | 事实源 |
|---|---|
| Task / Action / Resource Lock / Action Lease / Stop Fence | `CORE_ARCHITECTURE.md` |
| Policy / Freedom-first / Protected Surface 安全语义 / Broker | `SECURITY_PRIVILEGE.md` |
| Extension Profile、握手、probe、invoke、错误模型 | `EXTENSION_SDK.md` |
| 条件性 Conformance 与发布门槛 | `CONFORMANCE.md` |
| KCP / Schema 固定字段（Computer Use 不得扩张） | `IMPLEMENTATION_CONTRACTS.md` |
