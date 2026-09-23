# Runtime packages

`cpu/`、`vulkan/` 和 `cuda/` 分别保存经过版本锁定和哈希校验的 llama.cpp 运行时。三套运行时均固定为 build 10964 / commit `b29c606e2`。二进制不提交版本库，每个目录的 `runtime-manifest.json` 记录官方压缩包来源、大小和 SHA-256。
