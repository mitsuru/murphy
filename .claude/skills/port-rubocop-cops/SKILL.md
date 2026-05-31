---
name: port-rubocop-cops
description: >-
  Use when the user wants to implement multiple RuboCop cop gap-fills or new
  cop ports in parallel. Kicks off the port-rubocop-cops workflow which
  auto-groups issues by crate, spawns one agent per group in isolated
  worktrees, and handles the full cycle: implement → draft PR → Gemini review
  → resolve threads. Trigger phrases: "cop を実装して", "gap fill して",
  "port-rubocop-cops を実行", "cop workflow を走らせて", or any request to
  work on multiple cop issues at once.
---

# port-rubocop-cops skill

Launches the `port-rubocop-cops` workflow. The workflow:

1. **Discovers** issues (from `args` or `bd ready --label=cop -n 10`)
2. **Groups** them by crate, max 2 per agent
3. **Implements** each group in an isolated worktree (port-rubocop-cop phases)
4. **Opens draft PRs**, runs roborev-refine, handles Gemini review
5. **Returns** PR URLs and closed issue list

## How to invoke

Call `Workflow` with the appropriate args:

### Auto-fetch ready cop issues (default 10)
```javascript
Workflow({ name: "port-rubocop-cops" })
```

### Specify issue count
```javascript
Workflow({ name: "port-rubocop-cops", args: { n: 6 } })
```

### Specify exact issue IDs
```javascript
Workflow({ name: "port-rubocop-cops", args: ["murphy-ipxn", "murphy-ttzm", "murphy-fgcu"] })
```

## When the user types `/port-rubocop-cops [args]`

1. Parse any issue IDs or count from the command args
2. Call `Workflow(...)` with the appropriate form above
3. When the workflow completes, present the PR URLs and ask if the user wants to:
   - Review and merge the PRs
   - Run cleanup (`post-merge-cleanup` for each merged PR)

## Args parsing

| User input | Workflow call |
|---|---|
| `/port-rubocop-cops` | `Workflow({ name: "port-rubocop-cops" })` |
| `/port-rubocop-cops 6` | `Workflow({ name: "port-rubocop-cops", args: { n: 6 } })` |
| `/port-rubocop-cops murphy-ipxn murphy-ttzm` | `Workflow({ name: "port-rubocop-cops", args: ["murphy-ipxn", "murphy-ttzm"] })` |

## Notes

- Each group agent reads `bd show <id>` itself — no hardcoded descriptions needed
- Token API reference: `.claude/rules/token-api.md` is passed to all agents
- After workflow: PRs are in draft; run `gh pr ready + merge` separately or ask the user
