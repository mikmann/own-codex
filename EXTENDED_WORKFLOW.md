# Extended workflow

`codex-extended-workflow` extends the official Codex CLI without introducing a
second agent runtime or a repository-specific ticket format. The fork is based
on the official `rust-v0.147.0` tag at
`be6e8eac029b183056b7e4402879f15d2c85f61b`. The requested upstream spelling
`v0.147.0` was absent and the use of `rust-v0.147.0` was explicitly approved.
The distribution and local release tag use `v0.147.1-alpha-4`.

## Architecture

The implementation reuses Codex's existing configuration layers, prompt
context, MultiAgentV2 runtime, plan mechanism, session rollouts, UI events, and
model capability resolver:

- `codex-rs/config/src/extended_workflow.rs` defines and validates
  `ExtendedWorkflowToml`, `WorkflowProcessToml`, and per-agent profiles.
- `codex-rs/core/src/extended_workflow.rs::classify_request`, `plan_turn`, and
  `plan_reviewers` provide deterministic, unit-tested classification and
  dynamic role selection. This is local planning; it does not spend model
  tokens on a separate classifier call.
- `codex-rs/core/src/context/extended_workflow.rs` emits one bounded policy
  fragment and a compact per-turn JSON plan through the existing contextual
  user-message/world-state pipeline. The policy tells the main agent to use the
  existing plan mechanism and to implement only the selected ticket.
- `codex-rs/core/src/tools/handlers/extended_workflow.rs` exposes
  `plan_workflow_reviewers` and `validate_workflow_result`. The latter enforces
  result size, semantic contracts, relevance, evidence, contradictions, exact
  equality with the completed child thread's terminal JSON, and exactly one
  targeted correction attempt. Attempt, revision, digest, reviewer-pass, and
  blocker state are stored outside model context in
  `state/migrations/0047_extended_workflow_validations.sql`, keyed by root
  thread and turn. No receipt sidecar or synthetic function output is created.
- `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs` validates workflow
  identity against the server-owned per-turn selection ledger, rejects
  unselected/duplicate agents, requires `fork_turns = "none"`, and applies a
  read-only ceiling after role and explicit profile model overrides.
  `codex-rs/core/src/session/mod.rs` reapplies the persisted ceiling at the
  common session-construction boundary; `agent/control/spawn.rs` additionally
  preserves it, the selected model, provider, and reasoning effort across cold
  reload/resume.
- `codex-rs/core/src/tools/handlers/multi_agents_v2/close_agent.rs` adds the
  native V2 `close_agent` lifecycle operation. It accepts a live canonical task
  name or thread id, rejects root/self targets, shuts the target subtree down
  deepest-first, releases concurrency slots, and treats a repeated close by
  thread id as an idempotent no-op. `agent/control/lifecycle.rs` and the agent
  graph store persist `Open -> Closing -> Closed` only on the target's incoming
  edge; descendant edge states are preserved so an authorized legacy resume can
  still reopen the correct subtree. Full ancestor checks and resume-time
  reconciliation prevent publication below a closing path. The handler refuses
  to close a current-turn workflow agent until its result is accepted or
  terminally rejected. A non-pass reviewer stays open for its one post-fix
  review cycle; a passing reviewer or an exhausted second cycle can be closed.
- When no thread limit is explicitly configured, the extended launcher derives
  the MultiAgentV2 limit from the larger enabled expert/reviewer maximum plus
  the root thread. With the built-in profiles this means five total threads and
  four simultaneous child slots. An explicit user limit remains authoritative;
  `close_agent` releases occupied child slots for subsequent spawns.
- `codex-rs/protocol/src/protocol.rs` persists the workflow identity and
  read-only flag plus an optional profile context ceiling in backward-compatible
  thread-spawn metadata. This keeps UI identity, security constraints, and the
  context ceiling available across resume without adding a second persistence
  system.
- `codex-rs/tui/src/multi_agents.rs` renders name, type, role, model, reasoning,
  and textual status. Color is supplementary and follows terminal capability
  and `NO_COLOR` through `supports_color`.

All expert and reviewer agents are read-only. The main agent remains the only
writer and owns assessment, corrections, implementation, and integration.
Nested agents, callable MCP tools, dynamic/hosted/extension tools, apply-patch,
permission escalation, plugin installation, and app instructions are disabled
in those children. They retain only sandboxed read-only shell access, MCP
resource list/template/read operations, and local image inspection when the
existing feature and environment allow it.

