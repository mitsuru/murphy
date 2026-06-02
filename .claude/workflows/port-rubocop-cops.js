/**
 * port-rubocop-cops — generic RuboCop cop porting workflow
 *
 * Usage:
 *   Workflow({ name: "port-rubocop-cops" })
 *     → auto-fetches bd ready --label=cop -n 10
 *
 *   Workflow({ name: "port-rubocop-cops", args: ["murphy-ipxn", "murphy-ttzm", "murphy-fgcu"] })
 *     → processes the specified issues only
 *
 *   Workflow({ name: "port-rubocop-cops", args: { n: 6 } })
 *     → auto-fetches top N ready cop issues (object form)
 *
 *   Workflow({ name: "port-rubocop-cops", args: 6 })
 *     → same as above (bare number also accepted)
 */

export const meta = {
  name: 'port-rubocop-cops',
  description: 'Port/gap-fill RuboCop cops: auto-group by crate, implement, draft PR, Gemini review',
  phases: [
    { title: 'Discover', detail: 'Read issue details and group by crate (max 2 per agent)' },
    { title: 'Implement', detail: 'Per-group agents in worktrees: implement → PR → Gemini → resolve' },
    { title: 'Summary', detail: 'Collect results' },
  ],
}

// ─── Schemas ────────────────────────────────────────────────────────────────

const ISSUE_DETAIL_SCHEMA = {
  type: 'object',
  properties: {
    id:        { type: 'string' },
    title:     { type: 'string' },
    crate:     { type: 'string', enum: ['murphy-std', 'murphy-rspec', 'murphy-rails', 'murphy-rspec-rails', 'unknown'] },
    is_new_cop: { type: 'boolean' },
  },
  required: ['id', 'title', 'crate', 'is_new_cop'],
}

const RESULT_SCHEMA = {
  type: 'object',
  properties: {
    cop_ids:                { type: 'array', items: { type: 'string' } },
    success:                { type: 'boolean' },
    pr_url:                 { type: 'string' },
    summary:                { type: 'string' },
    issues_closed:          { type: 'array', items: { type: 'string' } },
    gemini_threads_resolved: { type: 'number' },
    failure_reason:         { type: 'string' },
  },
  required: ['cop_ids', 'success', 'summary', 'issues_closed'],
}

// ─── Common agent instructions ───────────────────────────────────────────────

const SETUP = `
## Worktree setup (run first — single Bash call)
mise trust && eval "$(mise activate bash)"
Prefix every cargo/ruby call with: eval "$(mise activate bash)" &&

## Important references — read these BEFORE touching cx.rs
- .claude/rules/token-api.md        — SourceTokenKind variants, sorted_tokens, token_before/after/in,
                                       block-opener pattern, heredoc ranges. Saves reading 6000-line cx.rs.
- .claude/rules/autocorrect-pattern.md — surgical emit_edit (two non-overlapping edits vs whole-node)
- .claude/rules/cop-options-hand-rolled.md — hand-rolled CopOptions error contract

## port-rubocop-cop phases (for each issue)
Phase 1: Run \`bd show <id>\` to get description, design, and file path.
         WebFetch the RuboCop source URL from the description.
Phase 2: Implement with TDD — failing test first, then code.
         For NEW cops: create crates/<pack>/src/cops/<ns>/<name>.rs,
         add pub mod / pub use to <ns>/mod.rs,
         add to register_cops! in lib.rs (murphy-std uses trailing commas).
Phase 3: Gap analysis — diff implementation vs Phase 1 spec list.
Phase 4: Document ABI blockers in murphy-parity notes (never bypass the single-surface boundary).
Phase 5: Invoke Skill tool with skill="roborev-refine" after pushing.
Phase 6: gh pr create --draft

## Quality gates (all must pass before Phase 5)
eval "$(mise activate bash)" && cargo test -p <CRATE>
eval "$(mise activate bash)" && cargo clippy -p <CRATE> --all-targets -- -D warnings
eval "$(mise activate bash)" && cargo +nightly fmt --check

## PR title convention
- Gap fill : "fix(<crate>-<id>): close parity gap in Pack/CopName"
- New cop  : "feat(<crate>-<id>): port Pack/CopName from RuboCop"
When multiple issues share one PR, list all IDs: "fix(murphy-std): ..."

## After draft PR — handle Gemini Code Assist review
\`\`\`bash
PR_NUM=$(gh pr view --json number -q '.number')
REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner')
OWNER=\${REPO%%/*}; REPO_NAME=\${REPO##*/}
for i in $(seq 1 30); do
  COUNT=$(gh api graphql -f query='query($o:String!,$r:String!,$p:Int!){repository(owner:$o,name:$r){pullRequest(number:$p){reviewThreads(first:20){nodes{isResolved comments(first:1){nodes{author{login}}}}}}}}' \\
    -f o=\$OWNER -f r=\$REPO_NAME -F p=\$PR_NUM \\
    --jq '[.data.repository.pullRequest.reviewThreads.nodes[]|select(.comments.nodes[0].author.login=="gemini-code-assist")|select(.isResolved==false)]|length' 2>/dev/null || echo 0)
  [ "\$COUNT" -gt 0 ] && echo "Gemini: \$COUNT threads" && break
  echo "Waiting Gemini attempt \$i/30..."; sleep 30
done
\`\`\`
Fetch thread details, triage (Fix/Dismiss), implement/reply in Japanese, resolve each thread:
\`\`\`bash
gh api "repos/\$REPO/pulls/comments/<COMMENT_ID>/replies" -f body="<日本語で返信>"
gh api graphql -f query='mutation($id:ID!){resolveReviewThread(input:{threadId:$id}){thread{isResolved}}}' -f id="<THREAD_NODE_ID>"
\`\`\`

## Issue tracking
bd update <id> --claim   # before writing code
bd close <id> --reason="implemented: <summary>"
`

