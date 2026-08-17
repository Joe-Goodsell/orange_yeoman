# Efficient Concept Extraction, Command Taxonomy, and Context Assembly

**Status:** Research deliverable
**Date:** 2026-08-17
**Scope:** Extraction efficiency, LLM command types, context bundling across discontinuous spans and files, and Markdown structure as a context system. Builds on `concept-extraction-and-prompting.md` and `async-batch-research-feasibility.md`. Research only; no implementation.

## Executive Summary

This document answers four questions that the prior extraction research raised but did not close:

1. **How does the app extract concepts efficiently?** Extract once per changed block, store the result, and let every command reuse it. Never re-extract per command.
2. **Which commands should the app send to the model?** Define a small command envelope with a trigger, scope, latency class, and context class. V1 ships explicit commands only: `/fact-check` and `/ignore`. Automatic commands and `/research` are roadmap. Support two invocation modes for every command: inline in the note text, and a global vim-style command line. One parser serves both.
3. **How does the app bundle context that spans files or is discontinuous within a file?** Treat context as a labeled list of typed spans, not a text blob. Freeze a context manifest at enqueue time. Mark results stale when any span changes.
4. **How does Markdown structure make this cheaper?** The vault is already a knowledge graph. Headings, files, folders, front matter, and wikilinks provide deterministic, free context before any retrieval runs.

The unifying principle: **the vault maintains a structured index, and every model request is assembled from that index.** The index is local, incremental, and cheap. The model sees only what the command's context class allows.

**V1 scope decision.** Explicit slash commands are the only v1 goal. V1 ships `/fact-check` and `/ignore` as inline commands, with the same-line focus rule and the shared command parser. Automatic parsing, the concept store, and research workloads are roadmap items. Everything below remains the target design; the phases in Section 7 schedule it.

## 1. Relationship to Prior Research

`concept-extraction-and-prompting.md` defines the extraction pipeline (Stages 0–3), claim and research schemas, prompt principles, and token budgets. `async-batch-research-feasibility.md` defines the cold batch path and the flush policy. This document extends both:

| Prior decision | Extension here |
| --- | --- |
| Staged pipeline (local rules, small model, strong model) | Add incremental diff-driven scheduling and a persistent concept store |
| Task-specific context budgets | Add a span-based context model that supports discontinuous and cross-file context |
| Slash commands as canonical surface | Add a full command taxonomy with a shared command envelope |
| Heading chain as claim context | Promote Markdown structure to a first-class context system with selectors |

## 2. Efficient Concept Extraction

**V1 scope.** With automatic processing deferred, v1 needs only Stage 0 (block parsing) and block hashing: the parser finds explicit command lines, excludes code fences and front matter, and supplies hashes for anchoring and staleness. Stages 1–3, the concept store, and the embedding index arrive with the roadmap automation. This section documents the full design so the v1 parser produces data the later stages can consume without re-parsing history.

### 2.1 Extract once, use many times

The most expensive mistake would be running extraction per command. A single block can feed a fact-check, a logic check, a link suggestion, and a research opportunity. Extraction must be a shared service with a stored result:

```text
block change -> Stage 0..3 pipeline (once) -> concept store -> all commands read
```

Every command — automatic or explicit — reads from the concept store. An explicit command may force a re-run for a stale or low-scored block, but the default path is a lookup, not a model call.

### 2.2 Incremental, diff-driven processing

Do not process files. Process changed blocks.

1. On each debounced file event, parse the file into blocks (Stage 0).
2. Hash each block. The block hash is the block identity, per prior research.
3. Diff the new block list against the stored list. A simple sequence diff on block hashes is sufficient, because Markdown edits rarely reorder unchanged blocks.
4. Classify each block: `unchanged`, `added`, `changed`, `moved`, or `removed`.
5. Enqueue only `added` and `changed` blocks into Stages 1–3.
6. Delete or re-link concepts attached to `removed` blocks. Move concepts with `moved` blocks without re-extraction.

This bounds cost to the edit size. A one-line edit in a 5,000-line vault costs one block, not one file.

