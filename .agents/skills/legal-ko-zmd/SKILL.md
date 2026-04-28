---
name: legal-ko-zmd
description: >-
  Search Korean laws and court precedents using zmd hybrid search (full-text +
  vector + reranking). Uses locally indexed collections from legalize-kr and
  precedent-kr GitHub repositories. Faster and more accurate than HTTP-based
  legal-ko-cli searches — supports semantic queries, context snippets, and
  direct document retrieval. USE FOR: search Korean law, search precedent,
  hybrid search, semantic search, 법률 검색, 판례 검색, 관련 법 찾아줘,
  관련 판례 찾아줘, find law by content, find precedent by topic, zmd search,
  zmd query. DO NOT USE FOR: legal advice or interpretation (always add a
  disclaimer), non-Korean jurisdictions, creating or editing files, TUI
  navigation (use legal-ko-search for navigate commands), 법조인 person search
  (use legal-ko-precedent for person index lookups).
license: MIT
metadata:
  author: legal-ko contributors
  version: "2.0.0"
---

# Korean Law & Precedent Search via zmd

Search Korean laws and court precedents using `zmd` — a local hybrid search
engine that combines full-text search (FTS), vector semantic search, and
reranking over locally indexed markdown collections.

| What | Answer |
|------|--------|
| **Tool** | `legal-ko-cli zmd` (native query, v0.5.6+) — preferred; `zmd` (v0.4.1) subprocess as fallback |
| **Data source** | Two collections indexed locally from GitHub |
| **Collections** | `laws` → [legalize-kr](https://github.com/legalize-kr/legalize-kr), `precedents` → [precedent-kr](https://github.com/legalize-kr/precedent-kr) |
| **Scope** | Laws: 법률 (Acts) only (~1,711 docs). Precedents: 민사 + 형사 대법원 by default (~43K docs); expandable to all 8 case types + 하급심 (~123K docs). |
| **Search modes** | Native: FTS (`zmd search`), hybrid FTS+vec0 (`zmd query`). Subprocess fallback: `zmd query --rerank` for cross-encoder reranking |
| **Output** | Markdown by default; `--json`, `--csv`, `--md` flags available |
| **Language** | Respond in the same language the user used (Korean or English) |

## Setup

### Prerequisites

`zmd` must be installed and on `$PATH`. Verify:

```bash
zmd version
```

### Building the Index

Collections are built using `legal-ko-cli zmd` subcommands (or Taskfile tasks),
which:

1. Clone the upstream repos (shallow) into `~/.cache/legal-ko/zmd/repos/`
2. Stage filtered files as hardlinks into `~/.cache/legal-ko/zmd/stage/`
3. Index via the native Rust indexer (writes directly to `.qmd/data.db`)
4. Skip unchanged files by stored source metadata (size + mtime) — re-runs are instant

```bash
# From the legal-ko workspace root:

# Index laws only (법률 ~1,711 docs)
task zmd:laws

# Index default precedents (민사+형사 대법원 ~43K docs)
task zmd:precedents

# Index ALL precedent case types and courts (~123K docs)
task zmd:precedents:all

# Index a specific case type with both court levels
task zmd:precedents:일반행정
task zmd:precedents:세무

# Index everything (laws + default precedents)
task zmd
```

> **Resumable:** If interrupted (Ctrl-C, timeout, etc.), re-run the same command.
> Already-indexed files are skipped automatically (source metadata check).

> **Timing:** First-run indexing speed depends on file count and hashing/embedding.
> Laws (~1,711 files) takes ~5-10 min. Default precedents (~43K files) takes
> ~30-60 min. Full precedents (~123K files) takes several hours.
> Re-runs with no changes complete in under 2 seconds.

Check progress at any time:

```bash
task zmd:status
# or
zmd status
```

### Collection Scope

The default configuration indexes a curated subset:

| Collection | Scope | File count | Source |
|-----------|-------|-----------|--------|
| `laws` | 법률 (Acts) only | ~1,711 | `legalize-kr` — `kr/*/법률.md` |
| `precedents` | 민사 + 형사, 대법원 only | ~43,491 | `precedent-kr` — `{민사,형사}/대법원/*.md` |

**Expandable via Taskfile tasks:**

| Task | Case type | Courts | ~Files |
|------|-----------|--------|--------|
| `task zmd:precedents` | 민사, 형사 | 대법원 | 43K |
| `task zmd:precedents:민사` | 민사 (civil) | 대법원 + 하급심 | 42K |
| `task zmd:precedents:형사` | 형사 (criminal) | 대법원 + 하급심 | 22K |
| `task zmd:precedents:일반행정` | 일반행정 (admin) | 대법원 + 하급심 | 45K |
| `task zmd:precedents:세무` | 세무 (tax) | 대법원 + 하급심 | 10K |
| `task zmd:precedents:특허` | 특허 (patent) | 대법원 + 하급심 | 3K |
| `task zmd:precedents:가사` | 가사 (family) | 대법원 + 하급심 | 1.4K |
| `task zmd:precedents:선거` | 선거·특별 (election) | 하급심 | 8 |
| `task zmd:precedents:기타` | 기타 (other) | 대법원 + 하급심 + 미분류 | 11 |
| `task zmd:precedents:all` | **All 8 case types** | **All courts** | **~123K** |

**Not indexed by default:**
- Laws: 대통령령 (시행령), 부령 (시행규칙), and other regulation types
- Precedents: 일반행정, 세무, 특허, 가사, 선거·특별, 기타 case types
- Precedents: 하급심 (lower court) rulings

To expand scope, run the corresponding Taskfile task or pass `--case-type` and
`--court` flags directly:

```bash
# Via Taskfile
task zmd:precedents:일반행정

# Via CLI directly
legal-ko-cli zmd precedents --case-type 일반행정 --court 대법원 --court 하급심
```

### Syncing Updates

The upstream repositories are updated periodically. To pull new/changed
documents and re-index:

```bash
task zmd:sync
```

This does a `git pull` on both repos, re-stages any new/changed files, and
re-indexes (incremental — only changed files are processed, unchanged files
are skipped by stored source metadata).

## When to Use

- User asks a legal question and you need to search across law **content** (not just titles)
- User describes a situation in natural language and needs matching laws or precedents
- User wants semantic search ("find laws about tenant protection" rather than exact title match)
- User needs context-rich snippets showing where matches occur in documents
- You need to retrieve a specific document by its collection path
- You want faster search than HTTP-based `legal-ko-cli` (local index vs GitHub API)

### When NOT to Use

- **TUI navigation** — Use `legal-ko-cli navigate` (from legal-ko-search skill)
- **법조인 person search** — Use `legal-ko-cli precedent-search-person` (from legal-ko-precedent skill)
- **Bookmarks, context, speak** — Use `legal-ko-cli` subcommands
- **Cross-reference (precedent→laws, law→precedents)** — Use `legal-ko-cli precedent-laws` / `law-precedents` (from legal-ko-precedent skill)

> **Complementary tools:** zmd excels at discovery (finding relevant documents).
> Once you identify a document, use `legal-ko-cli show` / `precedent-show` for
> structured JSON output with parsed metadata, or use `zmd get` for raw markdown.

## Search Commands

### 1. Full-Text Search (`search`)

Keyword-based search using the FTS index. Best for exact terms, law names,
case numbers, and specific legal terminology.

```bash
zmd search "주택임대차보호법" --json
zmd search "손해배상" laws --json        # search only the laws collection
zmd search "2000다10048" precedents      # search only precedents
```

**Syntax:** `zmd search <query> [collection] [--json|--csv|--md] [--sort=score|--sort=index]`

- The optional `collection` argument restricts search to a single collection (`laws` or `precedents`)
- Default sort is by relevance score

### 2. Vector Semantic Search (`vsearch`)

Embedding-based semantic search. Best for natural language queries, conceptual
searches, and finding documents by meaning rather than exact keywords.

```bash
zmd vsearch "전세 보증금을 돌려받지 못할 때 어떤 법이 적용되나요" --json
zmd vsearch "wrongful termination employment law Korea" --json
```

**Syntax:** `zmd vsearch <query> [--json|--csv|--md] [--sort=score|--sort=index]`

### 3. Hybrid Search (`query`)

Combines FTS and vector search for best results. Use `--expand` for query
expansion and `--rerank` for result reranking.

```bash
zmd query "임대차 보증금 반환" --json
zmd query "부당해고 구제" --expand --rerank --json
```

**Syntax:** `zmd query <query> [--expand] [--rerank] [--json|--csv|--md] [--sort=score|--sort=index]`

> **Recommended default:** Use `legal-ko-cli zmd query` (native) for most searches.
> It runs in ~410 ms in-process. Only use `zmd query --rerank` (subprocess, ~1.5 s)
> when native results seem poor for ambiguous semantic queries.

### 4. Context Snippets (`context`)

Returns search results with surrounding context — useful for seeing exactly
where and how a term appears in a document.

```bash
zmd context "대항력" --json
zmd context "제840조" --json
```

**Syntax:** `zmd context <query> [--json|--csv|--md] [--sort=score|--sort=index]`

## Document Retrieval

### Get a Single Document (`get`)

Retrieve a document by its collection path:

```bash
zmd get "laws/kr/민법/법률.md"
zmd get "precedents/민사/대법원/2000다10048.md"
```

**Syntax:** `zmd get <collection/path>` or `zmd get zmd://collection/path`

### Get Multiple Documents (`multi-get`)

Retrieve several documents at once:

```bash
zmd multi-get "laws/kr/민법/법률.md" "laws/kr/형법/법률.md" --json
```

**Syntax:** `zmd multi-get <doc-ref...> [--json|--csv|--md]`

### List Documents (`ls`)

Browse the document index:

```bash
zmd ls
```

## Workflow

### Phase 1 — Understand the Question

Parse the user's question to identify:

1. **Legal topic** — What area of law?
2. **Document type** — Are they looking for a statute, a court ruling, or both?
3. **Specificity** — Do they have a specific law/case name, or a general question?

### Phase 2 — Choose Search Strategy

> **Default: always use the native path** (`legal-ko-cli zmd search` or `legal-ko-cli zmd query`).
> These run in-process (~10–410 ms) vs the zmd subprocess (~1.5 s per call).
> Only fall back to `zmd query --rerank` when native results seem irrelevant.

| Situation | Strategy |
|-----------|----------|
| User names a specific law or case number | `legal-ko-cli zmd search <name> --json` (FTS-only, ~10 ms) |
| User describes a legal situation | `legal-ko-cli zmd query <q> --json` (hybrid FTS+vector, ~410 ms) |
| User wants precedents AND the laws they cite | `legal-ko-cli zmd similar <q> --json` (single call, ~30–410 ms) |
| Native results seem poor / ambiguous semantic query | `zmd query <q> --rerank --json` (subprocess fallback, ~1.5 s, cross-encoder reranking) |
| Need to see match context | `zmd context <q> --json` (subprocess) |
| Need a specific document | `zmd get <path>` (subprocess) |

### Phase 3 — Search and Discover

Run the appropriate search commands. Use `--collection` to scope results:

```bash
# Search laws only (native FTS, ~10 ms)
legal-ko-cli zmd search "근로기준" --collection laws --json

# Search precedents only (native FTS, ~10 ms)
legal-ko-cli zmd search "부당해고" --collection precedents --json

# Hybrid search everything (native FTS+vector, ~410 ms)
legal-ko-cli zmd query "부당해고 관련 법률과 판례" --json
```

**Multiple searches** — Run 2-5 searches with different terms to ensure coverage,
just as with legal-ko-cli. Korean legal terminology can be formal; translate
colloquial terms to legal language.

### Phase 4 — Read Full Documents

For the most relevant results, retrieve the full document:

```bash
# Via zmd (raw markdown)
zmd get "laws/kr/근로기준법/법률.md"

# Or via legal-ko-cli (structured JSON with parsed metadata)
legal-ko-cli show "kr/근로기준법/법률" --json
```

### Phase 5 — Cross-Reference (if needed)

For cross-referencing between statutes and precedents, hand off to `legal-ko-cli`:

```bash
# Find laws cited by a precedent
legal-ko-cli precedent-laws "민사/대법원/2000다10048" --json

# Find precedents citing a law
legal-ko-cli law-precedents "민법" --article "제840조" --json --limit 20
```

### Phase 6 — Present Findings

Structure the response as:

1. **Summary** — One-paragraph answer
2. **Relevant Documents** — Table with document path, title, and relevance
3. **Key Excerpts** — Quoted passages from the most relevant sections
4. **Cross-References** — Links between statutes and precedents (if applicable)
5. **Disclaimer** — Always include (see below)

### Disclaimer (Required)

**Always** append one of these disclaimers:

Korean:
> ⚠️ 이 정보는 법률/판례 검색 결과이며, 법률 자문이 아닙니다.
> 구체적인 사안은 반드시 변호사 등 전문가와 상담하세요.

English:
> ⚠️ This is a search result from Korean legislation and court precedents,
> not legal advice. Consult a licensed attorney for advice on your specific
> situation.

## Collection Path Reference

### Laws Collection (`laws`)

Document paths follow the pattern: `laws/kr/{법령명}/법률.md`

Only 법률 (Acts passed by the National Assembly) are indexed. 대통령령,
부령, and other regulation types are excluded to keep the index focused.

| Example Path | Description |
|-------------|-------------|
| `laws/kr/민법/법률.md` | Civil Act |
| `laws/kr/형법/법률.md` | Criminal Act |
| `laws/kr/근로기준법/법률.md` | Labor Standards Act |
| `laws/kr/주택임대차보호법/법률.md` | Housing Lease Protection Act |

### Precedents Collection (`precedents`)

Document paths follow the pattern: `precedents/{사건종류}/{법원명}/{사건번호}.md`

Only 민사 (civil) and 형사 (criminal) Supreme Court (대법원) cases are indexed
by default (~43K docs). Expand scope using Taskfile tasks (see Collection Scope above).

| Example Path | Description |
|-------------|-------------|
| `precedents/민사/대법원/2000다10048.md` | Civil Supreme Court case |
| `precedents/형사/대법원/2020도12017.md` | Criminal Supreme Court case |
| `precedents/일반행정/대법원/2019두30304.md` | Admin Supreme Court case (after expansion) |
| `precedents/민사/하급심/2015나12345.md` | Civil lower court case (after expansion) |

> **Not indexed by default:** 일반행정, 세무, 특허, 가사, 선거·특별, 기타 case
> types, and all 하급심 (lower court) cases. Run `task zmd:precedents:all` to
> index everything, or individual tasks like `task zmd:precedents:세무`.

## Topic → Search Term Reference

Reuse the same topic maps from the legal-ko-search and legal-ko-precedent
skills. The key difference: zmd searches **document content**, not just titles,
so natural language queries work better here.

| Topic | FTS Terms | Semantic Query |
|-------|-----------|---------------|
| 전세, 월세 | `임대차`, `보증금`, `주택` | "전세 보증금 반환 관련 법률" |
| 이혼, 양육권 | `이혼`, `양육`, `가사` | "이혼 시 양육권 관련 규정" |
| 부당해고 | `해고`, `근로`, `부당` | "직장에서 부당하게 해고당했을 때" |
| 교통사고 | `교통`, `손해배상` | "교통사고 피해자 보상 관련 법" |
| 사기, 횡령 | `사기`, `횡령`, `배임` | "사기 피해를 당했을 때 적용되는 형법" |
| 저작권 | `저작권`, `침해` | "블로그 글을 무단 복제한 경우" |
| 세금 | `부과처분`, `국세` | "세금 부과 처분에 이의 제기" |

## Combining zmd with legal-ko-cli

zmd and legal-ko-cli are complementary. Use this decision table:

| Task | Tool |
|------|------|
| **Discovery** — find relevant docs by topic | `legal-ko-cli zmd query --json` (native, ~410 ms) |
| **Content search** — search inside documents | `legal-ko-cli zmd search --json` (native, ~10 ms) |
| **Semantic reranking** — ambiguous natural language | `zmd query --rerank --json` (subprocess fallback, ~1.5 s) |
| **Context snippets** — see surrounding text | `zmd context --json` (subprocess) |
| **Structured metadata** — parsed frontmatter, sections | `legal-ko-cli show --json` / `precedent-show --json` |
| **Article listing** — list articles in a law | `legal-ko-cli articles --json` |
| **Cross-reference** — statute↔precedent links | `legal-ko-cli precedent-laws` / `law-precedents` |
| **Person search** — judges, attorneys | `legal-ko-cli precedent-search-person` |
| **TUI navigation** — open law in TUI | `legal-ko-cli navigate` |
| **Bookmarks** | `legal-ko-cli bookmarks` |

**Typical combined workflow:**

```bash
# 1. Discover precedents + their cited laws in one call (~30–410 ms)
legal-ko-cli zmd similar "전세 보증금 반환" --json

# 2. Get structured data for a specific law found above
legal-ko-cli show "kr/주택임대차보호법/법률" --json
legal-ko-cli articles "kr/주택임대차보호법/법률" --json

# 3. Cross-reference
legal-ko-cli law-precedents "주택임대차보호법" --article "제3조" --json --limit 10

# 4. Navigate in TUI
legal-ko-cli navigate "kr/주택임대차보호법/법률" --article "제3조"
```

## CLI & Taskfile Reference

### `legal-ko-cli zmd` subcommands

| Subcommand | Purpose |
|------------|---------|
| `query` | **Native hybrid search** (FTS5 + vec0 + RRF, ~410 ms) — preferred for discovery |
| `search` | **Native FTS-only search** (~10 ms) — preferred for keyword/exact lookups |
| `similar` | **Similarity pipeline** (query → precedents → cited laws, ~30–410 ms) — find precedents and their cited statutes in one call |
| `laws` | Clone legalize-kr, stage 법률.md files, index via native indexer |
| `precedents` | Clone precedent-kr, stage + index precedents (default: 민사+형사 대법원) |
| `all` | Run both `laws` and `precedents` phases |
| `sync` | Pull latest from upstream repos + re-stage + re-index |
| `status` | Show repos, staged files, collection document counts |
| `reset` | Remove collections and staged data (keeps repo clones) |

Precedent scope is controlled via `--case-type` and `--court` flags (repeatable).

### Taskfile tasks

| Task | Equivalent CLI | Purpose |
|------|---------------|---------|
| `task zmd` | `legal-ko-cli zmd all` | Laws + default precedents |
| `task zmd:laws` | `legal-ko-cli zmd laws` | Index laws (~1,711 files) |
| `task zmd:precedents` | `legal-ko-cli zmd precedents` | Default precedents (~43K) |
| `task zmd:precedents:all` | `legal-ko-cli zmd precedents --case-type ... --court ...` | All 8 case types + all courts (~123K) |
| `task zmd:precedents:민사` | `legal-ko-cli zmd precedents --case-type 민사 --court 대법원 --court 하급심` | Civil (~42K) |
| `task zmd:precedents:형사` | — | Criminal (~22K) |
| `task zmd:precedents:일반행정` | — | Administrative (~45K) |
| `task zmd:precedents:세무` | — | Tax (~10K) |
| `task zmd:precedents:특허` | — | Patent (~3K) |
| `task zmd:precedents:가사` | — | Family (~1.4K) |
| `task zmd:precedents:선거` | — | Election/special (~8) |
| `task zmd:precedents:기타` | — | Other (~11) |
| `task zmd:sync` | `legal-ko-cli zmd sync` | Pull + re-index |
| `task zmd:status` | `legal-ko-cli zmd status` | Show status |
| `task zmd:reset` | `legal-ko-cli zmd reset` | Remove collections |

### How Indexing Works

1. Clone/pull upstream repo (shallow) into `~/.cache/legal-ko/zmd/repos/`
2. Stage matching files as **hardlinks** into `~/.cache/legal-ko/zmd/stage/` (zero disk overhead)
3. **Pre-filter**: compare each staged file's size + mtime against stored `source_size`/`source_mtime_ns` in the database — unchanged files are skipped before any read or hash
4. **Index**: changed/new files are read, hashed (SHA-256), embedded (fnv_embed), and written to `.qmd/data.db` via the native Rust indexer
5. **Bulk mode**: FTS5 triggers are dropped during indexing and rebuilt afterward; prepared statements and batch inserts minimize SQLite overhead

This means:
- First run reads and indexes all files (minutes to hours depending on count)
- Re-runs with no upstream changes complete in **under 2 seconds** (metadata-only check, no file reads)
- Safe to interrupt — re-run picks up where it left off

### Directory Layout

```
~/.cache/legal-ko/zmd/
  repos/
    legalize-kr/          ← shallow git clone
    precedent-kr/         ← shallow git clone
  stage/
    laws/                 ← hardlinks to repos/legalize-kr/kr/*/법률.md
      kr/민법/법률.md
      kr/형법/법률.md
      ...
    precedents/           ← hardlinks to repos/precedent-kr/{민사,형사}/대법원/*.md
      민사/대법원/2000다10048.md
      형사/대법원/2020도12017.md
      ...
```

## Error Handling

| Error | Cause | Resolution |
|-------|-------|------------|
| `command not found: zmd` | zmd not installed | Install zmd and add to `$PATH` |
| `Failed to open database` | Database not initialized | Run `task zmd:laws` (or `task zmd`) to build initial index |
| `Documents: 0` in status | Index not yet built | Run `task zmd` to index laws + default precedents |
| No search results | Query too specific or index incomplete | Check `task zmd:status`; try broader terms; verify collections with `zmd ls` |
| Stale results | Upstream repos updated since last sync | Run `task zmd:sync` to pull and re-index |
| Precedent not found | Case type or court not in indexed scope | Expand scope with `task zmd:precedents:all`; use `legal-ko-cli precedent-show` as fallback |

## Examples

### Example 1: Natural language law search

**User:** "전세 보증금을 못 돌려받고 있어요"

```bash
# Native hybrid search (fast, ~410 ms)
legal-ko-cli zmd query "전세 보증금 반환" --json

# Get context snippets to see matching passages
zmd context "보증금 반환" --json

# Read the key law
zmd get "laws/kr/주택임대차보호법/법률.md"
```

### Example 2: Finding precedents on a specific legal issue

**User:** "명예훼손 관련 대법원 판례를 찾아줘"

```bash
# Native FTS search for precedents (~10 ms)
legal-ko-cli zmd search "명예훼손" --collection precedents --json

# Native hybrid for broader coverage (~410 ms)
legal-ko-cli zmd query "명예훼손 판례 대법원" --json

# Read a specific ruling
zmd get "precedents/형사/대법원/2020도12345.md"
```

### Example 3: Cross-collection search

**User:** "근로기준법 관련 법률과 판례를 모두 찾아줘"

```bash
# Native hybrid across both collections (~410 ms)
legal-ko-cli zmd query "근로기준법" --json

# Or search each collection separately for more control (~10 ms each)
legal-ko-cli zmd search "근로기준" --collection laws --json
legal-ko-cli zmd search "근로기준" --collection precedents --json
```

### Example 4: Retrieving a specific case by number

**User:** "2000다10048 판결을 보여줘"

```bash
# Native FTS search for exact case number (~10 ms)
legal-ko-cli zmd search "2000다10048" --collection precedents --json

# Or retrieve directly if you know the path
zmd get "precedents/민사/대법원/2000다10048.md"
```
