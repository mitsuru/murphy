/**
 * cop-port-batch — stream-port RuboCop cops to an acceptance branch, one final PR
 *
 * Usage:
 *   Workflow({ name: "cop-port-batch" })
 *     → filters bd ready for "Port Style/" (default)
 *
 *   Workflow({ name: "cop-port-batch", args: "Port Lint/" })
 *     → filters by "Port Lint/"
 *
 *   Workflow({ name: "cop-port-batch", args: "Port Layout/" })
 *     → filters by "Port Layout/"
 *
 * Strategy:
 *   pipeline(tasks, implement_and_integrate)
 *   Each agent: port-rubocop-cop phases 1–5 (merge_strategy: deferred)
 *               → rebase onto acceptance → push HEAD:acceptance (retry loop)
 *               → cleanup remote branch
 *   No separate Acceptor. Concurrent rebase-push contends only on the atomic
 *   ref update; the retry loop absorbs rejections. acceptance history is linear.
 *   Final: cargo test --workspace gate on acceptance, then one draft PR.
 */

export const meta = {
  name: 'cop-port-batch',
  description: 'Stream-port RuboCop cops: bd ready → TDD + roborev → rebase-push to acceptance branch → one final PR',
  phases: [
    { title: 'Discover', detail: 'Fetch matching tasks from bd ready' },
    { title: 'Setup',    detail: 'Create acceptance branch from main' },
    { title: 'Implement', detail: 'Implement + roborev + rebase-push per cop (streaming)' },
    { title: 'Gate',     detail: 'Full cargo test --workspace + clippy + fmt on acceptance' },
    { title: 'PR',       detail: 'Create final draft PR: acceptance → main' },
  ],
}

// ── Args ──────────────────────────────────────────────────────────────────────

// Filter string passed to grep against bd ready output.
const filter = (typeof args === 'string' && args.trim()) ? args.trim() : 'Port Style/'

// ── Schemas ───────────────────────────────────────────────────────────────────

const TASK_LIST_SCHEMA = {
  type: 'object',
  properties: {
    tasks: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          id:    { type: 'string' },
          title: { type: 'string' },
        },
        required: ['id', 'title'],
      },
    },
  },
  required: ['tasks'],
}

const SETUP_SCHEMA = {
  type: 'object',
  properties: {
    branch: { type: 'string' },
    pr_url: { type: 'string' },
  },
  required: ['branch', 'pr_url'],
}

const IMPL_SCHEMA = {
  type: 'object',
  properties: {
    id:             { type: 'string' },
    success:        { type: 'boolean' },
    cop_name:       { type: 'string' },
    skipped:        { type: 'boolean' },
    blocker_note:   { type: 'string' },
    failure_reason: { type: 'string' },
  },
  required: ['id', 'success'],
}

const GATE_SCHEMA = {
  type: 'object',
  properties: {
    passed:         { type: 'boolean' },
    failure_output: { type: 'string' },
  },
  required: ['passed'],
}

// ── Phase: Discover ───────────────────────────────────────────────────────────

phase('Discover')

const discovered = await agent(
  `Fetch cop port tasks from bd ready and filter by pattern.

Run exactly:
  bd ready -n 400 2>&1 | grep "${filter}"

Extract each output line: the issue ID (e.g. murphy-2wl8) and the full title text.
Return as "tasks" array with { id, title } objects.
If the command returns no output, return { tasks: [] }.`,
  { label: 'discover', phase: 'Discover', schema: TASK_LIST_SCHEMA }
)

const tasks = discovered?.tasks ?? []
log(`Found ${tasks.length} tasks matching "${filter}"`)

if (tasks.length === 0) {
  log('No tasks found — nothing to do.')
  return { merged: 0, failed: 0, skipped: 0, tasks: [] }
}

// ── Phase: Setup ──────────────────────────────────────────────────────────────

phase('Setup')

const filterLabel = filter.replace(/\/$/, '').replace(/^Port /, '')

