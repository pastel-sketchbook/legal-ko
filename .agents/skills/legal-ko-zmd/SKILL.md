---
name: legal-ko-zmd
description: >-
  Search Korean laws and precedents via local hybrid index (FTS + vector).
  USE FOR: 법률 검색, 판례 검색, semantic search, zmd query, zmd search.
  DO NOT USE FOR: legal advice, TUI navigation (use legal-ko-search),
  법조인 search (use legal-ko-precedent).
license: MIT
metadata:
  author: legal-ko contributors
  version: "4.0.0"
---

# Korean Law & Precedent Search via zmd

**UTILITY SKILL** — INVOKES: `legal-ko-cli zmd search|query|similar`

## Search

| Situation | Command |
|-----------|---------|
| Exact term / case number | `legal-ko-cli zmd search "<q>" --json` (~10 ms) |
| Natural language | `legal-ko-cli zmd query "<q>" --json` (~410 ms) |
| Precedents + cited laws | `legal-ko-cli zmd similar "<q>" --json` |
| Poor results fallback | `zmd query "<q>" --rerank --json` (~1.5 s) |
| Retrieve document | `zmd get "<collection/path>"` |

Scope: `--collection laws` or `--collection precedents`.

## Workflow

1. Identify topic, translate colloquial → formal legal terms
2. Run 2-5 searches with varied terms
3. Read full docs: `zmd get` or `legal-ko-cli show --json`
4. Cross-ref: `legal-ko-cli precedent-laws` / `law-precedents`
5. Present: summary, docs table, excerpts, disclaimer

## Data Sources

Legalize-KR provides four public datasets:
- Laws: `legalize-kr/legalize-kr`
- Court precedents: `legalize-kr/precedent-kr`
- Administrative rules: `legalize-kr/admrule-kr`
- Local ordinances: `legalize-kr/ordinance-kr`

Currently indexed by zmd: laws and precedents.

## Index Management

`task zmd:sync` — pull + re-index. `task zmd:status` — check state.

See `reference/INDEXING.md` for scope, tasks, and directory layout.

## Disclaimer

⚠️ 검색 결과이며 법률 자문이 아닙니다. 변호사와 상담하세요.
