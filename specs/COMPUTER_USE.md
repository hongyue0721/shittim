# COMPUTER_USE.md

> 本文件是跨平台 Computer Use、Provider 组合、坐标、操作快照、Execute Gate、输入租约和验证的唯一事实源。

## 1. 定位

Computer Use 是统一桌面状态和动作系统，不是一个截图点击工具。

核心负责：

- 统一桌面模型；
- 候选融合；
- Target Resolution；
- 动作路径选择；
- 可靠性；
- Snapshot；
- Execute Gate；
- 验证；
- 安全和接管；
- Stop Fence 协作；
- 输入租约（Input Lease）闭合。

Provider 负责平台事实和动作。

## 2. Unified Desktop Model

### Display

- stable id；
- name；
- logical geometry；
- physical size/pixels；
- scale；
- transform/rotation；
- primary；
- capture mapping。

### Workspace

- id；
- name/index；
- output relation；
- active/visible；
- platform semantics。

Workspace 不能假设所有平台都是二维虚拟桌面。

### Window

- provider identity；
- application identity；
- title；
- process/app metadata；
- workspace/output；
- logical geometry；
- focus；
- visibility/interactability；
- state；
- generation/version。

### Accessible Element

- stable-within-snapshot id；
- provider ref；
- role；
- name/description/value；
- states；
- actions；
- parent/children；
- window ref；
- geometry with coordinate space；
- reliability。

### Capture Frame

- frame id；
- source display/window/region；
- pixel dimensions；
- transform to logical space；
- timestamp；
- content hash；
- privacy redactions；
- object handle。

### Visual Candidate

- candidate id；
- bounding region；
- label/description；
- interactability probability；
- model/provider；
- relation to semantic elements；
- confidence。

### Snapshot

Snapshot 是同一时间窗口内融合后的桌面观察：

- desktop generation；
- windows；
- selected semantic tree；
- capture frames；
- visual candidates；
- coordinate transforms；
- candidate numbers；
- expiration policy。

## 3. Provider Profile

- Scene；
- Semantic；
- Capture；
- Input；
- Event；
- Visual Grounding；
- Application Adapter。

运行时可组合多个 Provider。一个 Provider 不必实现全部能力。

## 4. 能力探测

不得仅根据桌面名称判断能力。

能力档案至少描述：

- window enumerate/focus/move/resize；
- workspace enumerate/activate；
- semantic tree/action/value；
- display/window/region capture；
- keyboard/pointer/scroll/touch；
- absolute/relative input；
- desktop/application events；
- persistent permission；
- multi-display；
- fractional scale；
- reliability and limitations。

桌面名称只用于选择专有增强 Provider。

## 5. 可靠性等级

建议从高到低：

- R5：应用专用 API 已确认；
- R4：原生语义动作/状态已确认；
- R3：语义目标 + 可靠几何；
- R2：规则/快捷键；
- R1：视觉 Grounding；
- R0：未验证坐标猜测。

### R0 与风险等级的定位

- R0 仍可作为可靠性事实记录：表示未验证坐标猜测、证据最弱；
- 是否允许 R0 或任何低可靠性路径执行副作用，由 Policy 与 verification 配置决定；
- 不得写“R0 默认禁止用于副作用”；
- 不得把高风险动作“默认阻断”或“低视觉默认禁止”写进本规范；
- 风险等级与可靠性等级是判定输入，不是偷偷默认化的 deny 矩阵；
- Policy 可匹配规则要求更高可靠性或更强验证；无匹配规则时遵循 Freedom-first（allow）。

## 6. Target Resolver

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

模型可以参与相关性排序，但不能伪造平台事实。

## 7. 动作选择

顺序：

1. 应用专用 API；
2. Accessibility Action；
3. Window/Workspace Action；
4. 已知快捷键；
5. Semantic Geometry + Input；
6. Visual Candidate + Input；
7. 停止并请求用户。

选择必须考虑：

