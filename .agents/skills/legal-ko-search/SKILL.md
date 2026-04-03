---
name: legal-ko-search
description: >-
  Find relevant Korean laws and articles using the legal-ko-cli tool.
  Translates natural language legal questions (Korean or English) into
  structured searches, retrieves matching laws, reads their content, and
  cites specific articles. USE FOR: find Korean law, search Korean
  legislation, 법 찾기, 법률 검색, 관련 법 찾아줘, 법조항 찾기, legal
  question Korea, what law applies, tenant rights Korea, 전세 법,
  근로기준법 조항, 이혼 관련 법. DO NOT USE FOR: legal advice or
  interpretation (always add a disclaimer), non-Korean jurisdictions,
  creating or editing law files, TUI usage questions.
license: MIT
metadata:
  author: legal-ko contributors
  version: "1.0.0"
---

# Korean Law Search

Find relevant Korean laws and specific articles for any legal question
using `legal-ko-cli`.

| What | Answer |
|------|--------|
| **Tool** | `legal-ko-cli` (installed to `~/bin` via `task install`) |
| **Data source** | All Korean laws from [legalize-kr](https://github.com/legalize-kr/legalize-kr) |
| **Search method** | Title substring match (naive) or Meilisearch if configured |
| **Output** | Law titles, IDs, article citations with excerpts |
| **Language** | Respond in the same language the user used (Korean or English) |

## Install

```bash
# From the legal-ko workspace root
task install
```

This builds release binaries and copies `legal-ko` (TUI) and `legal-ko-cli`
to `~/bin`. Verify: `legal-ko-cli list --limit 1 --json` should return a JSON array.

## When to Use

- User describes a legal situation and wants to know which laws apply
- User asks for specific articles (조항) related to a topic
- User names a law and wants to see its contents or article list
- User asks "관련 법을 찾아줘", "어떤 법이 적용돼?", "What law covers...?"

## Workflow

### Phase 1 — Understand the Question

Parse the user's question to identify:

1. **Legal topic** — What area of law? (housing, labor, criminal, family, etc.)
2. **Specific situation** — What happened or what do they need? (eviction, unpaid wages, etc.)
3. **Desired output** — Do they want a law name, specific articles, or a summary?

### Phase 2 — Translate to Search Terms

> **CRITICAL:** `legal-ko-cli search` matches against **law titles** only.
> Colloquial descriptions must be translated to formal law names.
> Always run **multiple searches** with different keywords to ensure coverage.

Use the reference table below to map common topics to search terms. Combine
the user's topic with this domain knowledge to form 2-5 search queries.

**Example:** User says "전세 문제가 있어" (I have a jeonse problem)

This maps to housing/lease law. Run these searches:
```bash
legal-ko-cli search "임대차" --json --limit 20
legal-ko-cli search "주택" --json --limit 20
legal-ko-cli search "민법" --json --limit 5
legal-ko-cli search "보증금" --json --limit 10
```

### Phase 3 — Search and Discover

Run the search queries from Phase 2 using `legal-ko-cli search`:

```bash
legal-ko-cli search "<term>" --json --limit 20
```

Review the results. Deduplicate across searches (same `id` may appear in
multiple results). Pick the most relevant laws — typically 1-5 laws.

If the initial searches return no results, try:
- Shorter keywords (e.g., "임대" instead of "임대차보호")
- Related legal concepts from the topic map below
- `legal-ko-cli list --json` piped through a filter as a last resort

### Phase 4 — Read Law Content

For each relevant law, fetch its full content:

```bash
legal-ko-cli show "<id>" --json
```

The `content` field contains the full law text (markdown, frontmatter stripped).
Scan it to confirm relevance to the user's question.

### Phase 5 — Find Specific Articles

List the articles in each relevant law:

```bash
legal-ko-cli articles "<id>" --json
```

This returns an array of `{ "label": "제X조 (제목)", "line_index": N }`.

To extract a specific article's text, use the `show` output from Phase 4
and locate the text starting at the article heading. Articles end where the
next `##### 제...조` heading begins or at the end of the document.

### Phase 6 — Present Findings

Structure the response as:

1. **Summary** — One-paragraph answer to the user's question
2. **Relevant Laws** — Table of laws with ID, title, and why they're relevant
3. **Key Articles** — Specific article citations with brief excerpts
4. **Disclaimer** — Always include (see below)

**Example response structure:**

> 전세 관련 문제에는 주로 다음 법률이 적용됩니다:
>
> | 법률 | 핵심 조항 | 관련 내용 |
> |------|----------|----------|
> | 주택임대차보호법 | 제3조, 제3조의2 | 대항력, 우선변제권 |
> | 민법 | 제618조~제654조 | 임대차 일반 규정 |
>
> **제3조 (대항력 등)** — 임차인이 주택의 인도와 주민등록을 마친 때에는...
>
> ⚠️ 이 정보는 법률 조문의 검색 결과이며, 법률 자문이 아닙니다.
> 구체적인 사안은 반드시 변호사 등 전문가와 상담하세요.

### Disclaimer (Required)

**Always** append one of these disclaimers:

Korean:
> ⚠️ 이 정보는 법률 조문의 검색 결과이며, 법률 자문이 아닙니다.
> 구체적인 사안은 반드시 변호사 등 전문가와 상담하세요.

English:
> ⚠️ This is a search result from Korean legislation, not legal advice.
> Consult a licensed attorney for advice on your specific situation.

## Topic → Search Term Reference

Use this table to translate common legal topics into effective search terms
for `legal-ko-cli search`. Run **all listed terms** for the matching topic,
then pick the most relevant results.

| Topic (Korean) | Topic (English) | Search Terms |
|----------------|-----------------|--------------|
| 전세, 월세, 임대차 | Lease, rent, jeonse | `임대차`, `주택`, `민법`, `보증금` |
| 부동산 매매 | Real estate | `부동산`, `등기`, `공인중개사`, `민법` |
| 이혼, 양육권 | Divorce, custody | `민법`, `가사소송`, `가정폭력`, `양육` |
| 상속, 유언 | Inheritance, wills | `민법`, `상속세`, `유언` |
| 노동, 근로, 해고 | Labor, employment | `근로기준`, `노동조합`, `최저임금`, `고용` |
| 교통사고 | Traffic accident | `도로교통`, `자동차손해`, `교통사고` |
| 사기, 범죄, 폭행 | Fraud, crime, assault | `형법`, `형사소송`, `특정범죄` |
| 소비자 피해, 환불 | Consumer, refund | `소비자`, `전자상거래`, `할부거래` |
| 개인정보, 프라이버시 | Privacy, data | `개인정보`, `정보통신`, `신용정보` |
| 회사, 창업, 법인 | Company, startup | `상법`, `중소기업`, `벤처`, `법인세` |
| 의료 사고 | Medical malpractice | `의료`, `민법`, `형법` |
| 지식재산, 저작권 | IP, copyright | `저작권`, `특허`, `상표`, `부정경쟁` |
| 군대, 병역 | Military service | `병역`, `군형법`, `군인` |
| 세금, 납세 | Tax | `소득세`, `부가가치세`, `법인세`, `국세` |
| 행정 처분, 인허가 | Administrative | `행정`, `인허가`, `행정소송`, `행정절차` |
| 환경, 소음 | Environment, noise | `환경`, `소음`, `폐기물` |
| 학교, 교육 | Education | `교육`, `학교`, `학원` |

> **Tip:** If the topic doesn't appear above, decompose the user's problem
> into the underlying legal concepts and search for those. Korean law names
> are generally descriptive — a keyword from the topic will often appear in
> the law's title.

## CLI Command Reference

| Command | Purpose | Key Flags |
|---------|---------|-----------|
| `legal-ko-cli list` | List all laws | `--category`, `--department`, `--json`, `--limit` |
| `legal-ko-cli search <query>` | Search by title | `--json`, `--limit` |
| `legal-ko-cli show <id>` | Read full law text | `--json` |
| `legal-ko-cli articles <id>` | List articles in a law | `--json` |
| `legal-ko-cli bookmarks` | List bookmarked laws | `--json` |
| `legal-ko-cli context` | Read the TUI's current browsing state | `--json` |
| `legal-ko-cli navigate <id>` | Send a navigate command to the TUI | `--article`, `--json` |

**Law ID format:** `kr/{법령명}/{유형}` — e.g., `kr/민법/법률`, `kr/형법/법률`,
`kr/주택임대차보호법/법률`

**Category values:** `법률`, `대통령령`, `부령`

> **Always use `--json`** for structured output that's easier to parse.

## OpenCode Integration (TUI Context)

The TUI writes a context snapshot to `~/.cache/legal-ko/context.json` on every
navigation event. When running as an OpenCode agent alongside the TUI, use
`legal-ko-cli context --json` to read the user's current browsing state and
respond in the context of what they are looking at.

### Prerequisites

- The TUI (`legal-ko`) must be running — the context file is only written while
  the TUI is active.
- At least one supported AI agent must be installed. The TUI's `o` key opens an
  agent picker popup (OpenCode, Gemini CLI, GitHub Copilot CLI, Amp). Only
  agents found on `$PATH` appear in the picker. The last-used choice is
  persisted. Split panes use tmux, WezTerm, Zellij, or Ghostty. On terminals
  without split support, the TUI suspends itself, runs the agent in the
  foreground, and resumes when the agent exits.

### Context JSON Structure

```json
{
  "view": "detail",
  "timestamp": "2026-04-03T12:00:00Z",
  "selected_law": {
    "id": "kr/주택임대차보호법/법률",
    "title": "주택임대차보호법",
    "category": "법률",
    "departments": ["법무부"]
  },
  "filters": {
    "search_query": "임대차",
    "category": null,
    "department": null,
    "bookmarks_only": false,
    "total_laws": 6200,
    "filtered_count": 15
  },
  "detail": {
    "law_id": "kr/주택임대차보호법/법률",
    "law_title": "주택임대차보호법",
    "current_article": {
      "index": 2,
      "label": "제3조 (대항력 등)"
    },
    "total_articles": 18,
    "scroll_position": 45,
    "total_lines": 320
  }
}
```

| Field | Present When | Description |
|-------|-------------|-------------|
| `view` | Always | `"loading"`, `"list"`, or `"detail"` |
| `timestamp` | Always | ISO 8601 time of last update |
| `selected_law` | List or detail view | Law currently highlighted/open |
| `filters` | List or detail view | Active search query, category, department filters |
| `detail` | Detail view only | Scroll position, current article, total lines |
| `detail.current_article` | When scrolled past an article heading | Article the user is reading |

### Using Context in Searches

When the context is available, use it to **skip phases** in the search workflow:

1. **User is on detail view** — `detail.law_id` tells you which law they're
   reading. Skip straight to Phase 4/5 (show + articles) for that law.
2. **User has a search query** — `filters.search_query` tells you what they
   searched for. Use it as a starting point for Phase 2.
3. **User is on a specific article** — `detail.current_article.label` tells you
   exactly which article they're reading. Quote it directly in your response.

```bash
# Read current context
legal-ko-cli context --json

# If they're viewing a law, fetch its articles directly
legal-ko-cli articles "kr/주택임대차보호법/법률" --json
```

### Navigating the TUI

After finding relevant results, use `legal-ko-cli navigate` to move the TUI's
focus to the law or article. The behaviour is **context-aware** — it adapts
based on the TUI's current view.

**Read context first**, then navigate accordingly:

```bash
# 1. Always read context first to know what view the TUI is on
legal-ko-cli context --json
```

**If TUI is on list view** — navigate scrolls to and highlights the law:

```bash
# Scroll to 주택임대차보호법 in the list
legal-ko-cli navigate "kr/주택임대차보호법/법률"
```

**If TUI is on detail view (same law)** — navigate jumps to the article:

```bash
# Jump to 제3조 within the currently viewed law
legal-ko-cli navigate "kr/주택임대차보호법/법률" --article "제3조"
```

**If TUI is on detail view (different law)** — navigate returns to list and
highlights the target law:

```bash
# TUI is viewing 민법 but we want to point to 주택임대차보호법
legal-ko-cli navigate "kr/주택임대차보호법/법률"
```

**Article matching:** The `--article` value is matched as a **prefix** against
article labels. Use short prefixes like `"제3조"` to match `"제3조 (대항력 등)"`.
Use longer strings for disambiguation when multiple articles share a prefix.

**Workflow pattern for OpenCode:**

```bash
# 1. Read what the user is looking at
legal-ko-cli context --json

# 2. Search for relevant laws
legal-ko-cli search "임대차" --json --limit 20

# 3. Show the most relevant law
legal-ko-cli show "kr/주택임대차보호법/법률" --json

# 4. Get its articles
legal-ko-cli articles "kr/주택임대차보호법/법률" --json

# 5. Navigate the TUI to the relevant result
#    - On list view: scrolls to the law
#    - On detail view: jumps to the article
legal-ko-cli navigate "kr/주택임대차보호법/법률" --article "제3조"
```

## Error Handling

| Error | Cause | Resolution |
|-------|-------|------------|
| `command not found: legal-ko-cli` | CLI not installed | Run `task install` from the workspace root |
| Empty search results | Search term too specific or no matching law titles | Try shorter/broader keywords, check topic map |
| `HTTP 404` on `show` | Invalid law ID | Verify ID from `search` or `list` output first |
| `HTTP 403 rate limit` | GitHub API rate limit hit | Wait a minute and retry; metadata fetch is unauthenticated |
| Empty `departments`/`enforcement_date` in list | Expected — metadata is lazy | These fields populate only after `show` fetches the full content |

## Advanced: Filtering by Category

Korean law hierarchy:

| Level | Category | Description |
|-------|----------|-------------|
| 1 (highest) | `법률` | Acts passed by the National Assembly (국회) |
| 2 | `대통령령` (시행령) | Presidential decrees implementing Acts |
| 3 | `부령` (시행규칙) | Ministerial regulations with detailed rules |

To focus on primary legislation only:
```bash
legal-ko-cli list --category 법률 --json
```

When a user asks about a specific topic, start with `법률` level laws,
then drill into `대통령령`/`부령` only if the user needs implementation
details or specific procedures.

## Examples

### Example 1: 전세 문제

**User:** "전세 문제가 있어. 관련 법을 찾아줘."

**Agent workflow:**

```bash
# Phase 2-3: Search with multiple terms
legal-ko-cli search "임대차" --json --limit 20
legal-ko-cli search "주택" --json --limit 20
legal-ko-cli search "보증금" --json --limit 10

# Phase 4: Read the most relevant law
legal-ko-cli show "kr/주택임대차보호법/법률" --json

# Phase 5: Get article list
legal-ko-cli articles "kr/주택임대차보호법/법률" --json
```

### Example 2: 직장에서 부당해고

**User:** "직장에서 부당하게 해고당했어요."

**Agent workflow:**

```bash
legal-ko-cli search "근로기준" --json --limit 10
legal-ko-cli search "해고" --json --limit 10
legal-ko-cli search "노동" --json --limit 10

legal-ko-cli show "kr/근로기준법/법률" --json
legal-ko-cli articles "kr/근로기준법/법률" --json
```

Focus on: 제23조 (해고의 제한), 제27조 (해고사유 등의 서면통지),
제28조 (부당해고등의 구제신청)

### Example 3: English query about copyright

**User:** "Someone copied my blog posts. What Korean laws protect me?"

**Agent workflow:**

```bash
legal-ko-cli search "저작권" --json --limit 10
legal-ko-cli search "정보통신" --json --limit 10

legal-ko-cli show "kr/저작권법/법률" --json
legal-ko-cli articles "kr/저작권법/법률" --json
```

Focus on Chapter 2 (저작권), particularly provisions on
reproduction rights and remedies for infringement.