`unchanged` blocks keep their concepts forever until the block changes. This is what makes the vault index amortize: most blocks in a mature vault are never processed twice.

### 2.3 The concept store

Persist the index in the Rust core (SQLite is sufficient; it is already on the Tauri stack):

| Table | Holds | Key fields |
| --- | --- | --- |
| `blocks` | one row per parsed block | `file_path`, `block_hash`, `heading_path`, `char_start`, `char_end`, `kind` |
| `concepts` | entities, claims, topics | `type`, `normalized_name`, `first_seen` |
| `occurrences` | concept-in-block | `concept_id`, `block_id`, `start`, `end` |
| `links` | wikilinks found in blocks | `source_block_id`, `target_path`, `target_heading` |
| `embeddings` | lazy vector per block | `block_hash`, `model_id`, `vector` |

Entity merging uses normalized name plus entity type, per prior research. Surface forms stay in `occurrences`; the normalized form lives once in `concepts`. This gives cross-file entity resolution without a linker service.

The store solves three problems at once: command context lookup, cross-file entity overlap, and result re-anchoring after the user edits (find the span by block hash, then by content hash inside the block).

### 2.4 Tiered compute and batching

Keep the prior three-band routing, and add scheduling rules:

- **Stage 1 (local rules) runs on every changed block.** It is cheap and synchronous with the watcher debounce.
- **Stage 2 (small extraction model) is batched.** Collect middle-band blocks across all files during a flush window (for example 30 seconds hot, or the batch accumulator's window cold) and send them as one request with multiple `<candidate_block>` entries. The schema returns units tagged by block index.
- **Stage 3 (strong model) never runs automatically on blocks.** It is reserved for command execution: research synthesis, difficult fact-checks, logic checks across sections.

Priority order when the queue backs up: explicit slash commands first, then high-band blocks in the active file, then high-band blocks elsewhere, then middle-band blocks, then background scans.

### 2.5 Lazy embeddings

Do not embed the vault eagerly. Embed a block only when it passes Stage 1 with a non-low score, and cache by block hash. Re-embed only on block change. Embedding model identity is stored with the vector so a model swap can invalidate the cache cleanly.

For phase 1 and 2, structural and lexical context (Sections 4 and 5) may make embeddings unnecessary. Treat the embedding index as a phase 3 optimization that is added only if evaluation shows retrieval gaps.

### 2.6 What efficiency buys

| Mechanism | Cost avoided |
| --- | --- |
| Block-level diffing | Re-processing unchanged text after every edit |
| Concept store | Re-extraction per command; re-resolution of entities |
| Batched Stage 2 calls | Per-block request overhead and latency |
| Lazy embeddings | Indexing text no command will ever read |
| Hash-keyed caches | All of the above after an external edit rewrites line numbers but not content |

## 3. Command Taxonomy

**V1 scope decision.** Explicit slash commands are the only goal for v1. Automatic parsing (claim detection without a command) and research workloads are deferred to the roadmap. V1 ships `/fact-check` and `/ignore` as explicit inline commands. This section still documents the full taxonomy, because the command envelope, context classes, and selectors are designed once and shared by every later phase; only the activation schedule changes.

### 3.1 Design axes

Every command — automatic or explicit — is described by the same envelope. This lets the scheduler, the context assembler, and the side pane treat commands uniformly:

```json
{
  "command": "fact_check",
  "trigger": "automatic | inline | command_line | menu",
  "scope": "span | block | section | file | vault",
  "context_class": "claim | local | structural | cross_file | vault",
  "latency_class": "hot | warm | cold_batch",
  "batch_eligible": true,
  "output_schema_version": 1,
  "focus_ref": {"file": "...", "block_hash": "...", "start": 0, "end": 43}
}
```

`focus_ref` references the concept store, not inline text. The context assembler resolves it at submission time.

Latency classes:

- **hot:** synchronous or near-synchronous; small payload. In v1 this is local processing only; automatic candidate surfacing arrives with the roadmap automation.
- **warm:** queued, seconds to minutes; single-request commands (`/fact-check`, later `/logic-check`). **This is the only latency class v1 uses.**
- **cold_batch:** the batch accumulator path from the async feasibility research; research workloads and background fact-check sweeps. Roadmap.

### 3.2 Automatic commands (roadmap)

Automatic commands fire from pipeline output without user action. **None of these ship in v1.** They are deferred with the automatic extraction pipeline (Section 2), because they depend on Stages 1–3 and the concept store. They must be conservative when they arrive; the prior research's precision-over-recall rule applies to all of them.

| Command | Fires when | Context class | Latency | Notes |
| --- | --- | --- | --- | --- |
| `fact_check` | high-confidence `factual_claim` candidate | claim | warm or cold_batch | Core feature; route through batch accumulator when not urgent |
| `logic_check` | two or more claims in the same file conflict or a claim contradicts the section topic | structural | warm | Runs on the concept store first; model call only for suspected conflicts |
| `research_opportunity` | thin section, question, or gap signal | structural | cold_batch | Typed request per prior research; never "interesting facts" |
| `relate` | entity or topic overlap between the active block and other blocks | cross_file | hot | Link suggestions from the local index; no model call needed for the candidate list, only for ranking prose |
| `terminology_check` | same normalized entity with different surface forms, or inconsistent units/dates | vault | hot | Fully local; deterministic suggestions |

`relate` and `terminology_check` are new but cheap: both read only the concept store. They give visible side-pane value on every edit without model spend.

### 3.3 Explicit slash commands

The outline makes `/research`, `/fact-check`, and `/ignore` canonical. The v1 decision narrows this: **v1 ships `/fact-check` and `/ignore` only.** `/research` stays canonical in the design but is deferred with the research workloads, because it needs the typed-opportunity machinery and the batch path for good results.

| Command | Scope | Context class | Latency | Priority | Purpose |
| --- | --- | --- | --- | --- | --- |
| `/fact-check` | line (inline default) or selection/section | claim | warm | **v1** | Verify claims; strongest-evidence search per prior prompt |
| `/ignore` | line/block/section/file/pattern | none | hot | **v1** | Suppress processing; in v1 it suppresses command execution for a span, and later automatic processing too; store rule keyed by block hash or heading path |
| `/research` | section/file/selection | structural or cross_file | cold_batch | post-v1 | Goal-driven research with typed opportunities |
| `/logic-check` | section/file | structural | warm | post-v1 | Consistency and contradiction pass over the note's own claims |
| `/relate` | block/section | cross_file | hot/warm | post-v1 | Find and suggest wikilinks to related notes |
| `/define` | term in selection | cross_file | warm | later | Definition in the note's domain, preferring the vault's own usage |
| `/expand` | section | structural + cross_file | cold_batch | later | Expansion options for a thin section; output as suggestions, never edits |
| `/outline` | file | structural | warm | later | Re-structure proposal from the heading tree and topics |
| `/summarize` | file/section | structural | warm | later | Useful for the app itself: section summaries feed document context cheaply |
| `/cite` | claim/section | claim | cold_batch | later | Find sources for an uncited claim without a verdict; weaker sibling of `/fact-check` |
| `/timeline` | file/vault | vault | cold_batch | roadmap | Arrange dated claims into a chronology; a derived view, not new knowledge |
| `/ask` | free | vault | warm | roadmap | Free question answered from vault context; needs the full Section 4 machinery |

Rules that hold across the set:

- Suggestions are options in the side pane. No command rewrites the note without explicit user confirmation.
- Every command version its output schema, per prior research.
- Automatic variants exist only where the precision rule allows, and none before the roadmap automation phase. `/expand`, `/outline`, and `/ask` are always explicit.
- `/ignore` accepts scoped arguments (`/ignore block`, `/ignore section`, `/ignore file`, `/ignore /pattern/`) and persists by hash or heading path, not line number.

### 3.4 Command discovery cost

Each command adds prompt surface, schema surface, and side-pane UI. The v1 cap is two commands (`fact-check`, `ignore`), both explicit and inline. The design rule stands for later phases: a new command must define its context class and reuse an existing output schema where possible.

### 3.5 Invocation modes: inline and global

Every explicit command has two invocation modes:

- **Inline (local):** the command is typed in the note text and becomes part of the file.
  ```text
  /fact-check amperes are of more concern in electrical safety than volts
  ```
- **Global:** the command runs on a vim-style command line in the app UI. It is not part of the file.
  ```text
  :fact-check @sel
  :research @section --goal "battles where wedge formations were used"
  ```

| Property | Inline `/command` | Global `:command` |
| --- | --- | --- |
| Friction | None; no mode switch | Brief mode switch (`:` line, `Esc` cancels) |
| Argument form | Whole line containing the command, token stripped, plus optional selector | Full grammar: selectors, flags, quoted args, ranges |
| Persisted | Yes; it is note text on disk | No; app state only, unless persisted on purpose |
| Works from external editors | **Yes**; the watcher parses it from disk state | No; in-app only |
| Auditable | Yes; the request stays in the note as plain text | Only in app history |
| Ambiguity risk | Yes; prose can imitate a command (see 3.6) | None |
| Fits non-blocking UX | Yes; the user keeps typing, results land in the side pane | Yes; execution is async either way |

The modes are not competitors. Inline is the **canonical persisted form**, because disk state is the source of truth and any tool can author it. Global is an **in-app convenience** for editor-only scopes such as `@sel`, and it aligns with the vim-keybindings roadmap.

### 3.6 Inline command rules

**Grammar.** A line is a command candidate when all of these hold:

1. The line is inside an extractable block (Stage 0 parsing; code fences and front matter are already excluded).
2. The line contains a token matching `/name` for a name in the command registry. **The command may appear anywhere in the line** — at the start, mid-sentence, or at the end. Unknown `/foo` is ordinary prose.
3. The command token must be bounded by whitespace or line boundaries, so that `/research` matches but `/researching` or file paths such as `src/main.rs` do not.
4. An optional trailing `@selector` token may follow the command or end the line. Flags such as `--goal` are discouraged inline; quoting rules in prose are error-prone. Use the global mode for flags.

Examples of equivalent invocations:

```text
/fact-check amperes are of more concern in electrical safety than volts
Question for later /fact-check amperes are of more concern in electrical safety than volts
amperes are of more concern in electrical safety than volts /fact-check
```

**Argument: the whole line is the focus.** The command applies to the entire line that contains it, not only to the text after the token. The application strips the command token (and any trailing selector) and sends the remaining line text as the claim or goal:

```text
a fact I'm not sure about /fact-check
```

becomes the focus text:

```text
a fact I'm not sure about
```

This is the rule that makes trailing commands natural: users append `/fact-check` to a sentence they just wrote, exactly as they would tag it. The model receives the full remaining line, including lead-in phrases such as `Question for later`; the extraction and fact-check prompts already tolerate framing language around a claim, and evaluation fixtures should include such lead-ins.

**First-match rule.** If a line contains more than one registry command, only the first triggers. Its argument is the whole line with that one token stripped. The other command tokens are ordinary text. Do not chain commands on one line; use separate lines.

**Scope resolution.**

- Default: the focus is the containing line's text with the command token stripped (`@line` default, not `@block`). The focus span in the file is the whole line, so results anchor and re-anchor at line granularity.
- Trailing or following selector: the selector overrides the default scope (`/research @section`). When a selector overrides the line, the line text still ships as the goal or question text for the command.

**Ship scope: same-line context only.** For the first implementation, an inline command's context is its own line: the full line text as focus, plus the `structural` span (heading chain) that Section 4 always attaches. No surrounding-paragraph expansion, no antecedent search, no multi-line arguments for inline commands. This keeps parsing deterministic and matches how users naturally write a one-line ask. Larger context needs are served by the global mode with selectors, or by a trailing `@selector` on the inline line. Multi-line inline arguments stay an open question (Section 6).

**Text lifecycle.** The command line stays in the file. The app never deletes it, moves it, or edits around it. This obeys the agnostic-watcher constraint: the note remains plain, portable text in every tool. Results go to the side pane and anchor to the command line through its span and hash, the same way automatic results anchor to claim spans.

**Task identity and staleness.** The task identity is the hash of the command name, the stripped line text, and the resolved scope. When the user edits the command line, the submitted hash no longer matches and the result is marked stale, exactly as with any other focus span. When the user deletes the line, the result is stale with no re-anchor target.

**Ambiguity and false positives.** A note about the app can contain the literal text `/fact-check`, now also mid-sentence. Mitigations, in order:

- Whitespace-boundary token matching (rule 3 above) removes the most common false positives: paths, URLs, and inflections.
- Stage 0 excludes code fences, so documentation examples in code blocks never trigger.
- A one-line confirmation affordance in the side pane for inline commands discovered by the watcher in files not currently open: "Run this command?" Commands typed in the active editor run directly, because the user just typed them.
- `/ignore` accepts the command line as a block, so users can suppress a quoted example permanently.

**Async behavior.** Inline commands never block the editor. The line stays where it is; the side pane shows the command as queued, running, or done. This matches the non-blocking UX requirement and gives the linked-feedback UI a stable anchor in the file.

### 3.7 Global command rules

**Grammar.** The `:` line supports the full grammar that inline mode avoids:

```text
:fact-check @sel
:research @section --goal "..." --depth deep
:'<,'>fact-check        (visual selection range)
:fact-check! @sel       (bang: persist as inline, see below)
```

Ranges use marks or the visual selection. Line-number ranges are not offered, because line numbers are not stable identities (Section 2.2).

**Resolution.** Editor-only selectors such as `@sel` resolve to concrete spans at submission time and are recorded in the context manifest like any other span. After submission, the command is indistinguishable from an inline one: same envelope, same scheduler, same side-pane entry.

**Persistence.** A global command is ephemeral by default. The `!` variant writes the equivalent inline form into the note at the cursor (or as a line above the selection), so the request becomes part of the file, survives app restarts, and stays auditable. This follows the vim convention for destructive-or-persisting variants. Default stays ephemeral, because writing to the file must stay an explicit user choice.

### 3.8 One parser, one envelope

Both modes feed one tokenizer and one registry:

- Inline mode = restricted grammar: `/name` + free text + optional trailing selector.
- Global mode = full grammar: `:name` + selectors + flags + ranges + `!`.

Both produce the same command envelope (Section 3.1), so the scheduler, context assembler, and side pane never learn which mode produced a task. The watcher parses inline commands from disk state during Stage 0, which is how commands authored in Obsidian or any external editor execute with no extra mechanism.

## 4. Context Assembly

### 4.1 Context is a list of typed spans

The prior research selects context by task. This document generalizes it: context is an ordered, labeled list of spans, each with a role and provenance:

```json
{
  "spans": [
    {"role": "focus", "file": "notes/physics.md", "block_hash": "a1...", "start": 0, "end": 43, "text": "..."},
    {"role": "local_window", "file": "notes/physics.md", "block_hash": "a1...", "start": 44, "end": 210, "text": "..."},
    {"role": "structural", "file": "notes/physics.md", "heading_path": ["Properties", "Bismuth"]},
    {"role": "retrieved", "file": "notes/metallurgy.md", "block_hash": "b7...", "start": 900, "end": 1150, "text": "..."},
    {"role": "background", "file": "notes/metallurgy.md", "heading_path": ["Smelting"]}
  ]
}
```

Roles:

| Role | Content | Always present |
| --- | --- | --- |
| `focus` | The claim, selection, or section the command targets | yes |
| `local_window` | Surrounding sentences or the containing block | yes |
| `structural` | Heading chain, file title, front-matter tags | yes |
| `retrieved` | Spans pulled from other blocks or files | no |
| `background` | Outlines, summaries, entity lists that frame the retrieved spans | no |

Provenance is non-negotiable: user note text and web text never share a tag, per the injection rules in prior research. Each span keeps its file and heading path so the model — and the side pane — can cite where text came from.

### 4.2 Discontinuous context within one file

Context is often discontinuous: a pronoun needs an antecedent three paragraphs up; an argument spans a list and a later "Therefore" paragraph; a term is defined in an earlier section.

Mechanism:

1. **Attach the structural span always.** The heading chain carries topic and scope even when text spans are far apart.
2. **Apply bounded expansion rules.** Backward antecedent search: when Stage 1 or the extraction model flags anaphora, expand backward sentence by sentence (bounded, for example five sentences) until a candidate antecedent appears, then stop. List capture: when the focus is one list item, offer sibling items as an expansion rule, not a default. Definition lookup: resolve a term through the concept store's `occurrences` and include the definition block as a `retrieved` span.
3. **Merge adjacent or overlapping spans.** Two spans closer than a small gap (for example one blank line) merge into one. Packing fewer, larger spans beats packing many fragments.
4. **Mark gaps explicitly.** When spans cannot merge, join them with an omission marker:

```text
<local_context file="notes/physics.md">
Bismuth has a low melting point for a metal. [... 14 lines omitted ...]
This is why it is used in fusible alloys.
</local_context>
```

The marker tells the model the context is discontinuous. Without it, models tend to infer a relationship between the joined fragments that does not exist.

5. **Preserve original order.** Reordering spans within a file risks destroying discourse structure. Order spans by offset; only roles reorder at the prompt level (Section 4.4).

### 4.3 Multi-file context

Cross-file context candidates come from four generators, in cost order:

1. **Structural edges — free and high precision.** Wikilinks, backlinks (from the `links` table), same folder, and the file's own heading path. If the user linked two notes, that link is context.
2. **Entity overlap — cheap.** Blocks sharing normalized concepts with the focus, from `occurrences`. The `relate` command is this generator plus ranking.
3. **Lexical search — cheap.** A block-level lexical index (title, headings, and rare terms weighted up) over the concept store. For a personal vault this alone is strong.
4. **Embedding similarity — lazy.** Only if enabled, and only over embedded blocks (Section 2.5).

Assembly:

- Run generators, score candidates (`structural` edges weigh highest), and pack under the command's context-class budget.
- Group packed spans by file, each under a header with file path and heading path:

```text
<retrieved_context>
From notes/metallurgy.md, section "Smelting":
Bismuth melts at 271.4 C [...]
From notes/history.md, section "Alloys":
Fusible alloys appeared in [...]
</retrieved_context>
```

- Cap spans per file before capping files, so one long note does not eat the budget.
- If a candidate file is small and strongly linked, include its outline as `background` instead of spans. Outlines cost little and anchor the model.

### 4.4 Packing order

Long-context degradation ("lost in the middle") applies to assembled context as much as to long documents. Assemble in this order:

1. System/hidden prompt (versioned, cached prefix).
2. `background` outlines and the vault or file outline.
3. `retrieved` spans, grouped by file.
4. `local_window` spans.
5. `structural` heading chain.
6. `focus` spans.
7. Task instructions and the output contract, last.

Stable prefix ordering also lets prompt caching work: the hidden prompt and outlines rarely change between requests in a session.

### 4.5 Snapshots and staleness across files

Prior research marks results stale when the source text changes. Multi-file context makes this stricter:

- At enqueue time, freeze a **context manifest**: the span list plus the block hash of every span.
- Store the manifest with the task (batch tasks especially; the batch window is long).
- On result arrival, re-read each file in the manifest and compare hashes.
- If any `focus` or `local_window` span changed, mark `stale` and offer a re-run.
- If only a `retrieved` span changed, mark `partially_stale` and show which sources moved. The verdict may still hold; the app should say what it was based on.

This keeps the side pane honest during the long batch window, which is exactly when notes change.

### 4.6 Budget classes

Extending the prior budget table to the new context classes:

| Context class | Typical use | Initial budget | Expansion rule |
| --- | --- | ---: | --- |
| `claim` | fact-check one claim | 1,500 tokens | Expand only for anaphora or ambiguity |
| `local` | logic-check within a section | 3,000 tokens | Add sibling list items or the definition block |
| `structural` | section research, `/outline` | 4,000 tokens | Add adjacent sections by heading distance |
| `cross_file` | `/relate`, `/define` | 4,000 tokens | Add structural edges before lexical hits |
| `vault` | `/ask`, `/timeline` | 8,000 tokens | Staged workers per prior research; outlines before spans |

## 5. Markdown Structure as a Context System

The outline commits the app to a plain `.md` folder watched from disk. This constraint is an advantage: Markdown already encodes a context graph, and every tool in the ecosystem (including Obsidian) writes it in a parseable form.

### 5.1 The structure graph

| Structure element | Context meaning | Cost |
| --- | --- | --- |
| Heading chain | Topic scope; default context frame for any span under it | free (parser) |
| File name | Title and coarse topic | free |
| Folder path | Grouping signal; coarse domain | free |
| Front matter | Tags, aliases, dates; filtered metadata | free (parser) |
| Wikilink `[[note]]`, `[[note#heading]]` | Explicit cross-file edge; strongest relation the vault records | free |
| Tag `#tag` | Lightweight cross-file grouping | free |
| Block types (list, table, quote, code) | Processing rules: code excluded, quotes attributed, tables claim-dense | free |

Every element above is available from a disk parse with no model call. The design rule that follows: **structural context before retrieval, always.** Retrieval fills gaps the structure leaves; it does not replace structure.

### 5.2 Headings as the default context frame

The heading chain replaces retrieval for most single-file commands:

- A fact-check on a claim uses the claim's heading chain as `structural` context (prior research already specifies this).
- `/logic-check` on a section takes the section subtree as its scope, bounded by the next heading of equal or higher level.
- `/research` on a section carries the section plus its ancestors as scope, and sibling sections as optional `background`.

A document outline is therefore built locally from the heading tree and never costs tokens beyond the outline text itself.

### 5.3 Files and folders as context

- The file is the natural scope for `/outline` and `/summarize`.
- The folder is a first-class `relate` signal: notes in the same folder share a domain more often than not.
- Deep-link syntax (`file.md#heading`) should resolve inside command arguments so users can point commands at remote sections (Section 5.5).

### 5.4 Front matter and wikilinks

- Front matter is metadata, not prose. Filter to known keys (`tags`, `aliases`, `dates`) before it enters any prompt. Never send raw front matter blocks.
- Wikilinks are the highest-precision cross-file edges. Two consequences:
  1. Backlinks of the focus block's file are the first `retrieved` candidates for `cross_file` commands.
  2. `relate` should suggest missing links, not only report existing ones. A suggested link that the user accepts improves the structure graph for every future command — the vault teaches the app its own context.

### 5.5 A selector mini-language

Commands need a uniform way to name context. Selectors resolve through the concept store and structure graph, never through line numbers:

| Selector | Resolves to |
| --- | --- |
| `@sel` | Current editor selection (in-app only) |
| `@block` | Block containing the cursor |
| `@section` | Nearest heading section |
| `@file` | Current file |
| `@path/to/note.md` | A specific file |
| `@note.md#Heading` | A specific section in another file |
| `@backlinks` | Blocks that link here |
| `@tag(x)` / `@entity(Name)` | Blocks carrying a tag or concept |

Examples:

```text
/fact-check @sel
/research @section --goal "battles where wedge formations were used"
/logic-check @file
/relate @section
/ask "what did I write about fusible alloys?" @tag(metallurgy)
```

Selectors also serve as the internal scope notation for automatic commands, so automatic and explicit paths share one resolution mechanism.

### 5.6 Why structure is the safe choice under external editing

The app must react to disk state from any editor. Structure from a disk parse is identical regardless of which tool wrote the file. Editor selection (`@sel`) exists only inside the app. Therefore:

- Structural selectors are the canonical machine context.
- `@sel` is a convenience that resolves to spans at submission time and is recorded in the manifest like any other span.

This keeps `/fact-check` from Obsidian-authored files equivalent to `/fact-check` from the built-in editor.

### 5.7 Failure modes

| Failure | Mitigation |
| --- | --- |
| Long file with no headings | Fall back to paragraph windows and lexical retrieval; suggest `/outline` |
| Headings that do not match content | Heading chain is context, not truth; the model sees the text too |
| Flat vault (no folders, no links) | Lean on entity overlap and lexical search; suggest `relate` and `/outline` |
| Decorative or duplicate headings | Heading chain plus block hash identifies scope; dedupe by path, not title |
| Front matter with unexpected keys | Allow-list filter; unknown keys never reach a prompt |

## 6. Open Questions

1. **Local NLP home.** spaCy sidecar versus a small-model-only Stage 1 versus Rust-native rules. A sidecar adds packaging cost on macOS; a small-model Stage 1 adds latency and spend. Decide with the fixture set from prior research.
2. **Are embeddings needed at all?** Structural plus lexical context may cover personal vaults. Defer the embedding index until evaluation shows a retrieval gap that structure cannot close.
3. **`partially_stale` presentation.** How should the side pane show which retrieved sources moved under a result?
4. **Ignore persistence.** Block-hash rules break when the user edits an ignored block. Decide between re-anchoring ignores by similarity or expiring them with a notice.
5. **Selector syntax stability.** The mini-language becomes user-facing API. Freeze its grammar before the first release.
6. **Context assembly evaluation.** Extend the fixture set with multi-file and discontinuous cases, and measure whether structural context matches or beats retrieval at equal budget.
7. **Inline multi-line arguments.** The first implementation is same-line only (Section 3.6). Decide later whether `/research` accepts a goal that runs to the next blank line, and how the parser shows where the argument ends.
8. **Automatic inline confirmation.** Inline commands found by the watcher in externally edited files cannot ask "did you mean this?" before running. Decide the default: run on parse, or queue until the user opens the file and confirms.
9. **Bang-persist default.** `:command!` writes the inline form into the note. Confirm that users want opt-in persistence rather than opt-out, and where the written line lands.

## 7. Recommended Sequencing

| Phase | Work |
| --- | --- |
| **v1** | Stage 0 parser with block hashes; inline command detection (anywhere in line, whole-line focus); command envelope with `trigger: inline`; span manifests and staleness for command lines; `/fact-check` and `/ignore`; side-pane results anchored to command lines |
| 2 | Global `:` command line with full grammar, ranges, and `!` persistence; `/research` on the batch path; gap-marker packing; `partially_stale` |
| 3 | Automatic extraction (Stages 1–3), concept store, and the automatic commands from Section 3.2; `/logic-check` and `/relate`; `links` table and backlink candidates; batched Stage 2 extraction |
| 4 | Remaining commands (`/define`, `/expand`, `/outline`, `/summarize`, `/cite`); staged workers for `vault` context; lazy embeddings if evaluation requires them; `/timeline`, `/ask` |

V1 is deliberately minimal: it ships the parser, the command grammar, the envelope, and the staleness machinery on two explicit commands. This exercises the watcher path and the side-pane loop without any automatic model spend. Automation and research build on the same structures later.

## Conclusion

Efficient extraction is an indexing problem, not a prompting problem. The app should parse blocks on change, hash them, extract concepts once, and serve every command from the stored index. Commands share one envelope and one span-based context model, which makes discontinuous and cross-file context a packing problem with explicit provenance. Every command has two invocation modes with one parser: inline commands live in the note text and work from any editor; global commands run on a vim-style line with the full grammar. Markdown structure — headings, files, folders, front matter, and wikilinks — supplies most context for free and stays correct under external editing. Retrieval and embeddings are late additions for gaps the structure graph leaves. V1 ships none of the automation: it ships `/fact-check` and `/ignore` as explicit inline commands on the Stage 0 parser, and earns trust before any model runs without being asked.

## Sources

- Orange Yeoman concept extraction and prompting: `concept-extraction-and-prompting.md`
- Orange Yeoman async batch research: `async-batch-research-feasibility.md`
- Orange Yeoman detached backend: `detached-backend-feasibility.md`
- Orange Yeoman project outline: `../outline.md`
- Liu et al., Lost in the Middle: How Language Models Use Long Contexts: https://arxiv.org/abs/2307.03172
- Anthropic, Effective context engineering for AI agents: https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents
- CommonMark specification: https://spec.commonmark.org
- Obsidian, Internal links: https://help.obsidian.md/Linking+notes+and+files
