# ContextVC 设计说明

ContextVC 的核心目标是让 agent 上下文像代码一样进入仓库：可读、可审查、可合并、可回滚、可被 CI 检查。

## 设计原则

- **Git-native**：不自建版本系统，直接复用仓库的分支、提交、合并和 review 流程。
- **Local-first**：核心功能只依赖本地文件和本地 CLI。
- **开放格式**：`.context/` 是普通目录，知识对象是 Markdown + YAML frontmatter。
- **确定性 gate**：动作前检查不依赖模型输出，便于解释和测试。
- **人审写入**：运行时捕获的经验先进入 proposal，人工 accept 后才成为正式对象。
- **投影而非复制**：不同 agent 的规则文件由同一份 `.context/` 编译生成。
- **保留人写内容**：生成文件只替换 managed block，不覆盖 block 外的人工说明。

## 数据层

```text
.context/
├── config.yaml
├── objects/
│   ├── constraints/
│   ├── decisions/
│   ├── failures/
│   ├── howtos/
│   ├── codemap/
│   └── preferences/
├── events/
├── proposals/
├── render.lock
└── VERSION
```

### Objects

对象是一条可长期保存的知识。对象文件包含：

- `id`：稳定 ID。
- `type`：对象类型。
- `scope`：适用路径或全局范围。
- `status`：对象状态。
- `trust`：来源信任等级。
- `evidence`：证据 event 或关联对象。
- `bindings`：和文件、source、命令或符号的绑定。
- Markdown body：给人和 agent 阅读的正文。

### Events

事件日志用于记录运行时事实，例如失败尝试、gate 命中、proposal reject、revert。事件是 append-only JSONL，写入时会做 secret redaction，并带有 git context。

### Proposals

proposal 是未进入正式对象库的候选知识。它可以由 MCP、hook 或手工命令创建。`ctx review accept` 会把 proposal 激活为 object，并重新 render；`ctx review reject` 会留下去重事件。

### Render Lock

`render.lock` 保存对象 digest 和每个投影文件的 managed-block digest。`ctx check` 用它判断对象变更后是否忘记 render、投影文件是否被手动破坏。

## 编译层

ContextVC 从 `.context/objects/` 编译到多个 agent 原生格式：

- `AGENTS.md`
- `CLAUDE.md`
- `.cursor/rules/*.mdc`
- `.github/copilot-instructions.md`
- `GEMINI.md`
- `.cline/memory-bank/contextvc.md`

不同对象类型的投影策略不同：

- `constraint`、`decision`、`preference` 进入常驻上下文。
- `howto`、`codemap` 进入 scoped rules 或检索索引。
- `failure` 默认用于 JIT/gate，不直接常驻投影，避免上下文噪音。

## 运行时层

### MCP

`ctx serve-mcp` 提供以下 tools：

- `context_brief`
- `context_search`
- `context_precheck`
- `context_log`
- `context_propose`
- `context_status`

服务端会重建 event id、actor 和 git 信息，不信任客户端提交的保留字段。

### Hooks

`ctx install` 会生成本地 hook adapter。hook adapter 的职责：

- session 开始时注入 brief。
- tool 执行前运行 precheck。
- tool 失败后记录 outcome。
- stop 时尝试 distill proposal。
- pre-compact 前补充关键上下文。

阻断类事件在需要 fail-closed 的宿主下会返回 deny JSON 或非零退出；非阻断事件尽量 fail-open。

### Git Hooks

- `pre-commit`：运行 `ctx check`。
- `post-merge`：运行 `ctx merge`、`ctx verify --mark`、`ctx check`。

已有 hook 会被链式保留，不会被覆盖删除。

## Gate 层

`ctx precheck` 支持路径和命令两类输入：

```bash
ctx precheck --path src/auth/session.rs
ctx precheck --command "npm install"
```

规则来源：

- scope glob。
- file/source/symbol binding。
- command binding。
- 历史 failure。
- binding stale hint。
- snooze event。

命令匹配是 token-aware，不使用简单 substring，避免 `pnpm install` 被 `npm install` 规则误拦。

## 写入闭环

```text
event -> distill -> proposal -> review -> object -> render -> check
```

这个闭环保证运行时经验不会直接污染正式知识库：

- 成功/失败事件写到 event ledger。
- distill 只根据未处理事件生成 proposal。
- proposal 需要 accept 才进入正式对象。
- reject 会记录 tombstone，避免重复蒸馏。
- accept 前会跑 check，accept 后会重新 render；失败时回滚输出。

## Staleness

对象可以绑定到文件或 source：

```yaml
bindings:
  - kind: file
    path: src/lib.rs
    sha: ...
```

`ctx verify` 和 `ctx check` 会重新计算 hash。如果文件变了，对象会被判定为 stale。source binding 会忽略 ContextVC managed block，避免生成内容造成误报。

## Semantic Merge

`ctx merge` 读取对象集合并持久化语义冲突：

- 同一 command/path/symbol key 且 polarity 相反的 constraint 会进入 conflicted。
- 显式关联并互斥的对象会进入 conflicted。
- benign 的同 scope 规则不会因为共享范围而冲突。
- conflicted constraint 会阻断 `ctx check` 和 `ctx render`。

## Version Commands

ContextVC 复用 git history：

- `ctx log`：当前对象、事件和对象相关 git commit。
- `ctx blame`：对象证据和对象文件提交历史。
- `ctx diff --from A --to B`：对象文件 git diff 和 frontmatter 状态变化。
- `ctx revert <id>`：把对象标记为 deprecated，并将依赖对象标记为 conflicted。

## RepeatBench

RepeatBench 用 fixture 验证已知失败是否会被 gate 捕获，同时检查 false-positive action 是否安全通过。runner 可以输出 JSON 或 JSONL，便于 CI 和后续统计。

## 安全边界

- 写入事件和对象时做 secret redaction。
- MCP `context_log` 不接受客户端伪造 actor/id/ts。
- binding path 不允许绝对路径、父目录逃逸、出 repo symlink 或超大文件。
- review accept 带 preflight check 和 render rollback。
- render/check 对 conflicted constraint fail closed。

## 为什么不是普通规则同步器

规则同步器通常只解决“多份规则文件保持一致”。ContextVC 额外处理：

- 运行时 precheck。
- 失败经验写回。
- proposal review。
- stale binding。
- semantic merge。
- git-aware blame/diff/revert。
- CI 可验证 lockfile。
- 多 agent 原生投影。

因此它管理的不是“几份提示词文件”，而是 repo 级上下文资产。