// ─── Phase 0: Discover issues ────────────────────────────────────────────────

phase('Discover')

// Resolve n from args. Default is 30 (not 10) so bare invocation still fetches plenty.
const rawN = typeof args === 'number' ? args
  : typeof args === 'string' ? parseInt(args, 10)
  : (args && typeof args === 'object' ? parseInt(args.n, 10) : 0)
const n = rawN > 0 ? rawN : 30
log(`[debug] args=${JSON.stringify(args)} rawN=${rawN} n=${n}`)

// Fetch in batches of 10 — one agent per slice so each only returns ~10 IDs (reliable).
// Slice i covers lines (i*10+1)..(i+1)*10 via tail -n +START | head -10.
const BATCH_SIZE = 10
const numBatches = Math.ceil(n / BATCH_SIZE)
const SLICE_SCHEMA = {
  type: 'object',
  properties: { ids: { type: 'array', items: { type: 'string' } } },
  required: ['ids'],
}
const batchResults = await parallel(
  Array.from({ length: numBatches }, (_, i) => {
    const take = (i + 1) * BATCH_SIZE
    const skip = i * BATCH_SIZE
    return () => agent(
      `Run this bash command exactly and return the output lines as the "ids" array:
       bd ready --label=cop -n ${take} | awk '/^[○◐]/ {print $2}' | tail -n +${skip + 1} | head -${BATCH_SIZE}
       Each line is a murphy-* issue ID. Expected ~${BATCH_SIZE} items (lines ${skip + 1}–${take}).`,
      { label: `fetch-${i}`, schema: SLICE_SCHEMA }
    )
  })
)
const issueIds = [...new Set(batchResults.filter(Boolean).flatMap(b => b.ids))]

log(`Processing ${issueIds.length} issues: ${issueIds.join(', ')}`)

// Read issues in batches of 5 to avoid overwhelming bd's Dolt DB with 30 concurrent reads.
const DISCOVER_BATCH = 5
const discoverBatches = []
for (let i = 0; i < issueIds.length; i += DISCOVER_BATCH) {
  discoverBatches.push(issueIds.slice(i, i + DISCOVER_BATCH))
}

const MULTI_DETAIL_SCHEMA = {
  type: 'object',
  properties: {
    issues: { type: 'array', items: ISSUE_DETAIL_SCHEMA },
  },
  required: ['issues'],
}

