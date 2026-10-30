# MemPalace for bot memory (evaluation)

Status: evaluation only. Nothing is installed, wired in or added as a dependency.

Recommendation: **borrow ideas, don't adopt.** MemPalace is a strong, MIT-licensed local memory system, but it is a Python service with its own vector store and embedding model. Running it inside Loadout would add a second runtime, a second copy of memory that the keyring can't seal, and a store of raw chat that Loadout deliberately doesn't keep. Two of its ideas are worth taking into the Rust memory we already have: hybrid lexical plus vector recall, and validity windows on facts.

## What it is

MemPalace is an open-source memory layer for AI assistants. It mines conversations and project files into a local "palace" and lets an assistant search it later.

- **Repo and license:** `MemPalace/mempalace` on GitHub, MIT. Published on PyPI as `mempalace`.
- **Language and runtime:** Python 3.9 to 3.14. Installs with `uv`, `pipx` or `pip`, or as a Docker image (amd64 and arm64).
- **Storage:** content is kept verbatim, not summarized. The default vector store is embedded ChromaDB; `sqlite_exact`, Milvus, Qdrant and pgvector are other backends. A separate SQLite file holds a temporal knowledge graph: entities and relations with validity windows (add, query, invalidate, timeline).
- **Structure:** people and projects are "wings", topics are "rooms", and the original text sits in "drawers". The hierarchy narrows search before the vector lookup.
- **Recall:** semantic search over embeddings, with a hybrid mode (vector plus keyword) and optional LLM reranking. A `wake-up` command loads a small core context at the start of a session; deeper search runs on demand.
- **Embeddings:** a local model downloaded on first use (`all-MiniLM-L6-v2`, English only, or `embeddinggemma-300m`, multilingual), or any OpenAI-compatible `/v1/embeddings` endpoint such as llama.cpp, Ollama or LM Studio. A palace records which embedder built it and refuses to open with another until the index is rebuilt.
- **Interfaces:** a CLI (`init`, `mine`, `search`, `wake-up`, `audit` and more), a Python API, and an MCP server over stdio with about 44 tools. It ships auto-save hooks for some coding agents.
- **Privacy:** its README says nothing leaves the machine unless you opt in. The first embedding call needs the network to fetch the model. Since v3.11 it disables ChromaDB's own telemetry explicitly.

### Maturity

- Created in April 2026, so about seven months old. Very popular (tens of thousands of stars) and busy: the latest release, v3.11.0, came out in mid-October 2026, and PyPI still marks it "Beta".
- The headline launch claims (100% on LongMemEval and LoCoMo) were walked back. The LoCoMo run used a `top_k` larger than the number of sessions, and the LongMemEval run used targeted fixes and reranking. Its own docs now give 96.6% R@5 on LongMemEval for raw semantic search with no LLM, and lower numbers on LoCoMo. These are self-reported.
- Upgrades still carry migration steps: removed APIs, an embedder identity check that blocks a mismatched palace, and a one-time re-mine of conversations.
- Platform support leans on Linux and macOS. There is a DirectML extra and some Windows-specific fixes, but early coverage listed Windows as an open issue, and segfaults on macOS ARM64 and ChromaDB compatibility problems were reported.

## How Loadout keeps memory today

Everything is local Rust over files and SQLite, with no extra process:

- **Legacy notes** (`grokhub_agent::memory`): global and per-workspace `MEMORY.md`, plus a pre-compaction flush file, indexed in `{config}/memory/index.sqlite` with SQLite FTS5. The markdown files are the truth and the index can be deleted and rebuilt. Recall score is BM25 times a 30-day half-life decay. Injected recall is fenced as untrusted and capped at 4 KB / about 1,024 tokens.
- **Agent Memory Repo** (`grokhub_core::amr`, the default backend on new installs): one markdown node per fact, preference, decision, routine, person or project under `{config}/amr/nodes/`, edges in `edges.jsonl`, and an FTS5 index over plain nodes. Corrections add `supersedes` / `contradicts` edges. `forget` leaves a tombstone. Secrets are redacted on write.
- **Sealing:** personal or sensitive nodes are stored sealed (AEAD, key in the OS keyring) and are only indexed in memory once unlocked. A locked keyring pauses writes instead of writing plain text.
- **What gets kept:** `grokhub_core::learn_notes` writes a chat line only when it states a lasting fact, corrects the agent, or asks to be remembered. Raw chat is never piled into memory on purpose.
- **Consent:** core memory lines from a turn are proposed for approval, not written straight in. A `memory_writes` switch turns writes off.
- **Testing:** a LongMemEval-style recall eval runs on local fixtures (`grokhub_agent::eval::longmem`).