- side-effect class；
- reliability；
- provider availability；
- current focus；
- protected surfaces（按 Policy）；
- user supervision mode；
- verification availability；
- Execute Gate 结果。

## 8. Observation-Action-Verification

### Observe

建立 Snapshot，不假设旧状态有效。

### Resolve

选择目标和候选替代项。

### Prepare

- 聚焦正确窗口；
- 切换工作区/viewport；
- 等待动画和布局稳定；
- 确认没有遮挡；
- 获取输入锁 / Input Lease。

### Reobserve after Prepare

Prepare 可能改变焦点、布局、viewport 或遮挡关系。Prepare 完成后必须 reobserve 相关目标，再进入 Execute Gate 的最终检查。禁止把 Prepare 前的坐标/generation 直接当作成熟执行依据。

### Execute Gate

执行副作用前必须通过 Execute Gate。Gate 至少检查：

1. **Snapshot / window generation**：目标绑定的 Snapshot 与 window generation 仍有效，未过期；
2. **Provider ref**：目标仍可通过 provider ref 或等价稳定引用定位，不是裸旧坐标；
3. **可见且可交互**：目标 visible/interactable；遮挡、最小化、off-screen、锁屏等导致失败；
4. **Target / resource**：动作目标与 resource scope 一致；
5. **Policy**：当前 Actor/Entry/Task/Action 的 Policy 判定为 allow，或已满足 confirm/lease 条件；
6. **Approval / Lease**：若 Policy 要求 confirmation 或存在 Input Lease / permission lease，则 lease 未过期且绑定正确 task/action/resource；
7. **Stop Fence / 控制权**：不在用户接管、paused、unavailable、紧急停止或 Stop Fence 闭合之后的禁止输入状态；
8. **Protected Surface 规则**：按 Policy 决定是否允许对该表面观察/输入/云发送。

任一检查失败：

- 不得执行输入或改变目标状态的动作；
- 返回结构化错误（常见 `stale_state`、`permission_denied`、`approval_required`、`target_not_found`、`lease_expired` 等）；
- 可触发重新观察、换路径、请求用户或失败，取决于错误语义与 Task 策略。

### Execute

通过 Gate 后执行最可靠动作。

### Observe Successor

重新获取相关状态，而不是仅相信 Provider 返回成功。

### Verify

根据 Task 成功标准判断：

- 元素状态变化；
- 窗口出现/消失；
- 文件/文档变化；
- 应用反馈；
- 视觉差异；
- 外部系统确认。

验证失败可：重新观察、换路径、回滚、请求用户或失败。

### stale_state

在以下情况必须视为过期并返回 `stale_state`（或先 reobserve 再判定）：

- Snapshot / desktop generation 过期；
- window generation 变化；
- Prepare 后未 reobserve 却仍用旧证据；
- 工作区/viewport、DPI/scale、capture source 变化导致坐标空间失效；
- provider ref 失效且无法安全重定位；
- Input Lease 期间目标窗口已切换到不兼容上下文。

禁止直接使用旧图 x/y 或过期 generation 执行。

## 9. 坐标体系

必须明确：

- physical pixel；
- logical display；
- compositor/global（若存在）；
- capture frame；
- window-local；
- content/client area；
- semantic provider coordinates。

任何矩形必须带 coordinate space 和 transform version。

禁止假设：

```text
AT-SPI rect == screenshot pixels == input coordinates
```

旧坐标在以下情况后失效：

- 工作区/viewport 变化；
- 窗口移动、缩放、动画；
- DPI/scale 变化；
- Capture Source 改变；
- 应用布局变化；
- 新 Snapshot generation。

## 10. Operation Snapshot

面向用户的操作快照包含：

- 图像；
- 目标窗口/应用；
- 当前任务意图；
- 相关候选编号；
- 简短语义；
- 可靠性和风险；
- 建议动作；
- Snapshot id/generation；
- 过期提示。

### 编号策略