const batchDetails = await parallel(
  discoverBatches.map((batch, i) => () => agent(
    `For each issue ID below, run "bd show <id>" and return its details.
     IDs: ${batch.join(' ')}

     For each issue determine:
     - id: the issue ID
     - title: the issue title
     - crate: "murphy-rails" if title contains Rails/ or file is in crates/murphy-rails
              "murphy-rspec" if title contains RSpec/ or file is in crates/murphy-rspec
              "murphy-std"   for Lint/*/Style/*/Layout/* or crates/murphy-std
              "unknown"      if can't determine
     - is_new_cop: true if this is a new cop port, false if gap fill

     Return all ${batch.length} issues in the "issues" array.`,
    { label: `discover-batch-${i}`, schema: MULTI_DETAIL_SCHEMA }
  ))
)

const resolved = batchDetails.filter(Boolean).flatMap(b => b.issues)
log(`Resolved ${resolved.length} issues`)

// ─── Group by crate, max 2 per agent ─────────────────────────────────────────

function groupByCrate(issues, maxPerGroup = 2) {
  const byCreate = {}
  for (const issue of issues) {
    const c = issue.crate === 'unknown' ? 'murphy-std' : issue.crate
    if (!byCreate[c]) byCreate[c] = []
    byCreate[c].push(issue)
  }
  const groups = []
  for (const [crate, list] of Object.entries(byCreate)) {
    for (let i = 0; i < list.length; i += maxPerGroup) {
      groups.push({ crate, issues: list.slice(i, i + maxPerGroup) })
    }
  }
  return groups
}

const groups = groupByCrate(resolved, 4)
log(`${groups.length} groups: ${groups.map(g => `${g.crate}(${g.issues.map(i => i.id).join('+')})`).join(', ')}`)

// ─── Phase 1: Implement all groups in parallel ───────────────────────────────

phase('Implement')

const results = await parallel(
  groups.map(({ crate, issues }) => () => agent(
    `You are implementing ${issues.length} Murphy cop issue(s) in the ${crate} crate.

${SETUP}

## Your issues: ${issues.map(i => i.id).join(', ')}
${issues.map(i => `  - ${i.id}: ${i.title} (${i.is_new_cop ? 'new cop port' : 'gap fill'})`).join('\n')}

## Steps
1. For each issue, run \`bd show <id>\` to get the full description and design
2. Claim all issues: ${issues.map(i => `bd update ${i.id} --claim`).join(' && ')}
3. Follow port-rubocop-cop phases (Phase 1–6 above) for each issue
4. cargo test -p ${crate} && cargo clippy -p ${crate} --all-targets -- -D warnings && cargo +nightly fmt --check
5. Commit and push
6. Create draft PR
7. Invoke skill="roborev-refine"
8. Poll for Gemini review, handle threads, resolve
9. Close all issues: bd close ${issues.map(i => i.id).join(' ')}

Return structured result.`,
    {
      label: `impl:${crate}:${issues.map(i => i.id).join('+')}`,
      phase: 'Implement',
      isolation: 'worktree',
      schema: RESULT_SCHEMA,
    }
  ))
)

// ─── Summary ──────────────────────────────────────────────────────────────────

phase('Summary')

const succeeded = results.filter(Boolean).filter(r => r.success)
const failed    = results.filter(Boolean).filter(r => !r.success)
const allPRs    = results.filter(Boolean).map(r => r.pr_url).filter(Boolean)
const allClosed = results.filter(Boolean).flatMap(r => r.issues_closed || [])
const gemini    = results.filter(Boolean).reduce((s, r) => s + (r.gemini_threads_resolved || 0), 0)

log(`${succeeded.length}/${groups.length} groups succeeded, ${failed.length} failed`)
log(`PRs: ${allPRs.join(', ')}`)
log(`Issues closed: ${allClosed.join(', ')}`)
log(`Gemini threads resolved: ${gemini}`)
failed.forEach(r => r && log(`FAILED ${r.cop_ids?.join('+')}: ${r.failure_reason || '?'}`))

return { succeeded: succeeded.length, failed: failed.length, prs: allPRs, closed: allClosed, geminiResolved: gemini }
