# Extension SDK

> 状态：仅规范，未实现。Extension SDK 的唯一事实源是 [`specs/EXTENSION_SDK.md`](../../specs/EXTENSION_SDK.md)。本文不定义新 API。

## Extension SDK Base 与 Optional Profile

Extension SDK Base 指通用 Extension/Capability 协议与生命周期；它是必做基础能力，但当前仍没有可发布 SDK。旧草案中的“SDK Core”仅指本层，本文统一使用 **Extension SDK Base**，避免与 Shittim Core 混淆。未来 Optional Profile 在 Extension SDK Base 之上声明并组合特定能力，按条件生效。Computer Use 只作为未来可选 Extension Profile，不是 Shittim Core 能力；当前没有 Computer Use Schema、生成包、Profile composition contract 或 Provider。

## 设计边界

- Extension 默认进程外，不加载进 `agentd` 地址空间；
- Extension 只能通过公开 Extension protocol 被 Kernel 调用；
- Extension不能直接互调、不能直接调用Privilege Broker、不能写Task/Policy/Audit Kernel事实，也不能直接创建Child Task；Child Task唯一入口是Core的`kernel.task/task.child.create` Action；
- Native Extension 的权限声明必须区分 OS-enforced、host-enforced 与 declaration-only；Extension SDK Base 的 enforcement class 仍是真实约束，Profile 不得把 declaration-only 伪装成已执行的权限；
- Extension RPC 与 KCP 是不同协议，不能共享 Envelope 或混用方法目录；
- Profile/Extension只消费Core注入的PermissionDecision/Approval v2引用；不得判断material fingerprint等价、复用/失效Approval或签发Lease；

## 未来生成物

实现阶段将依据正式 Schema 生成 Extension SDK Base 产物：

- Manifest 和 Capability 类型；
- handshake/invoke/cancel/progress/health/error/event 类型；
- Rust/TypeScript/Python SDK 类型与 validator；
- conformance fixtures 与兼容测试。

PermissionDecision v2的material/observation fingerprint与Approval current-head facts属于Core公共合同。SDK只传递Kernel注入的opaque refs/fingerprints，不导出“isMaterialEquivalent”“approveImplicitly”或直接ChildTask create等越权helper。

## 实现者阅读顺序

1. [`AGENT.md`](../../AGENT.md)
2. [`specs/EXTENSION_SDK.md`](../../specs/EXTENSION_SDK.md)
3. [`specs/IMPLEMENTATION_CONTRACTS.md`](../../specs/IMPLEMENTATION_CONTRACTS.md)
4. [`specs/SECURITY_PRIVILEGE.md`](../../specs/SECURITY_PRIVILEGE.md)
5. [`specs/CONFORMANCE.md`](../../specs/CONFORMANCE.md)

## 与 KCP 的关系

KCP 服务于 Kernel 客户端；Extension RPC 服务于 Kernel 与 Extension/Provider 进程。KCP 当前首批八个方法不包含 Extension invoke；Extension RPC 的“JSON-RPC 风格”描述不意味着 KCP 使用 JSON-RPC。
