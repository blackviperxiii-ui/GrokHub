---
name: lean-tests
description: "Make a test suite and its CI jobs faster without losing coverage: measure first, push proof down the ladder, keep budgets, change one round at a time, report before/after. Use for /lean-tests, 'tests are slow', 'suite too slow for CI', 'push tests down', or speeding a file/class/branch's tests."
---

# Lean Tests

Make the suite and its CI jobs faster while every behavior keeps a proof that is as fast or faster. A faster suite that proves less is not lean, it's broken.

Workflow layout, caches and job staging belong to **lean-ci**. App runtime cost belongs to **optimize**. A contested number goes to **benchmark-checklist**.

Adapted from Marek Kirejczyk's MIT [refactor-tests](https://github.com/marekkirejczyk/skills/tree/main/skills/refactor-tests) skill.

Cite **viper-mode**, **principle-prove-it-works**, **principle-explain-the-number**, **principle-sequence-verifiable-units**, **principle-laziness-protocol**.

Work in this order: measure, push down, budgets, rounds, report. A test gets tuned only after it has its final level.

## Hard rules

These hold even when the user says "just make it fast":

- Never delete or loosen a test or an assertion to buy speed. Loosening includes a wider tolerance, a partial match where a whole value was compared, a mock where the test's claim needs the real thing, and a retry added to hide a flake.
- Never add a skipped test (`#[ignore]`, `it.skip`, `xit`, `@skip`, a `cfg` or env gate that turns it off in CI). A behavior bug goes in the findings log. Today's behavior stays pinned by a green test.
- Test timeouts and CI job and step timeouts (`timeout-minutes`, nextest `slow-timeout`, vitest `testTimeout`) stay as they are. A raised timeout hides the cost you came to remove.
- Every test you remove names the faster test that proves the same thing. Write that test first if it is missing.
- Never move a test out of the gate that runs before merge to make the gate look faster. Changing where a job runs is **lean-ci** work and needs the user to see it named.
- Change and delete only tests in scope. Report an out-of-scope duplicate with the faster test that covers it.

## 1. Measure first

No before number, no claim. Take the baseline on a **clean checkout of the base commit** (a fresh worktree, not your working tree), on a quiet machine, before touching any test.

Write the decision down first: the budget or CI limit you are trying to get under, and the run-to-run spread. Run the baseline three times and take the median. A change smaller than the spread is not a result.

### CI jobs

Split each job's wall time per step into four buckets: **setup** (checkout, toolchain, cache restore, installs), **compile/build**, **test run**, and **smoke/e2e**. Record the run id of every run you measure.

```bash
# Step times for one run, in seconds
gh api "repos/OWNER/REPO/actions/runs/RUN_ID/jobs?per_page=100" \
  --jq '.jobs[] | .name as $j | .steps[]
        | select(.started_at and .completed_at)
        | [$j, .name, ((.completed_at|fromdate) - (.started_at|fromdate))] | @tsv'
```

Take three recent green runs of the same workflow and event on the base branch and use the median per step. Note whether each run had a warm or a cold cache. A cold-cache run measures the cache, not the tests.

If one step both compiles and runs tests, split it locally with the commands below so compile time never gets blamed on the tests. When compile/build or setup is the largest bucket, the suite is not the problem. Hand that to **lean-ci** and say so in the report.

### Local suites

Use CPU time (`user` + `sys` from `/usr/bin/time -p`), not wall time alone. Wall time hides extra work on a many-core machine. A suite can double its work and barely move on a 16-core laptop, then blow the limit on a 2-core CI runner.

Pick what the repo uses:

```bash
# Rust, cargo-nextest: build first so compile and run are separate numbers
/usr/bin/time -p cargo nextest run --profile ci --no-run
/usr/bin/time -p cargo nextest run --profile ci
# Per-test timings land in target/nextest/ci/junit.xml when the repo's
# .config/nextest.toml has:
#   [profile.ci.junit]
#   path = "junit.xml"
# If the profile has no junit section, read the per-test durations nextest
# prints, and list the slow ones with: --final-status-level slow

# Rust, stock cargo test: same split
/usr/bin/time -p cargo test --no-run
/usr/bin/time -p cargo test
# One test binary at a time shows which crate or target is slow:
/usr/bin/time -p cargo test -p my-crate --test my_integration
# Where the compile time goes:
cargo build --tests --timings

# JS/TS, vitest
/usr/bin/time -p npx vitest run
# Per-file and per-test durations, one file at a time so files don't share cores:
npx vitest run --no-file-parallelism --reporter=verbose
npx vitest run --no-file-parallelism --reporter=json --outputFile=/tmp/vitest-before.json

# JS/TS, whatever the package's test script runs
/usr/bin/time -p npm test
```