The expert process is advisory. Its selected subagents provide independent
specialist analyses, alternatives, risks, evidence, and recommendations that
help the main agent create concepts, make decisions, or create tickets. Experts
do not make the final decision and do not author the final concept or ticket.
The main agent evaluates and integrates all validated contributions and remains
accountable for every resulting artifact.

The reviewer process is an independent quality gate after implementation. Its
selected subagents inspect the actual changes made by the main agent for their
purpose and necessity, alignment with the root request and accepted ticket, and
functional and technical correctness. Reviewers verify the repository diff and
relevant tests instead of trusting the main agent's change summary. They work
read-only and report findings; only the main agent assesses findings, changes
the workspace, and reruns affected tests.

## Classification and selection

`classify_request` maps each root request to `simple_answer`, `work_order`, or
`concept` using bounded text input and explicit German/English signals.
Implementation/change signals win over concept wording; concept-only requests
remain concepts. Risk, material uncertainty, detected domains, scope signals,
and request size produce an inspectable complexity score and reasons.

- A routine `simple_answer` selects no experts.
- A simple answer can select experts when it crosses domains, contains material
  uncertainty, or has elevated/high risk.
- A `work_order` or `concept` selects between the configured minimum and
  maximum according to complexity, affected domains, risk, and decision scope.
- Reviewer selection happens only after implementation and uses the factual
  change summary. Roles are unique within the round and matched to the changes.
- Short German/English continuation commands such as `Bitte mach weiter`,
  `Please continue`, and `Go ahead` remain work orders instead of silently
  bypassing the workflow in a follow-up turn.

The turn plan, selected profiles, count, estimates, budget decision, and
classifier reasons are visible to the main model as structured JSON and are
covered by tests in `codex-rs/core/src/extended_workflow.rs` and
`codex-rs/core/tests/suite/extended_workflow.rs`.

## Configuration

No special repository file is required when the repository is opened through
the `codex-extended-workflow` launcher; its lowest-priority product default
enables the workflow. The smallest explicit, repository-persisted setup is
`<repository>/.codex/config.toml`. The file applies only to that repository.
`AGENTS.md` and `.codex/agents/` are not required by this architecture and are
not created by the preparation command. Use those existing Codex conventions
only when a repository independently needs persistent instructions or reusable
agent-role files.

No repository-specific thread-limit entry is needed for the built-in maximum of
four simultaneous experts or reviewers. To choose a different child capacity,
set `agents.max_concurrent_threads_per_session`; this setting counts child
threads, while the internal MultiAgentV2 total also includes the root thread.

Example:

```toml
[extended_workflow]
enabled = true
audit_mode = false
workflow_budget_tokens = 16000
max_result_bytes = 8192

[extended_workflow.experts]
enabled = true
min = 2
max = 4
round_budget_tokens = 8000

[[extended_workflow.experts.profiles]]
name = "expert_1"
role = "Architecture"
model = "gpt-5.6-sol"
reasoning_effort = "high"
token_budget = 2400
context_budget = 12000

[[extended_workflow.experts.profiles]]
name = "expert_2"
role = "Orchestration and context"
model = "gpt-5.6-terra"
reasoning_effort = "xhigh"
token_budget = 2200

[[extended_workflow.experts.profiles]]
name = "expert_3"
role = "Security and permissions"
model = "gpt-5.5"
reasoning_effort = "high"

[[extended_workflow.experts.profiles]]
name = "expert_4"
role = "Testing and compatibility"
model = "gpt-5.6-sol"
reasoning_effort = "max"

[extended_workflow.reviewers]
enabled = true
min = 2
max = 2
round_budget_tokens = 6000

[[extended_workflow.reviewers.profiles]]
name = "reviewer_1"
role = "Correctness and architecture"
model = "gpt-5.6-sol"
reasoning_effort = "high"
token_budget = 2400
context_budget = 12000

[[extended_workflow.reviewers.profiles]]
name = "reviewer_2"
role = "Security and permissions"
model = "gpt-5.6-terra"
reasoning_effort = "xhigh"
token_budget = 2200
context_budget = 12000
```

For a one-off invocation, the four dedicated global arguments override the
repository values without changing the file:

```shell
codex-extended-workflow --workflow-token-limit 16000 --expert-min 2 --expert-max 4 --reviewer-min 2 --reviewer-max 4
```

Either process, or both processes together, can be disabled for one invocation:

```shell
codex-extended-workflow --disable-experts
codex-extended-workflow --disable-reviewers
codex-extended-workflow --disable-experts --disable-reviewers
```

