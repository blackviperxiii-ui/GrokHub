---
name: lean-ci
description: "Make a repo's GitHub Actions CI fast and cheap without weakening what it proves: measure first, run each check at the right stage, test only what the diff reaches, reuse caches, cap artifact storage, and prove the after time. Use for /lean-ci, 'CI is slow', 'CI costs too much', 'speed up the PR gate', re-enabling CI after it was turned off, or setting up CI on a new repo."
---

# Lean CI

Make CI fast and cheap while every required check still means what it meant. A faster gate that proves less is not lean, it's broken.

Five moves, in this order: delete waste, run each check at the right stage, test what changed, reuse setup and caches, balance the jobs so the slowest one sets the time. The usual target is a pull request gate under about 5 minutes (under 3 for small repos) with the heavy builds after merge.

## Hard limits

These hold even when the user says "just make it fast":

- Never delete or loosen a test, and never drop a check from what runs before release. Move it to a later stage instead.
- Never remove or rename a required status check, and never edit a branch ruleset or protection to make a PR pass. A required check you no longer run on PRs must still report (see "Skipped is passing").
- Never change spending limits, billing, runner plans, or delete artifacts, caches or runs without the user's explicit yes for that action.
- Never cancel runs by hand unless the user said so for that run.
- Never push to the release branch (`main`), force-push, or change the default branch. CI changes land as a normal PR into the integration branch.
- Release and publish workflows keep their triggers unless the user asks. Ask before shortening the retention of anything a release step downloads.

## Step 1: measure before

You can't claim faster without a before number.

```bash
gh api "repos/OWNER/REPO/actions/runs?per_page=100&created=>=YYYY-MM-DD" \
  --jq '.workflow_runs[] | select(.conclusion=="success" or .conclusion=="failure")
        | [.name, .event, (((.updated_at|fromdate)-(.run_started_at|fromdate))/60)] | @tsv'
```

Group by workflow and event and record median and p90 wall time. Then pull job times for a few typical runs (`gh api repos/OWNER/REPO/actions/runs/RUN_ID/jobs --jq '.jobs[]|[.name,.started_at,.completed_at]|@tsv'`) and find the long pole: the one job that sets the wall time. Also list:

- Every workflow, its triggers and its state (`gh api repos/OWNER/REPO/actions/workflows`).
- The required checks on each protected branch (rulesets and classic protection).
- Cache usage (`gh api repos/OWNER/REPO/actions/cache/usage` and `.../actions/caches`). GitHub evicts caches past 10 GB per repo, so a full cache means cold builds.
- Artifact uploads and their `retention-days`.
- Which runners each job uses (hosted or self-hosted) and whether the repo is public (free hosted minutes) or private (counts against the account's included minutes).

Write the table down. It goes in the PR body.

## Step 2: delete waste

- Workflows nobody reads (duplicate linters, a second test run of the same thing, a scheduled job whose output nobody uses).
- Steps that rebuild what an earlier job already built in the same run. Pass an artifact with `retention-days: 1` instead.
- Matrix legs that add no coverage on PRs (a third OS, a second toolchain).
- Uploads on runs where nobody downloads them.

Deleting a workflow or job is a change the user should see named in the PR. Deleting a test is never on this list.

## Step 3: run each check at the right stage

Split checks into three stages:

| Stage | Runs | Purpose |
|---|---|---|
| PR gate | every PR into the integration branch | fast, catches most breakage: format, lint, unit tests for what the diff reaches, guards, secret and PII scans |
| Post-merge push | push to the integration branch | the heavy legs: other OSes, mobile and desktop builds, end-to-end and screenshot runs, packaging |
| Release gate and nightly | PRs into `main`, merge queue, push to `main`, schedule, manual | everything, with no path filter, plus the slow OSes |

Windows and macOS runners are the usual long pole. Move them off integration-branch PRs to the post-merge push, and keep them on PRs into `main`. Say so in the workflow header comment.

### Skipped is passing

A job skipped by its own job-level `if:` reports "skipped", and GitHub counts a skipped required check as passing. So gate jobs with a job-level `if:` on outputs from a small `changes` job, never with a workflow-level `on.*.paths` filter on a workflow that holds a required check. A workflow-level filter means the check never reports and the PR hangs forever on "Expected".

```yaml
jobs:
  changes:
    runs-on: ubuntu-latest
    outputs:
      rust: ${{ steps.filter.outputs.rust }}
      windows: ${{ github.event_name != 'pull_request' || github.base_ref == 'main' }}
    steps:
      - uses: actions/checkout@v4
      - id: filter
        uses: dorny/paths-filter@v3
        with:
          filters: |
            rust:
              - '**/*.rs'
              - '**/Cargo.toml'
              - 'Cargo.lock'
              - '.github/workflows/ci.yml'

  windows:
    needs: changes
    if: ${{ !cancelled() && (needs.changes.result != 'success' || needs.changes.outputs.windows == 'true') }}
    runs-on: windows-latest
    steps:
      - if: needs.changes.result != 'success'
        run: exit 1
      # ... real steps
```

The `needs.changes.result != 'success'` branch makes the job run and fail when `changes` itself broke, so a broken filter can never turn into a green skip. Always include the workflow file itself in every filter so a CI edit tests itself.

### Aggregator jobs

When you split one required job into several (for example `lint` and `test` out of `linux`), keep the required name as a thin aggregator so the ruleset doesn't change:

```yaml
  linux:
    needs: [changes, linux-lint, linux-tests]
    if: ${{ !cancelled() }}
    runs-on: ubuntu-latest
    steps:
      - if: needs.changes.result != 'success'
        run: exit 1
      - run: |
          for r in "${{ needs.linux-lint.result }}" "${{ needs.linux-tests.result }}"; do
            case "$r" in success|skipped) ;; *) echo "dependency result: $r"; exit 1;; esac
          done
```

## Step 4: test what changed

On PRs, run tests only for the packages the diff reaches, including everything that depends on them. On push, nightly and release runs, run the full suite.

- The PR checkout is the merge commit. With `fetch-depth: 2`, `git diff --name-only HEAD^1 HEAD` is exactly the PR's change.
- Map changed paths to packages with the build tool's own metadata (`cargo metadata`, `npm query`/workspaces, `go list -deps`, `pnpm --filter ...[origin/beta]`, Nx/Turbo affected). Add the transitive reverse dependencies.
- Fall back to the whole workspace when the diff touches anything shared: the lockfile, the root manifest, CI files, build scripts, shared assets or fixtures, any path you can't map to exactly one package, or an empty diff.
- Ignore pure docs paths only when nothing reads them at test time. Check first: some repos load docs or data files in tests.
- Put the picker in a small script with its own unit tests that assert real outputs for real paths (a leaf change, a core change that fans out, a lockfile change that runs everything). Run those tests in the same job. A picker that silently returns too little is the one way this step can weaken the gate.

See `references/affected-packages.md` for a worked Cargo example.

## Step 5: reuse setup and caches

- One cache key per job shape (`Swatinem/rust-cache` `shared-key`, `actions/setup-node` `cache: npm`, `setup-python` `cache: pip`). Don't let every job write its own giant cache.
- Save caches only from push runs on the integration branch (`save-if: ${{ github.event_name == 'push' }}`) so PR caches don't evict the good ones.
- Watch the 10 GB repo cache cap. Release workflows with full target dirs fill it fast; give them their own key and keep them off the PR gate's key.
- Install toolchains with pinned versions and the setup action's cache, not by hand each run.
- Use prebuilt tool installers (`taiki-e/install-action` for `cargo-nextest`, `actionlint`, and similar) instead of compiling tools in CI.

## Step 6: concurrency and runners

```yaml
concurrency:
  group: ci-${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}
```

PRs cancel their own older run. Pushes to protected branches don't cancel, so every merged commit gets a full result.

Runner gotchas:

- A queued run on a self-hosted runner that is offline holds the concurrency group, so every newer run on that ref waits behind it as "pending". Fixing the runner label doesn't help, because `runs-on` is fixed when a run queues. The stuck run must be cancelled (with the user's yes) before anything moves.
- To switch a private repo from self-hosted to hosted runners, prefer a repo variable in `runs-on` (`${{ vars.RUNNER || 'ubuntu-latest' }}`) so it's one switch, then delete the old self-hosted branch of the expression once hosted is proven. Hosted minutes on a private repo count against the account's included minutes; tell the user before switching and never change a spending limit yourself.
- Hosted runners start clean. Jobs that worked on a self-hosted box often relied on tools installed there (a CLI from a root `package.json`, a system package). Install them in the job.

