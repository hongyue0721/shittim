# Extension SDK

> 状态：仅规范，未实现。Extension SDK 的唯一事实源是 [`specs/EXTENSION_SDK.md`](../../specs/EXTENSION_SDK.md)。本文不定义新 API。

## Extension SDK Base 与 Optional Profile

Extension SDK Base 指通用 Extension/Capability 协议与生命周期；它是必做基础能力，但当前仍没有可发布 SDK。旧草案中的“SDK Core”仅指本层，本文统一使用 **Extension SDK Base**，避免与 Shittim Core 混淆。未来 Optional Profile 在 Extension SDK Base 之上声明并组合特定能力，按条件生效。Computer Use 只作为未来可选 Extension Profile，不是 Shittim Core 能力；当前没有 Computer Use Schema、生成包、Profile composition contract 或 Provider。

## 设计边界

- Extension 默认进程外，不加载进 `agentd` 地址空间；
- Extension 只能通过公开 Extension protocol 被 Kernel 调用；
- Extension 不能直接互调、不能直接调用 Privilege Broker、不能写 Task/Policy/Audit Kernel 事实；
- Native Extension 的权限声明必须区分 OS-enforced、host-enforced 与 declaration-only；Extension SDK Base 的 enforcement class 仍是真实约束，Profile 不得把 declaration-only 伪装成已执行的权限；
- Extension RPC 与 KCP 是不同协议，不能共享 Envelope 或混用方法目录；
- Provider 返回或外形类似 Event 的 JSON 不能升级为 Kernel Event。

## 未来生成物

实现阶段将依据正式 Schema 生成 Extension SDK Base 产物：

- Manifest 和 Capability 类型；
- handshake/invoke/cancel/progress/health/error/event 类型；
- Rust/TypeScript/Python SDK 类型与 validator；
- conformance fixtures 与兼容测试。

这些 Extension SDK Base 生成物目前均不存在；Computer Use Profile 的 Schema、生成包、composition 和 Provider 也均不存在。依赖方不得导入虚构包名，也不得假设已有运行时或真实 Provider。

## 实现者阅读顺序

1. [`AGENT.md`](../../AGENT.md)
2. [`specs/EXTENSION_SDK.md`](../../specs/EXTENSION_SDK.md)
3. [`specs/IMPLEMENTATION_CONTRACTS.md`](../../specs/IMPLEMENTATION_CONTRACTS.md)
4. [`specs/SECURITY_PRIVILEGE.md`](../../specs/SECURITY_PRIVILEGE.md)
5. [`specs/CONFORMANCE.md`](../../specs/CONFORMANCE.md)

## 与 KCP 的关系

KCP 服务于 Kernel 客户端；Extension RPC 服务于 Kernel 与 Extension/Provider 进程。KCP 当前首批八个方法不包含 Extension invoke；Extension RPC 的“JSON-RPC 风格”描述不意味着 KCP 使用 JSON-RPC。