There are no embeddings anywhere in memory today; recall is lexical only.

## Fit and clash

| Area | MemPalace | Loadout today | Verdict |
| --- | --- | --- | --- |
| Runtime | Python service plus ChromaDB | One Rust binary, `rusqlite` | Clash: a second runtime to ship, update and supervise on Windows and Linux |
| What is stored | Verbatim conversations and files | Distilled lines only; raw chat is filtered out | Clash: it would keep exactly what `learn_notes` is built to drop |
| Sensitive data | Plain vectors and text on disk | Sealed nodes, keyring key, redaction on write | Clash: a palace would hold sealed facts unsealed |
| Recall quality | Semantic plus keyword, reranking | Keyword (FTS5) plus recency | Gap worth closing |
| Facts over time | Validity windows on relations | `supersedes` / `contradicts` edges, tombstones | Close; validity windows are a clean addition |
| Wake-up context | Small core load, then search on demand | First-turn injection under a token cap | Already similar |
| Hookup | MCP over stdio | Loadout already connects to MCP servers (Settings → Connectors) | Possible, but a whole extra process for one feature |
| Embeddings | Local model download or an `/v1/embeddings` endpoint | llama.cpp `llama-server` sidecar for the on-device model | The sidecar could serve embeddings without MemPalace |

## Privacy

MemPalace is local by default, which fits. The problems are what it stores and how:

- It keeps whole conversations. Loadout's rule is to keep only lasting facts and corrections.
- Its stores are not sealed with the keyring, so anything marked personal or sensitive in AMR would sit in plain text in ChromaDB and SQLite.
- The first run downloads an embedding model unless pointed at a local endpoint.
- If chats and memory sync to paired computers, a second store means a second thing to keep out of, or put into, that sync.

None of this is a flaw in MemPalace. It's a different contract from the one Loadout makes on its Privacy page.

## License

MIT on both sides. Borrowing ideas needs nothing. Vendoring or shipping MemPalace code would need its MIT notice kept, and its dependency tree (ChromaDB, numpy, grpcio, Pillow and the embedding stack) would each need a license and advisory check. One release note already flags a Pillow advisory on Python 3.9.

## Cost to integrate

Adopting it as is would mean:

1. Bundling a Python runtime and its wheels in the Windows installer and Linux packages, or requiring Docker. That likely adds hundreds of MB to the install.
2. Starting, watching and stopping a second sidecar beside `llama-server`, and pausing it under the same governor rules (games, battery, heat).
3. Two-way sync between AMR nodes and the palace, or moving memory into the palace and losing sealing, tombstones and approval staging.
4. Handling its migrations (embedder identity, re-mines, removed APIs) on every update.
5. New tests for all of that on both platforms, where only `crates/` builds today.

That is weeks of work and an ongoing upkeep cost, for a recall gain we can get more cheaply.

## Recommendation: borrow ideas

Skip MemPalace as a dependency. Take these ideas into `grokhub_core::amr` and `grokhub_agent::memory`, each as its own small change:

1. **Hybrid recall.** Keep FTS5 and add an optional vector score from the `llama-server` sidecar's embeddings endpoint, only while the on-device model is on. Store vectors next to the index, rebuildable from the node files, and only for plain nodes (sealed nodes stay in-memory only, as their FTS index does). Record which embedder built them and rebuild on a mismatch, as MemPalace does.
2. **Validity windows.** Give fact and relation nodes `valid_from` / `valid_to`, so "lives in X" can end without a tombstone and recall can answer "what was true then".
3. **Measure first.** Extend the LongMemEval-style eval with multi-session and knowledge-update cases and record a baseline before and after hybrid recall, so any change is shown to help.

Revisit adopting MemPalace only if it ships a stable embeddable library (no Python service) with at-rest encryption, or if Loadout ever needs whole-conversation search, which today it doesn't.

## Sources

- https://github.com/MemPalace/mempalace
- https://raw.githubusercontent.com/MemPalace/mempalace/main/README.md
- https://pypi.org/project/mempalace/3.4.0/
- https://newreleases.io/project/github/MemPalace/mempalace/release/v3.11.0
- https://www.sourcepulse.org/projects/27582815
- https://awesome.ecosyste.ms/projects/github.com%2Fmempalace%2Fmempalace
- https://blog.mean.ceo/?p=3663