## Step 7: artifacts

- Upload only where someone downloads it: build bundles on post-merge pushes and release runs, not on PRs. Failure reports (`if: failure()`) are fine anywhere.
- `retention-days: 1` for hand-offs between jobs in the same run and for debug-only artifacts. Keep longer retention only where a documented release step downloads the artifact later, and say which step in a comment.
- Artifact storage quota is account-wide. When it's full, uploads fail in every repo on the account. Shorter retention stops the growth; deleting old artifacts frees space now, and that needs the user's yes.

## Step 8: agents and permissions

- Agents with a fine-grained or app token often get HTTP 403 on `workflow_dispatch` and on run cancel. Don't route around it with another credential. Push a commit or ask the user to dispatch.
- Re-enabling disabled workflows: record the state first (workflow states and required checks to a backup file), then re-enable from that record so you restore exactly what was there. Leave workflows the user turned off for cost (paid AI review bots, autofix bots) off unless they say otherwise.

## Step 9: prove it

Before calling it done:

1. Lint the workflow (`actionlint`) and run any repo CI guard scripts and their tests.
2. Run the picker's unit tests.
3. Open the PR. Its own CI run is the first after number: the PR gate wall time and the long pole.
4. After merge, record the push-run time and confirm the heavy legs still ran on push.
5. Confirm every required check still reports on a docs-only PR (skipped, not missing).
6. Put the before and after table in the PR body and the report: per workflow and event, median before, measured after, and what moved where.

Report what you didn't verify (for example, "nightly not yet observed").

## Per-repo checklist

- [ ] Before times recorded (median, p90, long pole).
- [ ] Required checks listed per protected branch; none removed or renamed.
- [ ] Every job gated by job-level `if:` on a `changes` job; no workflow-level path filter on a required-check workflow.
- [ ] Broken `changes` job fails loud, never a green skip.
- [ ] Concurrency cancels stale PR runs only.
- [ ] Windows and macOS off integration-branch PRs; on PRs into `main`, merge queue, push and nightly.
- [ ] Heavy builds, end-to-end and screenshot runs on post-merge push, not PRs.
- [ ] PR tests limited to affected packages, with a tested picker and a full-workspace fallback.
- [ ] One cache key per job shape; caches saved from pushes only; repo under the 10 GB cap.
- [ ] Uploads only where downloaded; `retention-days: 1` unless a release step needs longer.
- [ ] No stale self-hosted queued runs blocking the concurrency group.
- [ ] Release workflow triggers unchanged unless the user asked.
- [ ] After times measured from real runs and reported with the before table.
