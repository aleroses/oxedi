# CLAUDE.md — how work is done in oxedi

Lossless, fast, data-driven EDI 835 parser core in Rust (edition 2024); learning Rust in
depth is a co-equal goal. Public repo `javillegasna/oxedi`. Conversation language is
Spanish; code, commits, PRs and issues are in English.

## Non-negotiables in code

These apply in any clone or fork. The code conventions and test-data rules are in
`CONTRIBUTING.md` (sections Code conventions and Tests and test data), imported here; follow
them exactly as written there. `ARCHITECTURE.md` is the public map of the code.

@CONTRIBUTING.md

Agent-specific additions:

- "Comments describe the code, not its history" also excludes stage numbers, the
  maintainers' internal codes (such as N1 or P10) and links to planning notes.
- Open an issue before adding any dependency, as `CONTRIBUTING.md` says; the maintainer
  approves each one before it is added.

Rules for every change and refactor, checked by implementers and reviewers alike:

- Null safety and security come first by default. No `None`/null sentinel for "the default"
  and no untyped values (`Any`, `dict[str, Any]`, stringly-typed options) in a Rust or Python
  API: a default is the typed value (rendered in the stub), an absence is a type. Conversions
  are checked (no silent truncation, wrap or fallback). When readability or brevity competes
  with null safety or security, null safety and security win.
- Touch it, fix it: a change that modifies a function listed in
  `.github/complexity-baseline.txt` leaves it within the targets; `make complexity` fails
  with `TOUCHED` otherwise. Only flat per-variant dispatch is accepted (`flat`), with its
  reason in the PR.
- Fix the cause, never only split a function: choose the fitting pattern from any family
  (design, architecture, functional, dynamic programming and memoization, concurrency- and
  parallel-friendly data, algorithms and data structures) and name it in the PR.
- Lower complexity without raising cost. Chaining iterator passes is fine on cold paths (once
  per spec, file or option set); on hot paths (per segment, element, cell or row) keep or
  improve the Big-O and the allocations: no nested scans, no search over a growing collection
  inside a loop, no repeated `collect`. Where it matters, choose the algorithm or
  architecture (single pass, an index built once, streaming), and run the affected
  `cargo bench` groups before and after, with the numbers in the commit message.
- Keep verification cheap: compare benchmarks with only the affected groups and short times
  (`cargo bench -p oxedi_core --bench <group> -- --warm-up-time 1 --measurement-time 3`) and
  leave the full measurement to the final review; when checking each commit in a temporary
  worktree, share one `CARGO_TARGET_DIR` so only what changed is rebuilt.
- An open issue a change touches is fixed or fulfilled and closed by that change
  (`Closes #N`), never left out of date.
- A PR changes at most 400 lines (additions plus deletions, tests included, generated goldens
  and stubs excluded); a larger change is planned as several PRs, each merging on its own with
  its own gate. Its body is in English with two sections, **Intent** and **Verification**; never
  list the diff content; no attribution lines. The maintainer merges.

## Maintainer workflow

This section needs the maintainer's local notes. `.doc/` is git-ignored and exists only in
the maintainer's checkout, and the project board is private: in a clone or fork neither is
there, so skip this section and work from the issue, `CONTRIBUTING.md` and `ARCHITECTURE.md`.
The only public planning page is `docs/roadmap.md` on the site.

Where the notes live:

- `.doc/architecture/`: the contract, one file per part; start at its `README.md`.
  `contract.md` holds the invariants (N1–N7) and principles (P1–P10; "Lossless" is N1,
  "Errors explain themselves" is P10), `processing-model.md` and `stage-map.md` the model and
  the map, `decisions/` the index of every decision (taken T*, open D*), and `stages/` one
  commitment file per stage with its decisions, deliverable, gate and scope, listed in
  `stages/README.md`. A stage starts only after its commitment file is approved by the owner;
  a plan is written only after that. A new stage is a new file plus one row in each index.
- `.doc/roadmap.md`: stage status, what each unlocks, open decisions by stage.
- `.doc/plans/stage-N-*.md`: executable plans with TDD steps and an exit gate.
- `.doc/state.md`: current snapshot and next steps (update it when a stage changes state).
- `.doc/analysis/`: the owner's study notes with Mermaid diagrams about the code (structure,
  flow, tests and their reasons, Rust concepts, patterns). Update them per stage from the
  real code, not from plans; classDef colours need `color:#111`.