默认只标 3-12 个任务相关候选。

模式：

- focused：任务相关；
- explore：当前区域主要元素；
- debug：尽量完整，仅开发者使用。

标注应避免重叠，并对低置信度候选做不同提示。

### 用户回复“点 7”

流程：

1. 解析 Snapshot 和 candidate；
2. 检查授权入口与 Task；
3. 重新观察目标窗口；
4. 通过 provider ref/语义重新定位；
5. 若变化过大，生成新 Snapshot；
6. 重新评估 Policy / Lease；
7. 通过 Execute Gate 后执行与验证。

禁止直接使用旧图 x/y。

## 11. 用户图和模型图分离

用户可获得完整清晰标注图。

模型优先获得：

- 结构化元素；
- 候选摘要；
- 局部裁剪；
- 必要截图。

这使远程可视化不等同于每一步都消耗大图 Token。

是否将某帧发送云模型，由 Policy 与数据/隐私策略决定（见 Protected Surface）。

## 12. Protected Surfaces

Protected Surface 不是写死的“认证界面绝不上云”硬禁清单 alone，而是由 Policy 决定对特定表面的：

- 观察（observe/capture）；
- 输入（input/action）；
- 云发送（send frame/content to cloud model or remote channel）。

系统应识别并标记常见敏感表面，作为 Policy 输入，例如：

- Agent 宿主/调试控制台；
- Companion 主控制界面；
- 权限审批窗口；
- 系统认证/密码界面；
- Secret 管理界面；
- 紧急停止控件。

规则：

- 识别与标记必须存在，便于规则匹配与审计；
- 默认行为由 Policy 配置；本规范不保留“认证界面绝不上云”的不可配置硬禁作为唯一安全模型；
- 实现可提供安全的出厂推荐规则，但推荐规则不是规范级硬编码 deny；
- 无匹配规则时 Freedom-first：allow；匹配规则可 deny/confirm/require_local 等；
- Computer Use 不得从受保护表面提取 Secret 到普通日志或未授权通道；这属于 Secret/数据策略，与“是否允许观察存在”分开。

## 13. 输入控制权与 Input Lease

状态：

- agent_control；
- user_control；
- waiting_user；
- paused；
- unavailable。

### Input Lease

输入类动作应绑定 Input Lease：

- lease id；
- task / action 绑定；
- actor / entry point；
- 目标 scope（session/window/resource）；
- 超时与最大动作次数（按配置）；
- 创建时 Snapshot/generation 提示；
- 撤销条件。

Lease 闭合条件：

- 正常完成并 release；
- 超时；
- 用户接管；
- Stop Fence / 紧急停止；
- 锁屏 / session unavailable；
- Policy 撤销；
- Task 取消或失败恢复。

闭合后不得继续发送输入；恢复时必须重新观察并重新获取 lease（若仍需要）。

### 用户接管

用户接管时：

- Agent 输入调用必须取消或完成当前原子单位；
- Kernel 将输入锁转移给用户；
- Agent 不再发送输入；
- Input Lease 闭合或挂起为不可用；
- 恢复时重新观察，并通过 Execute Gate。

### 锁屏与 session unavailable

屏幕锁定或会话不可用后：

- 输入 lease 默认失效；
- 新的输入动作不得执行；
- 状态进入 unavailable 或等价；
- 解锁后必须 reobserve，不得沿用锁前坐标。

## 14. Stop Fence

Computer Use 必须服从全局 Stop Fence / Emergency Stop：

- 停止后不得发起新的输入或 privileged desktop 副作用；
- 进行中的可取消输入应取消；不可安全中断的动作标记恢复待查；
- 停止不依赖模型合作；
- Fence 解除前，Execute Gate 必须失败；
- 与用户接管、lease 闭合、审计记录一致。

Stop Fence 的权威状态在 Kernel；Provider 只执行取消/停止指令。

## 15. Linux

### 通用层