const setup = await agent(
  `Create an acceptance branch for this batch of cop ports, then open a draft PR immediately.

Steps:
1. eval "$(mise activate bash)"
2. git checkout main && git pull --rebase origin main
3. Build a branch name:
     SLUG=$(echo "${filter}" | tr '[:upper:]' '[:lower:]' | tr '/ ' '--' | sed 's/-\\+/-/g; s/-$//')
     DATE=$(date +%Y%m%d)
     BRANCH="cop-port-batch-\${SLUG}-\${DATE}"
4. git checkout -b "\$BRANCH"
5. Add an empty commit so GitHub allows PR creation:
     git commit --allow-empty -m "chore: open acceptance branch for ${filterLabel} cop batch"
6. git push -u origin "\$BRANCH"
7. Create a draft PR:
     gh pr create --draft \\
       --base main \\
       --head "\$BRANCH" \\
       --title "feat(murphy-std): batch port ${filterLabel} cops" \\
       --body "## Status: in progress

Streaming cop ports accumulate here via \`cop-port-batch\` orchestrator.
Filter: \`${filter}\`

This PR will be updated with the full cop list and marked ready once all implementations complete and the quality gate passes."
8. Return { branch: "\$BRANCH", pr_url: "<the https://github.com/... URL from gh pr create output>" }`,
  { label: 'setup', phase: 'Setup', schema: SETUP_SCHEMA }
)

const acceptanceBranch = setup?.branch
const prUrl = setup?.pr_url
if (!acceptanceBranch) {
  log('ERROR: failed to create acceptance branch — aborting.')
  return { error: 'acceptance branch setup failed' }
}
log(`Acceptance branch: ${acceptanceBranch}`)
log(`Draft PR: ${prUrl}`)

// ── Phase: Implement (streaming pipeline) ────────────────────────────────────

phase('Implement')

const WORKTREE_SETUP = `
## Worktree setup (run first — single Bash call)
mise trust && eval "$(mise activate bash)"
Prefix every cargo/ruby call with: eval "$(mise activate bash)" &&

## Key references (read before touching cx.rs)
- .claude/rules/token-api.md            — SourceTokenKind variants, token_before/after/in
- .claude/rules/autocorrect-pattern.md  — surgical emit_edit (two edits vs whole-node)
- .claude/rules/cop-options-hand-rolled.md — hand-rolled CopOptions error contract
`

const results = await pipeline(
  tasks,
  task => agent(
    `Implement and integrate the Murphy cop for beads issue ${task.id}: "${task.title}"
${WORKTREE_SETUP}
## Steps

### 1. Claim and read
bd update ${task.id} --claim
bd show ${task.id}

### 2. Implement (port-rubocop-cop phases 1–5, merge_strategy: deferred)
Invoke the Skill tool with skill="port-rubocop-cop" and args="merge_strategy: deferred".
This runs phases 1–5: read RuboCop source → TDD implementation → gap analysis → escalation check → roborev-refine.
Do NOT create a PR or merge (Phase 6 is skipped by merge_strategy: deferred).

If Phase 4 escalation is required (ABI gap that cannot be worked around within
the single-surface boundary):
  - Record the blocker: bd update ${task.id} --notes="BLOCKER: <escalation reason>"
  - Return { id: "${task.id}", success: false, skipped: true, blocker_note: "<reason>" }
  - Stop here — do not proceed to quality gates or integration.

### 3. Quality gates (must all pass before integration)
eval "$(mise activate bash)" && cargo test -p murphy-std
eval "$(mise activate bash)" && cargo clippy -p murphy-std --all-targets -- -D warnings
eval "$(mise activate bash)" && cargo +nightly fmt --check

### 4. Integrate: rebase-push onto acceptance branch
COP_BRANCH=\$(git rev-parse --abbrev-ref HEAD)

git fetch origin

# Rebase onto acceptance (cops touch only their own new file — no conflicts expected)
git rebase origin/${acceptanceBranch}

# Push with retry loop (up to 5 attempts; another agent may have pushed between fetch and push)
for i in 1 2 3 4 5; do
  git push origin HEAD:${acceptanceBranch} && break
  if [ \$i -eq 5 ]; then
    echo "ERROR: push failed after 5 attempts"
    exit 1
  fi
  echo "Push attempt \$i rejected — re-fetching and rebasing..."
  git fetch origin
  git rebase origin/${acceptanceBranch}
done

### 5. Cleanup
# Capture paths before leaving the worktree
WT=\$(git rev-parse --show-toplevel)
MAIN_REPO=\$(git worktree list --porcelain | awk '/^worktree / {print substr(\$0, 10); exit}')

# Remove the remote cop branch (keeps origin tidy)
git push origin --delete "\$COP_BRANCH" || true

# Close the beads issue
bd close ${task.id} --reason="cop ported and integrated into ${acceptanceBranch}"

# Remove local worktree — must cd out first (cannot remove from inside)
cd "\$MAIN_REPO"
git worktree remove "\$WT" || git worktree remove --force "\$WT"
git branch -D "\$COP_BRANCH" 2>/dev/null || true
git worktree prune

Return { id: "${task.id}", success: true, cop_name: "<Pack/CopName e.g. Style/Alias>" }`,
    {
      label: `impl:${task.id}`,
      phase: 'Implement',
      isolation: 'worktree',
      schema: IMPL_SCHEMA,
    }
  )
)