- `docs/roadmap.md` (public, on the site): a short, user-facing summary in English: available
  now, next, being considered; no stage codes, decision ids or internal detail. Update it with
  one line when a stage ships or the next item changes.
- GitHub Project #8 "oxedi835 — Roadmap & Deuda Técnica" (private), the one board: work
  organized as initiative → epic → task. Every issue gets `Level`, `Initiative`, `Priority`
  and `Size`; epics are issues whose tasks are native sub-issues. Deferred review findings are
  issues (Context / Problem / Recommendation, no solutions). Status moves with the work: the
  current sprint (about 15 days) in Sprint Backlog, In Progress before work starts, In Review
  when its PR opens, Done when the PR closes it; Done items are archived.

How a stage runs:

1. Discuss open decisions with trade-offs in chat; write the stage's commitment file in
   `.doc/architecture/stages/`; owner approves. A new dependency is approved there.
2. Write the plan with workflow-kit:write-plan in `.doc/plans/stage-N-*.md`; owner approves.
3. Feature branch from `master`. Execute with workflow-kit:run-plan: the main session only
   orchestrates, batches, model tiers, reviews, fix rounds and the ledger follow the kit. Set
   up the workspace with the rules and lenses below; the `CONTRIBUTING.md` sections are named
   because `wk-setup` copies the `@CONTRIBUTING.md` import line as text and does not expand it:
   `wk-setup <plan> 'CLAUDE.md#Non-negotiables in code' 'CONTRIBUTING.md#Code conventions'
   'CONTRIBUTING.md#Tests and test data' 'CONTRIBUTING.md#Complexity'
   'CLAUDE.md#Final review focus' 'CLAUDE.md#Agents in the sandbox' lens:rust lens:python`.
   Before deleting the workspace, archive the ledger with
   `wk-archive <plan> .doc/analysis/stage-N-ledger.md`.
4. PR against `master` as above. The owner merges.
5. Deferred minors become issues on Project #8. Then update `.doc/roadmap.md`,
   `.doc/state.md` and, when what users can do changed, `docs/roadmap.md`.

### Agents in the sandbox

- Each implementer works in its own worktree of the feature branch, never in the main checkout.
- Every command that runs project or dependency code runs as `make sandbox CMD="..."`:
  `make gates`, `make py-test`, `make viewer-test` (with `IMAGE=viewer`), the sandbox's own tests
  and a single `cargo test` alike. Only the targets `CONTRIBUTING.md` lists under "What stays on
  the host" run on the host.
- Docker commands use the rootful daemon for that command only, as
  `env -u DOCKER_HOST DOCKER_HOST=unix:///var/run/docker.sock make sandbox CMD="..."`; never
  change the shell's or Docker's configuration. When that daemon does not answer, the agent stops
  and asks the owner to start it.
- One sandbox run per worktree at a time: a second one is refused while the first runs. Do not
  commit while a run of the same worktree is in progress, since its audit reports the change.
- Implementers run in parallel only when the plan marks batches with no file in common, each in
  its own worktree; the controller integrates them in batch order.

### Final review focus

- Errors explain themselves (P10): every error variant says what failed, where and what to do.
- Null safety and security, as in the non-negotiables above, across the whole branch.

## Commands

`make help` lists every target. The gates and the Python binding have make targets; the raw
commands are kept here because CI runs them directly.

```bash
make gates            # fmt-check, clippy -D warnings, test, bench --no-run, doc -D warnings
make py-test          # maturin develop into .venv, then pytest crates/oxedi_py/tests
make dist && make smoke   # sdist + release wheel, then install in a clean venv outside the repo
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all && cargo fmt --all -- --check
UPDATE_GOLDEN=1 cargo test -p oxedi_core --test engine_golden   # regenerate goldens, then inspect
UPDATE_GOLDEN=1 cargo test -p oxedi_core --test project_golden  # table and diagnostic goldens
cargo bench --workspace                                           # ~5 min; baselines go in commit messages
```

Publishing (`make publish-test`, `make publish`, `make tag`) reads `~/.pypirc`; tokens never
enter the repo. Python floor 3.11; the base wheel declares no Python dependencies.