For another runner, find its equivalents of the same three things: build without running, run under `time`, and a per-test duration report (`pytest --durations=25`, `go test -json`, `jest --json`).

Write the before table: suite CPU time, test count, the ten slowest tests and the ten slowest files, each with its level on the ladder. Keep it outside the repo or in working notes, and keep the raw output next to it.

Rule out what else the number could be: tests that were filtered out or skipped, a cached run that did no work, a failed run that stopped early, a different feature set or profile than CI uses. The test count before and after is the first check.

## 2. Push-down ladder

Keep each test at the **fastest level that can prove what it asserts**. The ladder, fastest first:

1. **Pure function.** A decision, a calculation, a parse. No I/O, no clock, no shared state.
2. **Unit / service.** One component with fakes at its edges. Proves each outcome once and what lands in the fakes.
3. **Integration / route.** Real wiring across a boundary: a database, an HTTP handler, a subprocess, the filesystem.
4. **App smoke / browser.** The whole app. One journey per critical flow, plus what needs a real browser or a real window.

What earns a slow level, and what moves down:

| Level | Stays | Moves down |
|---|---|---|
| App smoke / browser | One journey per critical flow, and what only a real app or browser shows | Edge cases and logic, to route or unit tests |
| Integration / route | Each status or error class once, each outside service's wiring, each follow-up effect, real query behavior (filters, locks, constraints) | Every further reason for the same status, and row or payload mapping written in application code |
| Unit / service | Each outcome once, and what lands in the fakes | Decisions, to pure functions |

For each slow test in scope, try the fastest rung first and stop at the first that can prove it. Write a one-line **verdict** in working notes:

- **stays**: name what only this level proves.
- **moves**: name the faster level it goes to.
- **covered-by**: name the faster test that already proves every assertion. The slow test can then be removed.

```text
orders_api::rejects_negative_quantity   moves       pure fn validate_quantity
orders_api::creates_order_201           stays       only route proves status + DB row + email effect
checkout.spec "coupon edge cases" x6    covered-by  coupon::tests::* (unit); keep 1 journey
```

**Extract to test.** Pull out a small pure function when the code holds a real decision and the function gets a name from the domain. Move the code unchanged and keep it reachable through its module's real interface. Don't make a private helper public only for a test. When a test can only move by changing logic, its verdict is **stays**.

**Proof before a move or a removal**, one check per thing the test asserted:

1. Name the faster test that covers it. Write that test first if it doesn't exist.
2. Break that behavior in the code for a moment and confirm a remaining test goes red.
3. Revert the break and confirm the tree is clean.

No red test means the verdict is **stays** until a faster test goes red on that break.

## 3. Budgets

A cost belongs at the highest level that can carry it.

| Level | Paid | Default budget | What belongs here |
|---|---|---|---|
| **Run** | Once per suite invocation | Seconds are fine | Compiling, migrations run once and saved as a snapshot, fixture templates, a shared server or container |
| **File** | Once per test file, module or binary | 100 ms from first import to the end of its setup | Importing the app, starting it, opening its database from the snapshot |
| **Test** | Every test, its own setup included | 10 ms for pure and unit tests, 100 ms for a test that needs fresh real state (a database, a subprocess) | One call, one request, one round trip |
| **Suite** | The whole run | CPU time rises no more than 25% against the baseline, and the run fits CI's job limit | |

Use the repo's own budgets when it has them (a README, a CI limit, nextest `slow-timeout`). Otherwise use these defaults and say in the report that you did. Compiled languages pay the file budget at build time, so judge the build bucket separately.

Know what the runner keeps between tests. A runner that starts a new process per file (vitest by default) or per test (nextest) loses anything held in memory, so run-level work has to be handed on as files on disk.

