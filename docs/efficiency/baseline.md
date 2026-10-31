# Efficiency bench baseline (LO-30)

How to run it: `cargo run -p grokhub-agent --example efficiency_bench`. The bench is offline and scripted, so it needs no keys and no network. Set `GROKHUB_BENCH_DUMP=<dir>` to save every request body as JSON.

Each scenario runs the real chat engine: the real system prompt, the full tool set and the app's unlimited step budget. A scripted model stands in for Grok or a local model, so the byte counts are what either one would be asked to read. The "est tokens" column is bytes divided by 4. "Stable prefix" is the share of each request's tools+input head that the next request repeats byte for byte. That is the part a provider prompt cache or a llama.cpp KV cache can reuse. "Recall" checks that facts planted early are still in the last request. Peak RSS is the process high-water mark so far, and it only grows.

Baseline on beta at 36b685bc:

| scenario | requests | 1st sys B | 1st tools B | 1st history B | 1st total B | all requests B | est tokens | stable prefix % | recall | wall ms | peak RSS so far KB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| chat | 1 | 12427 | 21012 | 109 | 33632 | 33632 | 8408 | 100 | ok | 23 | 23284 |
| tools | 4 | 12427 | 21012 | 125 | 33648 | 273229 | 68307 | 100 | ok | 69 | 25860 |
| diagnostics | 7 | 14547 | 21012 | 108 | 35752 | 944911 | 236227 | 100 | ok | 106 | 29260 |
| long-chat | 1 | 12369 | 21012 | 11096 | 44601 | 44601 | 11150 | 100 | ok | 18 | 29036 |
| three-turns | 4 | 12427 | 21012 | 90 | 33613 | 135689 | 33922 | 98 | ok | 58 | 29036 |

What it shows:
- **A plain "hi" costs about 8.4k tokens.** 21 KB of that, 62%, is tool schemas sent on every request, and 12 KB is the system prompt (LO-34).
- **"Run a diagnostics check" makes 7 requests for a 4-step job.** The done-list asks for "services", "packages", "boot errors" and "ports", which no tool result can show, so the run is pushed again 3 times ("You stopped again without progress"). Each push resends the full context (LO-31).
- **One read_file of a 2000-line log returns 137 KB.** That output is carried in every later request, which is most of the diagnostics run's ~236k tokens (LO-35).
- **Prefix stability is already high within a run,** at 98–100%, so the cache work in LO-33 is about keeping it that way across turns and resumes.
