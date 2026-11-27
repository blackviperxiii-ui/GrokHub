---
name: database-architect
description: Database schema and migration work. Design tables, write or review migrations, tenant isolation (RLS), indexes, constraints, choosing a database. PostgreSQL default; SQLite notes for local storage. /database-architect
---

# Database Architect

Read and follow `AGENT.md`, vendored unmodified at a pinned commit (`VENDORED.json`, MIT, `NOTICE.md`). These rules come first:

**Safety.** Scratch databases only: never production or shared. Stop only a server you started, by its data dir (`pg_ctl -D <dir> stop`); never `pkill` or `killall`. Never print or commit credentials. Squawk only pinned (`npx squawk-cli@2.72.0`), never `@latest`.

**Hand-offs.** `AGENT.md` names agents vstack doesn't ship:
- postgres-pro (query tuning) → **optimize**
- backend-developer (ORM and app code) → **architect**
- security-auditor (PII) → **qa-walkthrough** security pass
- neon-database-architect → none; use Neon's docs, list what's untested.

**SQLite** (local app storage):
- `PRAGMA foreign_keys = ON` on every connection; it's off by default.
- `STRICT` tables; WAL plus `busy_timeout` for one writer, many readers.
- Number migrations with `PRAGMA user_version`, one transaction each.
- No drop-constraint or type change: set `foreign_keys = OFF` outside a transaction, rebuild (new table, copy, drop, rename) and run `PRAGMA foreign_key_check` before `COMMIT`, then ON.
- No RLS or roles; isolation lives in the app.
- Prove it on a temp file: `sqlite3 -bail`.

Resync: `docs/database-architect.md`.
