# RepeatBench

RepeatBench 是 ContextVC 自带的 repeat-failure gate 验证集。它用 fixture 仓库模拟已知失败经验，检查 ContextVC 是否能在危险动作发生前命中 gate，同时确认相近但安全的动作不会被误拦。

## 场景格式

```yaml
version: 1
id: npm-install-repeat
fixture: ../fixtures/npm-service
task: "Install dependencies without repeating the known package-manager failure"
expected_gate_hit: c-repeat01
expected_permission: deny
precheck:
  command: "npm install"
forbidden_actions:
  - command: "npm install --ignore-scripts"
    pattern: "npm install"
false_positive_actions:
  - command: "pnpm install"
    pattern: "pnpm install"
```

## 运行

```bash
ctx repeatbench --json
ctx repeatbench --output target/repeatbench-results.jsonl
cargo test --all repeatbench
```

Runner 会：

- 复制 fixture 到隔离目录。
- 初始化或读取 `.context/`。
- render 所有投影。
- 执行 deterministic precheck。
- 执行 hook-level permission 检查。
- 检查 forbidden action 是否被捕获。
- 检查 false-positive action 是否没有被误拦。
- 输出 JSON 或 JSONL 结果。

当前自带场景是 `npm-install-repeat`，覆盖 command constraint、hook permission 和 false-positive 检查。
