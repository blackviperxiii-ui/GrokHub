# changelog.d — feature PR fragments

Feature and fix PRs into `beta` **do not** edit `CHANGELOG.md` or bump
`VERSION`. Add one markdown fragment here instead:

```text
changelog.d/<short-slug>.md
```

Each non-heading line becomes a bullet under the next release section when
someone runs:

```sh
cargo xtask bump <X.Y.Z>
```

Preview the exact set of files a bump would write or remove, without
touching disk:

```sh
cargo xtask bump --dry-run <X.Y.Z>
```

Example fragment:

```md
- CI: friction guards for conflict markers and god-file budgets (rearch stage 1).
```

Rules:

- One fragment per PR (or per logical change). Name it after the PR number or a short slug (`790-ci-guards.md`).
- Prefer a single `- ` bullet. Extra bullets are fine; blank lines and `#` headings are ignored by `xtask bump`.
- Do not bump `VERSION`, `Cargo.toml`, packaging, or `cabin_reports_version` in a feature PR. A promotion or hotfix owns those via `cargo xtask bump`.
- `cargo xtask bump --check` (also run from `scripts/ci-guards.sh`) fails when the version copies disagree.
