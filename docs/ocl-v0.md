# Open Context Layer (OCL) v0

> ContextVC canonical format for `.context/` directories.

## Version

The `VERSION` file in `.context/` MUST contain `0`.

## Directory Layout

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

High-frequency artifacts (`.context/.cache/index.sqlite`, session transcripts) live under `.cache/` and MUST NOT be committed.

The JSON Schema for object frontmatter is published at
`docs/schema/ocl-object-v0.schema.json`; `ctx check` also applies semantic
validation for type, status, trust, binding kind, and enforcement values.

## Knowledge Object

Each object is a Markdown file with YAML frontmatter:

```yaml
---
id: f-8f21
type: failure
title: "Short title"
scope: ["src/auth/**"]
status: active
trust: agent_verified
confidence: 0.9
evidence: [ev_01J9XQ]
bindings: []
created: created-by-review
verified: verified-by-review
supersedes: null
---
```

### Types

| type | gate | projection |
| --- | --- | --- |
| constraint | block/ask/warn | resident |
| decision | warn | resident/scoped |
| failure | warn | JIT only |
| howto | - | scoped |
| codemap | - | scoped/JIT |
| preference | - | resident (limited) |

### Status

`proposed | active | conflicted | stale | deprecated`

## Events

Append-only JSONL shards under `events/`. The CLI chooses shard names for safe local appends and git-friendly merges.

## Render Lock

`render.lock` records object digests and per-target file digests for drift detection.

## Projection Targets

The default compiler targets are:

- `AGENTS.md`
- `CLAUDE.md`
- `.cursor/rules/*.mdc`
- `.github/copilot-instructions.md`
- `GEMINI.md`
- `.cline/memory-bank/contextvc.md`

All projection targets use ContextVC managed blocks and preserve human-authored
content outside those blocks.

## Merge Semantics

- One object per file; hash-based IDs reduce collisions.
- Event shards merge with union semantics (git merge=union).
- Logical conflicts surface as `status: conflicted`.
- A conflicted `constraint` blocks `ctx check` and `ctx render`.
- `ctx merge` persists semantic conflicts after git merges; the installed
  post-merge hook runs `ctx merge`, `ctx verify --mark`, and `ctx check`.