For a persistent repository-local choice, set `enabled = false` in the
corresponding process table:

```toml
[extended_workflow.experts]
enabled = false

[extended_workflow.reviewers]
enabled = false
```

The same arguments can be supplied to `resume` and other runtime commands.
Existing dotted `-c` overrides remain supported. The built-in defaults are
experts `min = 2`, `max = 4` and reviewers `min = 2`, `max = 4`.

Every expert and reviewer profile independently selects its `model` and
`reasoning_effort`, so profiles in the same round may use different models and
quality levels. The model names above are examples, not an allowlist. Known
quality values are `none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`,
and `ultra`; future non-empty values advertised by a model are also accepted by
the configuration parser. At spawn time the existing model catalog and
capability resolver in `apply_requested_spawn_agent_model_overrides` validates
that the model is available and that it supports the selected quality level.
Unsupported combinations fail before the child starts, with the same explicit
error used by native `spawn_agent`.

Defaults for experts and reviewers are independently `enabled = true`,
`min = 2`, `max = 4`. Each process enforces
`0 <= min <= max <= 6`; equal bounds, including `0/0` and `6/6`, are valid.
Each process accepts at most six profiles, and profile string fields are
bounded to 256 UTF-8 bytes so the model-visible plan remains predictably small.
`enabled = true, min = 0, max = 0` means that classification and workflow
guidance remain active but that process starts no automatic subagents.
`enabled = false` starts none regardless of its bounds. Both processes may be
disabled at once, and `[extended_workflow] enabled = false` disables the entire
extension.

When no configuration source sets the top-level `enabled` value, the primary
`codex-extended-workflow` launcher supplies `enabled = true` as its product
default. The compatibility `codex` launcher and direct library consumers
default the extension to disabled. This launcher default is deliberately below
every persisted or CLI configuration layer, so an explicit `enabled = false`
always wins. Repository preparation writes `enabled = true` explicitly.

The effective precedence follows the existing Codex layer stack:

1. explicit CLI overrides such as
   `-c 'extended_workflow.experts.min=1'`;
2. a trusted repository's `.codex/config.toml`;
3. global Codex configuration;
4. launcher/internal defaults (`true` for `codex-extended-workflow`, otherwise
   `false`; process defaults remain `2..=4`).

Repository configuration is subject to Codex's existing project-trust rules;
the fork does not bypass them.

## Contracts and one correction

The native validator deserializes results into these contracts from
`codex-rs/core/src/extended_workflow.rs`:

- `ExpertResult`: subject, summary, findings, recommendations, file/symbol
  evidence, and uncertainties/open questions.
- `WorkflowTicket`: title, goal, context, requirements, acceptance criteria,
  dependencies, tests/validation, and risks/open questions.
- `WorkflowConcept`: title, objective, context, decisions, alternatives,
  risks/open questions, and optional validated tickets.
- `ReviewerResult`: subject, verdict, summary, concrete findings, evidence, and
  missing tests. Every finding contains severity dimensions, rationale, file,
  symbol, recommended correction, and missing tests.

An initial invalid result returns exact validation errors and
`retry_allowed = true`. Only one targeted follow-up is allowed. A second invalid
submission is terminal and cannot be replaced by an unbounded new attempt.
Expert/reviewer retry identity is bound server-side to the assignment turn,
assignment SHA-256 digest, target `ThreadId`, terminal output revision, and
terminal output digest. The caller-supplied object must equal the completed
child thread's exact terminal JSON, and the subject is checked against the
server-persisted task, not a free caller assertion. Accepted and exhausted state
is read from the bounded SQLite validation ledger on resume; raw result bodies
are not duplicated and ledger rows are never injected into model context.
Before a ticket can complete, every accepted reviewer result is compared with
the reviewer's latest persisted terminal output revision and digest. A newer
reviewer output invalidates the earlier pass and requires validation of that
new output, so stale approval cannot satisfy the completion gate.

## Reproducible reviewer severity

Severity is computed by `severity_for_dimensions`, not accepted as an
unexplained label. Reviewers provide three dimensions:

- impact: `local = 0`, `material = 2`, `system_wide = 3`;
- likelihood: `unlikely = 0`, `plausible = 1`, `expected = 3`;
- recoverability: `easy = 0`, `costly = 1`, `irreversible = 3`.

The sum maps to `Low` (0–2), `Medium` (3–4), `High` (5–6), or `Critical`
(7–9). A mismatch rejects the review result. Every reviewer finding is rendered
as a yellow warning event, including `Critical`, while the severity text remains
explicit. Every event also prints the severity text, file, symbol, rationale,
correction, and missing tests, so color is never the only signal.