**Price a split before making it.** Before a round that splits a test, or gives each example fresh state, multiply: new test count times setup cost per test. If the product breaks a budget, make the setup cheap first (snapshot, template, shared read-only fixture), then split. One example per test with fresh state multiplies whatever setup costs: a suite that goes from 800 tests to 2,200 with a 300 ms setup adds seven CPU minutes and nobody sees it on a laptop.

## 4. Rounds

One change per round. Re-run the scoped package or file, then the suite, and be green before the next round. Red after a round means the round changed behavior: undo it and take a smaller step. Don't fix forward.

Commit each green round on its own so any round can be reverted alone. Measure again after each round that was meant to save time. A round that saved nothing gets reverted, not kept as churn.

**Find the cause first.** Everything over budget gets a named cause, confirmed by measuring where its time goes (a profile, timestamps around setup, a run with the suspect removed). The usual suspects: a real database or server started per test, migrations run per test, a sleep or a poll with a long interval, a real network call, a hashing cost set for production, a full app import in a pure test, a build inside the test, serial tests that could share nothing and run in parallel.

Fix in this order:

1. **Prepare once per run and copy per test**: a database snapshot or template, a prebuilt fixture. Tests stay isolated and usually don't change.
2. **Replace a real dependency with a fake**, where the level still proves the claim. A fake clock for a sleep is the cheapest case.
3. **Share an expensive real fixture across a file**, only where every test in the file reads and none writes. This gives up some isolation, so it comes last and is named in the report.

**Warm-up moves into setup.** Whatever only the first test would pay moves to file or run setup, because any test can run first. Prove order doesn't matter: run the changed tests shuffled where the runner can (`vitest run --sequence.shuffle`, `cargo nextest` runs each test in its own process, the `pytest-randomly` plugin), and one at a time.

**Slow application code** is in scope only when the fix keeps what every caller sees. A fix that would change behavior goes in the findings log.

### Findings log

A bug you find is not part of this work. Leave the code as it behaves today, keep the green test that pins it, and add a line to the findings log in the report. No skipped "should be" test.

```text
FINDING  format_amount(-500) returns "£-5", expected "-£5"
  pinned by: money::tests::negative_amount_today (green)
  where: src/money.rs:41
  not fixed here: behavior change, out of scope
```

## 5. Report: `## Test speed`

Put this section in the PR body. Replace it if it already exists.

```markdown
## Test speed

Base: <sha>. Budgets: <repo's own | skill defaults>.

| Job | Step bucket | Before (run id) | After (run id) |
|---|---|---|---|
| linux-tests | setup | 0:41 (1234567) | 0:40 (1234999) |
| linux-tests | compile/build | 3:10 | 3:08 |
| linux-tests | test run | 6:20 | 2:05 |
| linux-tests | smoke/e2e | 1:30 | 1:30 |

Local suite: CPU 412 s -> 131 s (median of 3, spread 6 s). Tests 825 -> 840.
```

Then list:

- Each test moved or removed, and the faster test that now covers it.
- The ten slowest tests and files, before and after.
- What is still over budget: its time, its named cause, and the change that would fix it.
- Any fixture shared across a file, and why that file is read-only.
- The findings log.
- Out-of-scope duplicates and slow tests owned by another runner, left as they are.
- What you did not measure (for example, "Windows job not rerun").

The after column comes from real runs of the PR's own CI, not an estimate. If the test count went down, every missing test appears in the moved-or-removed list.

Stop when the budgets hold, or when every remaining over-budget item has a cause and a next fix (Laziness Protocol). Hand CI workflow changes to **lean-ci**.

## Checklist

- [ ] Baseline from a clean checkout of the base commit, median of three, spread recorded.
- [ ] CI time split per job into setup, compile/build, test run, smoke/e2e, with run ids.
- [ ] Local numbers are CPU time, and compile is separated from the test run.
- [ ] Every slow test in scope has a verdict: stays, moves, or covered-by.
- [ ] Every move or removal passed the break-it proof.
- [ ] Every split was priced before it was made.
- [ ] One change per round, each round green and committed alone.
- [ ] No test or assertion deleted or loosened, no skip added, no timeout raised.
- [ ] Test count before and after explained.
- [ ] `## Test speed` is in the PR body with what is still over budget and why.