// ── Tally results ─────────────────────────────────────────────────────────────

const merged  = results.filter(Boolean).filter(r =>  r.success)
const notOk   = results.filter(Boolean).filter(r => !r.success)
const skipped = notOk.filter(r => r.skipped)
const errored = notOk.filter(r => !r.skipped)

log(`Integrated: ${merged.length} | Skipped (escalation): ${skipped.length} | Errored: ${errored.length}`)
if (skipped.length > 0) {
  skipped.forEach(r => log(`  SKIP ${r.id}: ${r.blocker_note ?? '(no note)'}`))
}
if (errored.length > 0) {
  errored.forEach(r => log(`  ERR  ${r.id}: ${r.failure_reason ?? '(no reason)'}`))
}

if (merged.length === 0) {
  log('Nothing integrated — skipping gate and PR.')
  return {
    merged: 0, skipped: skipped.length, errored: errored.length,
    acceptanceBranch,
    skippedTasks: skipped.map(r => ({ id: r.id, reason: r.blocker_note })),
    erroredTasks: errored.map(r => ({ id: r.id, reason: r.failure_reason })),
  }
}

// ── Phase: Gate ───────────────────────────────────────────────────────────────

phase('Gate')

const gate = await agent(
  `Run the full quality gate on the integrated acceptance branch "${acceptanceBranch}".

Steps:
1. git fetch origin
2. git checkout ${acceptanceBranch} && git pull origin ${acceptanceBranch}
3. eval "$(mise activate bash)" && cargo test --workspace 2>&1
4. eval "$(mise activate bash)" && cargo clippy --workspace --all-targets -- -D warnings 2>&1
5. eval "$(mise activate bash)" && cargo +nightly fmt --check 2>&1

Return { passed: true } if all three commands exit 0.
Return { passed: false, failure_output: "<trimmed relevant output>" } if any fail.`,
  { label: 'gate', phase: 'Gate', schema: GATE_SCHEMA }
)

if (!gate?.passed) {
  log(`GATE FAILED: ${gate?.failure_output ?? '(no output)'}`)
  log(`PR remains draft at: ${prUrl}`)
  log('Fix failures on acceptance branch, then re-run or manually ready the PR.')
  return {
    merged: merged.length, skipped: skipped.length, errored: errored.length,
    gate: 'FAILED',
    gateOutput: gate?.failure_output,
    acceptanceBranch,
    pr: prUrl,
    skippedTasks: skipped.map(r => ({ id: r.id, reason: r.blocker_note })),
    erroredTasks: errored.map(r => ({ id: r.id, reason: r.failure_reason })),
  }
}

log('Gate passed.')

// ── Phase: PR ─────────────────────────────────────────────────────────────────

phase('PR')

const copList  = merged.map(r => `- ${r.cop_name ?? r.id}`).join('\n')
const skipList = skipped.length > 0
  ? skipped.map(r => `- ${r.id}: ${r.blocker_note ?? '(escalation)'}`).join('\n')
  : 'None'

await agent(
  `Update and ready the draft PR for the completed cop port batch.

PR: ${prUrl}
Branch: ${acceptanceBranch} → main

Steps:
1. git push origin ${acceptanceBranch}

2. Update the PR title and body:
gh pr edit "${prUrl}" \\
  --title "feat(murphy-std): batch port ${merged.length} ${filterLabel} cops" \\
  --body "## Summary

Batch-ported ${merged.length} RuboCop cops to \`murphy-std\` via the \`cop-port-batch\` orchestrator.
Filter: \`${filter}\`

## Integrated cops (${merged.length})
${copList}

## Skipped — Phase-4 escalations (${skipped.length})
${skipList}

## Test plan
- [x] Each cop implemented TDD (failing test first, then code)
- [x] \`roborev-refine\` passed per cop
- [x] \`cargo test --workspace\` green on acceptance branch
- [x] \`clippy\` + \`fmt\` clean on acceptance branch
"

3. Mark the PR ready for review:
gh pr ready "${prUrl}"`,
  { label: 'finalize-pr', phase: 'PR' }
)

log(`PR finalized: ${prUrl}`)

return {
  merged: merged.length,
  skipped: skipped.length,
  errored: errored.length,
  gate: 'PASSED',
  acceptanceBranch,
  pr: prUrl,
  skippedTasks: skipped.map(r => ({ id: r.id, reason: r.blocker_note })),
  erroredTasks: errored.map(r => ({ id: r.id, reason: r.failure_reason })),
}