## Token and context budgets

The provider's model catalog remains authoritative for the maximum context
window and supported output capabilities. This fork does not claim to impose a
provider-side hard output-token cap when the API/runtime cannot do so.

- `context_budget` is a per-profile hard local context-window ceiling. The
  runtime applies the smaller of this value, an existing global override, and
  the selected model's provider/catalog maximum. The ceiling is persisted with
  workflow identity and restored after resume or residency reload. Before a
  selected subagent starts, the runtime also compares the ceiling with a
  conservative byte-based estimate of the canonical assignment alone. An
  assignment that cannot fit is rejected visibly; the estimate is not
  presented as an exact provider token count.
- `workflow_budget_tokens` is an enforced cumulative provider-token stop shared
  by the main agent, experts, reviewers, and compactions in one workflow.
  Provider `total_tokens` are counted literally, including cached input and
  reasoning/output usage. `0` means unlimited. The CLI equivalent is
  `--workflow-token-limit <TOKENS>`; without it the repository, then global
  configuration is used, otherwise the workflow is unlimited.
- The base limit and current usage are persisted in the selected Alpha-3 SQLite
  state database and restored by `resume`. A successful workflow completion
  closes the workflow and starts the next independent request at zero while the
  session total continues. `/workflow-reset-budget` explicitly abandons the
  current workflow and resets only its counter.
- At a non-zero ceiling the TUI offers another 100%, 75%, 50%, or 25% of the
  original base limit, or Stop. An extension raises the ceiling and never
  clears already used tokens. Non-interactive `exec` cannot ask and stops. Since
  provider usage is known only after a response completes, concurrent responses
  already in flight can overshoot a ceiling.
- Profile `token_budget` and `round_budget_tokens` remain conservative advisory
  planning values because the shared provider runtime does not expose an
  independent hard per-child output-token stop for this path. The overall
  `workflow_budget_tokens` limit is not divided into round estimates and does
  not reduce the configured expert or reviewer count before actual usage
  reaches the persisted ceiling.
- The planner estimates selected subagents and refuses to start a
  round when the configured minimum cannot fit; it reports `budget_blocked`
  instead of silently reducing quality.
- Input tokens, output tokens, and the main-agent share are reported as not
  independently hard-enforced by this layer.
- `max_result_bytes` defaults to 8 KiB and has a hard configurable ceiling of
  9,000 UTF-8 bytes. The same 9,000-byte ceiling applies to each individual
  model-visible workflow context fragment; the one-retry rule is also locally
  hard-enforced.
- Workflow experts/reviewers are hard-required to use `fork_turns = "none"`.
  Compact task contracts, bounded result objects, and summaries limit context
  growth; requests for full or partial parent history are rejected before
  spawning.

On Windows, an enabled extended workflow defaults shell tools to non-login
PowerShell, which adds `-NoProfile` and avoids profile failures under
Constrained Language Mode. An explicit top-level `allow_login_shell = true`
still opts back into login-shell behavior. The workflow policy also requires
CLM-compatible commands and direct native patch tooling instead of PowerShell
pipelines or batch wrappers around `apply_patch`.

No time- or token-saving percentage is claimed: this repository has no stable
provider-token benchmark baseline. Comparisons must use actual provider usage
for the same model, prompt, repository, and cache state.

## Prompt, receipt, session, and resume files

The upstream code does not generate dedicated workflow receipt files.
`codex-rs/rollout/src/recorder.rs::RolloutRecorder` stores canonical session
events as JSONL under the Codex home so sessions can be listed and resumed.
Those files are required session history and are left untouched.

The `codex-extended-workflow` launcher defaults to its own Codex home:
`~/.codex-extended-workflow`. This separates local credentials, user
configuration, sessions, logs, and the SQLite database from the compatibility
`codex` launcher's normal `~/.codex` home. There is no automatic migration,
copy, or fallback from Alpha-2 or normal Codex data. Consequently, an Alpha-2
session is not visible to an isolated Alpha-3 invocation.

Use `codex-extended-workflow --shared-state ...` to opt into the normal Codex
state, configuration, sessions, logs, and SQLite database for one invocation.
Credentials and MCP OAuth tokens remain in `~/.codex-extended-workflow`, so a
shared-state invocation may require a separate extended-workflow login. This
avoids concurrent normal/extended processes rotating or truncating the same
credential files. For example:

