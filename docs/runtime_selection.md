# CUDA / Vulkan / CPU 自动探测与降级

## 桌面端接口

`ilia-inference::RuntimeManager` 是桌面端唯一需要依赖的运行时入口：

- `RuntimeManager::probe`：只读探测，返回可序列化的 `RuntimeProbeReport`；
- `RuntimeManager::start`：按策略启动，失败时继续尝试下一后端，返回 `AutoManagedLlamaServer`；
- `AutoManagedLlamaServer::report`：返回最终后端、参数和每次启动尝试；
- 对象销毁时自动终止并回收 `llama-server` 子进程。

探测命令有 10 秒超时，不会因损坏的显卡驱动无限阻塞桌面端。实际启动仍需要通过 `/health` 健康检查。

## 选择顺序

| 用户选择 | 尝试顺序 |
| --- | --- |
| 自动 | CUDA → Vulkan → CPU |
| CUDA | CUDA → Vulkan → CPU |
| Vulkan | Vulkan → CPU |
| CPU | CPU |

探测成功不代表模型一定能加载。例如显存已被其他程序占用时，CUDA 探测可能成功但模型启动失败；`RuntimeManager::start` 会记录错误并继续降级。

## 默认运行参数

| 后端 | 上下文 | GPU层 | 设备 | Flash Attention |
| --- | ---: | ---: | --- | --- |
| CUDA | 16,384 | 99 | 推荐 CUDA 独显 | 开 |
| Vulkan | 8,192 | 99 | 优先 NVIDIA/AMD，其次 Intel Arc，最后核显 | 开 |
| CPU | 4,096 | 0 | `none`，强制禁用GPU | 关 |

Vulkan 设备不会单纯按报告的共享内存大小选择；这可避免 Intel 核显因报告较大的共享内存而覆盖实际更快的独显。

## 诊断命令

```powershell
cargo run --release -p ilia-inference --bin ilia-runtime -- `
  --runtime-root runtime `
  --backend auto
```

输出 JSON，包括三套运行时是否存在、设备名称、显存、推荐设备、降级顺序和最终推荐后端。桌面端可以直接把同一结构展示在“模型与硬件设置”页面。

## 已验证设备

当前机器探测结果：

- CUDA：`CUDA0`，NVIDIA GeForce RTX 4070 Laptop GPU；
- Vulkan：`Vulkan0` Intel UHD Graphics 770、`Vulkan1` RTX 4070，推荐 `Vulkan1`；
- CPU：无GPU设备，使用 `--device none`。

三条路径均已实际加载 Qwen3-4B Q4_K_M 并完成带引证问答。首次 Vulkan 推理可能因着色器缓存建立而明显较慢。