- AT-SPI；
- XDG Portal；
- PipeWire；
- 可组合 Input；
- 通用桌面事件；
- X11 兼容 Provider（可选）。

### Hyprland Scene Provider

负责：

- outputs；
- workspaces；
- clients/windows；
- focus；
- special workspace；
- layout/animation stability；
- events；
- window activation and placement。

不重复实现 AT-SPI 和视觉模型。

### niri Scene Provider

负责：

- outputs/workspaces/windows；
- scrolling layout/viewport；
- visible/partially-visible/interactable；
- focus causing layout movement；
- dynamic capture target integration（可用时）；
- event-driven stabilization。

niri 操作前必须 `make target interactable -> wait -> reobserve`。该 reobserve 是 Execute Gate 的前置事实来源。

### Generic Wayland

缺少专用 Scene Provider 时，可以提供语义、截图和输入的部分能力，但必须报告窗口管理限制，不得假装完整支持。

## 16. Windows

Provider 可使用：

- UI Automation；
- Win32/DWM window facts；
- Windows capture APIs；
- semantic action and input；
- UAC/privileged path 与普通 Computer Use 分离。

高完整性窗口可能拒绝普通输入，必须返回 permission/integrity 错误。

## 17. macOS

Provider 可使用：

- Accessibility/AX；
- ScreenCaptureKit；
- 原生窗口和事件接口；
- CGEvent 等输入；
- 系统 Accessibility/Screen Recording 权限状态。

Spaces 和后台窗口限制必须作为能力事实返回。

## 18. Application Adapter

应用专用 Adapter 可优先使用：

- Browser CDP/DOM；
- 编辑器插件；
- 办公软件 API；
- 专业应用脚本接口。

它仍通过 Computer Use/Capability SDK 接入，不得绕过 Task 和权限。

## 19. Visual Grounding

OmniParser 等只产生候选，不直接执行输入。

视觉候选必须与语义元素去重，并携带模型版本和置信度。

视觉无法保证找全，因此目标是找到当前任务需要的元素，而不是枚举所有可点击区域。

低视觉置信度是可靠性与 verification 输入；是否允许执行由 Policy 与配置决定，不得规范级默认阻断。

## 20. 远程监督模式

- automatic：在 Policy 与 Execute Gate 允许时自动执行，关键节点汇报；
- approval：当 Policy 或用户配置要求时，在执行前发送 Snapshot 等待确认；
- cooperative：用户通过编号和方向逐步指挥；
- observe-only：只截图/状态，不输入。

“approval 模式”是监督配置，不是“高风险默认阻断”矩阵。高风险/低置信度是否需要 Snapshot 确认，由 Policy 与 verification 配置决定。

## 21. 降级

示例：

- Semantic Action 不可用 -> 快捷键或语义几何；
- Window Capture 不可用 -> Display Capture + crop；
- Scene 不完整 -> 用户手动聚焦或兼容模式；
- Visual 不可用 -> 仅语义；
- Input 不可用 -> observe-only；
- Verification 不可用 -> 按 Policy 请求确认、换路径或继续（若规则允许）；不得在本规范写死“拒绝高风险动作”。

降级路径仍须通过 Execute Gate。

## 22. Provider 定位

现成组件可作为 Provider 或参考：

- computer-use-linux：Linux 语义、窗口、截图与输入；
- pi-computer-use：Windows/macOS 和状态式操作参考；
- OmniParser：视觉候选；
- Set-of-Mark：标注方法。

核心不得被任一组件的数据模型绑定。

## 23. 与 Policy / Kernel 的关系

- Computer Use 不自建平行权限系统；
- 所有副作用仍走 `Task -> Policy -> agentd -> Broker/Extension -> system mechanism -> verify/audit`；
- Provider 成功不等于目标成功；
- 风险等级、可靠性等级、Protected Surface 标签都是判定输入；
- Freedom-first：无匹配规则时 allow；confirm/deny 只来自匹配规则、系统机制或恢复需要。