```shell
codex-extended-workflow --shared-state resume <session-id>
```

Alternatively, `CODEX_HOME` selects an explicit home. Combining `CODEX_HOME`
with `--shared-state` is rejected as ambiguous. Resume an explicit-home session
with the same `CODEX_HOME` value. Explicit `CODEX_SQLITE_HOME`,
`sqlite_home`, and `log_dir` overrides retain their upstream meaning and may
place those individual stores outside the selected Codex home. Repository
`.codex/config.toml` files and remote account/cloud data are not copied; they
remain shared when the same repository or account is used.

The separate default home is an accidental-sharing boundary, not a security
boundary against another process running as the same local user. Alpha-3 does
not attempt to prevent symlink, junction, reparse-point, or active TOCTOU path
manipulation. Complete hardening would require OS-specific handle-relative I/O
throughout each store; post-write canonicalization alone would not close that
race and is outside this Alpha-3 change.

Prompt source/template files under `codex-rs/prompts/` and embedded Markdown in
`codex-rs/core/src/` are tracked build inputs, not generated repository
artifacts. `codex-rs/core/src/prompt_debug.rs::build_prompt_input` assembles a
model-visible prompt in memory; `run_debug_prompt_input_command` in
`codex-rs/cli/src/main.rs` prints its JSON to stdout.

The extended workflow adds its policy and plan in memory and writes no separate
prompt or receipt files. `audit_mode = true` marks the normal model-visible
plan/session diagnostics as audit-enabled; it never creates a file. Compact
validation metadata is persisted in the state database belonging to the
selected Codex home, not in a new repository artifact. Existing rollouts and
prompt files are never deleted.

## Safe repository preparation

Both entry points call the same planner and applier from
`codex-rs/config/src/extended_workflow_prepare.rs`:

```shell
# Preview only; never writes.
codex-extended-workflow prepare-extend-workflow

# Prints the plan first, then performs only that safe change.
codex-extended-workflow prepare-extend-workflow --apply

# Global isolated configuration under ~/.codex-extended-workflow/config.toml.
codex-extended-workflow prepare-extend-workflow --global
codex-extended-workflow prepare-extend-workflow --global --apply
```

In the TUI use `/prepare-extend-workflow`, `/prepare-extend-workflow apply`,
`/prepare-extend-workflow global`, or `/prepare-extend-workflow global apply`.
Repository mode finds the nearest Git worktree
root, analyzes existing Codex TOML, rejects malformed or explicitly disabled
workflow configuration as a conflict, and also treats an existing file without
an `[extended_workflow]` table as a manual-merge conflict. Existing files are
never replaced. The command prevents symlink/root escape. A new file is
published from a fully synced same-directory temporary file using a no-clobber
hard-link commit point; if another process creates `config.toml` after preview,
the command returns `PreviewStale` and never replaces it. A correct file is
reused byte for byte; repeated application is idempotent. There is deliberately
no force or overwrite option. For a manual merge, add `[extended_workflow]` and
`enabled = true`, then preview again. Outside Git, in non-writable repositories,
and on stale previews it fails without replacing user content.

For an empty target the generated, commented configuration includes the
explicit expert and reviewer profiles, 2..4 process ranges, audit and result
limits, `workflow_budget_tokens = 0`, and the `session-tokens` and
`workflow-tokens` status-line items. Existing files are still a manual-merge
boundary and are never overwritten.

## CLI compatibility and versioning

The primary Rust and npm binary is `codex-extended-workflow`; help, errors,
completion, startup, `/status`, and resume examples use that name. npm keeps the
upstream platform payload filename `vendor/.../bin/codex(.exe)` for artifact
compatibility, but `bin/codex-extended-workflow.js` sets the narrowly scoped
`CODEX_EXTENDED_WORKFLOW_LAUNCHER=1` marker before launching it. The native CLI
recognizes only the exact value `1`, so the packaged primary command receives
the extended defaults while `bin/codex.js` remains the disabled-by-default
compatibility entry point. Rust crate names and other public interfaces remain
stable. App-server, foreground remote control, MCP tool sessions, and the
TypeScript SDK propagate the same one-shot product default; an explicit
configuration value still wins. Both launchers report `v0.147.1-alpha-4`.

The official app-server updater and update prompt are hard-disabled when the
extended product mode is active, preventing a managed daemon from replacing
this fork with an upstream binary. The compatibility `codex` launcher retains
the upstream updater behavior. Installation and publishing are intentionally
local-only until an explicit release process is requested.
