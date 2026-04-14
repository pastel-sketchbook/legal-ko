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
| **Tool** | `zmd` (v0.3.0+) |
| **Data source** | Two collections indexed locally from GitHub |
| **Collections** | `laws` → [legalize-kr](https://github.com/legalize-kr/legalize-kr), `precedents` → [precedent-kr](https://github.com/legalize-kr/precedent-kr) |
| **Scope** | Laws: 법률 (Acts) only (~1,711 docs). Precedents: 민사 + 형사 대법원 only (~35K docs). Expandable. |
| **Search modes** | FTS (`search`), vector (`vsearch`), hybrid (`query`), context snippets (`context`) |
| **Output** | Markdown by default; `--json`, `--csv`, `--md` flags available |
| **Language** | Respond in the same language the user used (Korean or English) |

## Setup

### Prerequisites

`zmd` must be installed and on `$PATH`. Verify:

```bash
zmd version
```

### Building the Index

Collections are built using `scripts/zmd-collections.sh`, which:

1. Clones the upstream repos (shallow) into `~/.cache/legal-ko/zmd/repos/`
2. Stages filtered files as hardlinks into `~/.cache/legal-ko/zmd/stage/`
3. Registers the staged dirs as local zmd collections
4. Indexes in batches of 100 files (configurable), calling `zmd update` per batch

```bash
# From the legal-ko workspace root:

# Index laws only (법률 ~1,711 docs)
./scripts/zmd-collections.sh laws

# Index precedents only (민사+형사 대법원 ~35K docs)
./scripts/zmd-collections.sh precedents

# Index everything
./scripts/zmd-collections.sh all
```

> **Resumable:** If interrupted (Ctrl-C, timeout, etc.), re-run the same command.
> Already-indexed files are skipped automatically. Progress is shown per batch.

> **Timing:** Indexing speed depends on embedding generation. Expect ~1-20s/file
> depending on document size. Laws (~1,711 files) takes ~25-60 min.
> Precedents (~35K files) takes many hours. Run in a dedicated terminal.

Check progress at any time:

```bash
./scripts/zmd-collections.sh status
# or
zmd status
```

### Collection Scope

The script indexes a curated subset by default:

| Collection | Scope | File count | Source |
|-----------|-------|-----------|--------|
| `laws` | 법률 (Acts) only | ~1,711 | `legalize-kr` — `kr/*/법률.md` |
| `precedents` | 민사 + 형사, 대법원 only | ~35,000 | `precedent-kr` — `{민사,형사}/대법원/*.md` |

**Excluded by default:**
- Laws: 대통령령 (시행령), 부령 (시행규칙), and other regulation types
- Precedents: 가사, 세무, 일반행정, 특허, 기타, 선거·특별 case types
- Precedents: 하급심 (lower court) rulings

**To expand scope**, edit the arrays at the top of `scripts/zmd-collections.sh`:

```bash
# Add lower courts
PRECEDENT_COURTS=("대법원" "하급심")

# Add more case types
PRECEDENT_CASE_TYPES=("민사" "형사" "세무" "일반행정")
```

Then re-run `./scripts/zmd-collections.sh precedents`.

### Syncing Updates

The upstream repositories are updated periodically. To pull new/changed
documents and re-index:

```bash
./scripts/zmd-collections.sh sync
```

This does a `git pull` on both repos, re-stages any new files, and runs
`zmd update` (incremental — only new/changed files are processed).

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

> **Recommended default:** Use `query` with `--rerank` for most searches.
> It gives the best results by combining keyword precision with semantic recall.

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

| Situation | Strategy |
|-----------|----------|
| User names a specific law or case number | `zmd search` (FTS) with exact name/number |
| User describes a legal situation | `zmd query --rerank` (hybrid) for best recall + precision |
| User asks a conceptual question | `zmd vsearch` (semantic) for meaning-based results |
| Need to see match context | `zmd context` for snippets |
| Need a specific document | `zmd get` with known path |

### Phase 3 — Search and Discover

Run the appropriate search commands. Use the collection name to scope results:

```bash
# Search laws only
zmd search "근로기준" laws --json

# Search precedents only
zmd search "부당해고" precedents --json

# Search everything (laws + precedents)
zmd query "부당해고 관련 법률과 판례" --rerank --json
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
by default. Expand scope via the script configuration.

| Example Path | Description |
|-------------|-------------|
| `precedents/민사/대법원/2000다10048.md` | Civil Supreme Court case |
| `precedents/형사/대법원/2020도12017.md` | Criminal Supreme Court case |

> **Not indexed by default:** 가사 (family), 세무 (tax), 일반행정 (admin),
> 특허 (patent), 하급심 (lower court) cases. Use `legal-ko-cli` for those.

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
| **Discovery** — find relevant docs by topic | `zmd query --rerank` |
| **Content search** — search inside documents | `zmd search` or `zmd context` |
| **Semantic search** — natural language queries | `zmd vsearch` |
| **Structured metadata** — parsed frontmatter, sections | `legal-ko-cli show --json` / `precedent-show --json` |
| **Article listing** — list articles in a law | `legal-ko-cli articles --json` |
| **Cross-reference** — statute↔precedent links | `legal-ko-cli precedent-laws` / `law-precedents` |
| **Person search** — judges, attorneys | `legal-ko-cli precedent-search-person` |
| **TUI navigation** — open law in TUI | `legal-ko-cli navigate` |
| **Bookmarks** | `legal-ko-cli bookmarks` |

**Typical combined workflow:**

```bash
# 1. Discover relevant documents with zmd
zmd query "전세 보증금 반환" --rerank --json

# 2. Get structured data with legal-ko-cli
legal-ko-cli show "kr/주택임대차보호법/법률" --json
legal-ko-cli articles "kr/주택임대차보호법/법률" --json

# 3. Cross-reference
legal-ko-cli law-precedents "주택임대차보호법" --article "제3조" --json --limit 10

# 4. Navigate in TUI
legal-ko-cli navigate "kr/주택임대차보호법/법률" --article "제3조"
```

## Script Reference

### `scripts/zmd-collections.sh`

| Command | Purpose |
|---------|---------|
| `all` | Run all phases: laws then precedents (default) |
| `laws` | Clone + stage + index laws (법률 only, ~1,711 docs) |
| `precedents` | Clone + stage + index precedents (민사+형사 대법원, ~35K docs) |
| `sync` | Pull latest from upstream repos + re-stage + re-index |
| `status` | Show current state (repos, staged files, zmd collections) |
| `reset` | Remove collections and staged data (keeps repo clones) |
| `help` | Show help |

| Env Var | Default | Purpose |
|---------|---------|---------|
| `ZMD_CACHE_DIR` | `~/.cache/legal-ko/zmd` | Cache root for repos, staged files |
| `ZMD_BATCH_SIZE` | `100` | Files per `zmd update` call |

### How Batching Works

1. The script finds all matching files (e.g. `kr/*/법률.md`)
2. Stages them as **hardlinks** into the collection dir (zero disk overhead)
3. Calls `zmd update` every `BATCH_SIZE` files
4. `zmd update` skips already-indexed docs, so only new files are processed
5. On re-run, already-staged files are also skipped

This means:
- Each batch completes in 1-5 min (not hours)
- Progress is visible after every batch
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
| `Failed to open database` | Database not initialized | Run `zmd update` or `./scripts/zmd-collections.sh laws` |
| `Documents: 0` in status | Index not yet built | Run `./scripts/zmd-collections.sh laws` (or `all`) |
| No search results | Query too specific or index incomplete | Check `zmd status`; try broader terms; verify collections with `zmd collection list` |
| Stale results | Upstream repos updated since last sync | Run `./scripts/zmd-collections.sh sync` |
| Precedent not found | Case type or court not in indexed scope | Check scope config; use `legal-ko-cli precedent-show` as fallback |

## Examples

### Example 1: Natural language law search

**User:** "전세 보증금을 못 돌려받고 있어요"

```bash
# Semantic search finds relevant laws by meaning
zmd query "전세 보증금 반환" --rerank --json

# Get context snippets to see matching passages
zmd context "보증금 반환" --json

# Read the key law
zmd get "laws/kr/주택임대차보호법/법률.md"
```

### Example 2: Finding precedents on a specific legal issue

**User:** "명예훼손 관련 대법원 판례를 찾아줘"

```bash
# Hybrid search across precedents
zmd search "명예훼손" precedents --json

# Semantic search for broader coverage
zmd vsearch "명예훼손 판례 대법원" --json

# Read a specific ruling
zmd get "precedents/형사/대법원/2020도12345.md"
```

### Example 3: Cross-collection search

**User:** "근로기준법 관련 법률과 판례를 모두 찾아줘"

```bash
# Search across both collections
zmd query "근로기준법" --rerank --json

# Or search each collection separately for more control
zmd search "근로기준" laws --json
zmd search "근로기준" precedents --json
```

### Example 4: Retrieving a specific case by number

**User:** "2000다10048 판결을 보여줘"

```bash
# FTS search for exact case number
zmd search "2000다10048" precedents --json

# Or retrieve directly if you know the path
zmd get "precedents/민사/대법원/2000다10048.md"
```
