---
name: legal-ko-search
description: >-
  Find Korean laws and articles by title search via legal-ko-cli.
  USE FOR: 법 찾기, 법률 검색, 법조항 찾기, find Korean law, what law applies.
  DO NOT USE FOR: legal advice, precedent search (use legal-ko-precedent),
  content search (use legal-ko-zmd).
license: MIT
metadata:
  author: legal-ko contributors
  version: "2.0.0"
---

# Korean Law Search

**UTILITY SKILL** — INVOKES: `legal-ko-cli search|show|articles|navigate`

## Workflow

1. Map colloquial terms to formal law names (see `reference/TOPICS.md`)
2. Search: `legal-ko-cli search "<term>" --json --limit 20` (run 2-5 terms)
3. Read: `legal-ko-cli show "<id>" --json`
4. Articles: `legal-ko-cli articles "<id>" --json`
5. Present: summary, law table, key articles, disclaimer

> `search` matches **titles only**. Always run multiple keywords.

## Commands

| Command | Purpose |
|---------|---------|
| `search <q> --json` | Search by title |
| `show <id> --json` | Full law text |
| `articles <id> --json` | List articles |
| `navigate <id> --article "제N조"` | Open in TUI |
| `context --json` | Read TUI state |

ID format: `kr/{법령명}/{유형}` — e.g., `kr/민법/법률`

## TUI Context

`legal-ko-cli context --json` reads what the user is viewing.
See `reference/TUI.md` for JSON structure and navigate usage.

## Disclaimer

⚠️ 검색 결과이며 법률 자문이 아닙니다. 변호사와 상담하세요.
