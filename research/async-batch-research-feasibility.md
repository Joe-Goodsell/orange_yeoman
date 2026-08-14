# Feasibility: Async Research via Batch APIs

**Status:** Research deliverable (plan-mode output)
**Date:** 2026-08-14
**Scope:** Cold / async research & fact-checking path only. Hot inline feedback is out of scope for this round (separate synchronous design later).

---

## TL;DR

Feasible and well-matched to the Orange Yeoman outline. The outline's "discounted batch processing" and "async manner" goals map onto Anthropic's Message Batches API, which supports autonomous web search and cited sources inside batched requests at ~50% token discount with sub-1h typical latency. The non-blocking UX requirement is *helped* by batching: results land later in the right pane, so latency is a feature, not a bug.

Caveat: the discount applies to model tokens, not to web-search calls themselves. Tune `max_uses` and batch size to control search spend.

---

## 1. Two workloads, two paths

| Path | Latency need | Batch fit |
|---|---|---|
| Hot / inline (quick corrections as you type) | sub-second to seconds | Wrong tool — batch SLA (1h typical / 24h max) blocks UX |
| Cold / async (`/research`, `/fact-check`, autonomous background research) | minutes to hours, surfaced in side pane | **Ideal** — matches outline's "async manner" + "discounted batch" |

The non-blocking UX requirement is reinforced by batching: results arrive later in the right pane exactly as the outline describes.

---

## 2. Provider comparison (verified this session)

| | **Anthropic Message Batches** | **OpenAI Batch** |
|---|---|---|
| Discount | 50% on tokens | 50% on tokens |
| Typical latency | **most batches < 1 hour** | up to 24h (often faster, but SLA is 24h) |
| Batch caps | 100k requests / 256 MB | 50k requests / 200 MB |
| Web search inside batch | Supported — `web_search` + `web_fetch` server tools; returns **cited sources** (url, title, cited_text) | Supported via `/v1/responses` web-search tool |
| Prompt caching | Stacks with batch discount (30-98% hit rate) | Available |
| Submission | Inline request array (no file upload) | Requires `.jsonl` file upload (operational overhead) |
| Results | Stream `.jsonl`, unordered — key off `custom_id` | Same, unordered |
| Expiration | 24h processing window; results available 29 days | 24h window; output deleted 30 days after completion |

### Why Anthropic is the stronger default for this project

Fact-checking *is* "search the web for sources that support/refute claims." Anthropic lets the model run web search autonomously inside each batched request and hand back citations (url, title, cited_text). The sub-1-hour typical latency also fits a personal note app far better than a 24h SLA.

**Note on Google Gemini Batch:** a batch API exists but its specs could not be verified this session (docs fetch failed). Confirm before relying on it for cost arbitrage.

---

## 3. The cost caveat that matters

"Discounted" applies to **model tokens, not web-search calls.**

- Anthropic web search: **$10 / 1,000 searches** at full price. The 50% batch discount applies to the *tokens* consumed (input + output, including search-result content loaded into context), **not** to the per-search fee.
- OpenAI web search: similar model — search usage is billed separately from token usage.

So batch delivers full savings on model reasoning, partial savings on research. The app should set this expectation in its cost UI and expose `max_uses` to cap searches per request.

---

## 4. Decisions (applied)

| Decision | Choice | Rationale |
|---|---|---|
| Primary provider | **Anthropic** (Message Batches) | Web search + citations inside batch; <1h typical latency; best fit for fact-checking |
| Design scope | **Async batch research only** | Keeps scope tight; inline feedback is a separate synchronous design later |
| Batch flush trigger | **Hybrid: count + time + manual** | e.g. flush at >=25 claims OR every 30 min OR on explicit "run research now" — balances cost, latency, control |

---

## 5. Proposed architecture (high level)

Tauri Rust core + TypeScript/web frontend. Cold path only.

1. **Claim extraction** (Rust core)
   - Triggers: `/fact-check` (block-scoped), `/research` (file or selection), idle-after-save, or background scan.
   - Parse atomic claims from markdown. Each claim becomes a queued item:
     - `{custom_id, file_path, line_range, claim_text, source_hash, tool: web_search, status: queued}`

2. **Batch accumulator** (Rust, hybrid flush policy)
   - Submit a batch when **count >= N** (start ~25) **OR** every **T minutes** (start ~30) **OR** on explicit user "run research now."
   - Single urgent claim may fall back to a **synchronous** Messages API call (outside batch) when the user wants one answer fast; batch is for the accumulated cold work.

3. **Submission** (Anthropic Message Batches)
   - Each request includes `web_search` tool enabled + `max_uses` cap per request.
   - `custom_id` = claim key so results map back to file location.
   - Capture returned `batch_id`s for tracking.

4. **Persistence** (local SQLite)
   - Tables: `claims` (custom_id -> file_path / line_range / source_hash), `batches` (id, status, created_at, completed_at), `results` (custom_id, batch_id, outcome, payload).
   - Survives restart and re-maps to on-disk state if files were changed externally (Obsidian, etc.) — satisfies the outline's agnostic-watcher / overlay constraint.

5. **Poller** (Rust background task)
   - Poll batch status -> on `ended`, stream results, map `custom_id` -> location, write to side-pane state store.
   - Handle `errored` / `expired` results: re-queue or surface failure in the side pane.

6. **`/ignore`**
   - Marks a block/claim excluded from extraction so it never batches. Persists per-block ignore flags alongside claims metadata.

7. **Stale-result handling**
   - If the file changed between submit and result (source_hash mismatch), mark the result "stale — re-run?" rather than authoritatively surfacing it.
   - Ties into the on-disk-watcher constraint: the app is an observer, not exclusive owner.

---

## 6. Key risks / tradeoffs

- **Single-claim batches are wasteful** — accumulate across files and time, or route one-offs to sync. Hybrid flush policy addresses this.
- **Search throttling** — Anthropic throttles web search per-organization in batches; large search-heavy batches slow down. Tune batch size and `max_uses` accordingly.
- **Staleness vs external edits** — results must map to on-disk state at submit time and degrade gracefully when files mutate externally.
- **API keys in a desktop app** — store in macOS Keychain, never in the repo; abstract the provider behind a trait so cost can be arbitrage-later.
- **Latency variance** — the UX must communicate the lifecycle: "queued -> in progress -> arrived," which the outline already embraces.
- **Cost transparency** — the 50% discount is on tokens, not searches; surface this in any per-research-run cost estimate so users are not surprised.

---

## 7. Implementation prerequisites

- `git init` + initial commit (repo is not yet a git repository per AGENTS.md).
- Tauri project scaffolding (no `Cargo.toml`, `package.json`, or `tauri.conf.json` exist yet).
- `opencode.json` and agent config are already in place.
- No build/test/lint commands exist yet — do not fabricate or run them until tooling is initialized.

---

## Sources (verified during research)

- OpenAI Batch API: https://platform.openai.com/docs/guides/batch (50% discount, 24h SLA, .jsonl file upload, 50k req / 200 MB caps)
- Anthropic Message Batches: https://platform.claude.com/docs/en/build-with-claude/batch-processing (50% discount, most batches <1h, 100k req / 256 MB, prompt caching stacks)
- Anthropic Web Search tool: https://platform.claude.com/docs/en/build-with-claude/tool-use/web-search-tool (supported in batch, returns citations, $10/1k searches, per-org throttling in batches)
- Outline: /Users/josephgoodsell/development/orange_yeoman/outline.md
- Project constraints: /Users/josephgoodsell/development/orange_yeoman/AGENTS.md