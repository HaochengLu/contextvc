---
id: c-repeat01
type: constraint
title: Use pnpm instead of npm install
scope:
  - "**"
status: active
trust: human
confidence: 1.0
evidence: []
bindings:
  - kind: command
    pattern: "npm install"
    enforcement: block
created: init
verified: review
---

Do not run `npm install` in this repo. Use `pnpm install` so the lockfile remains consistent.
