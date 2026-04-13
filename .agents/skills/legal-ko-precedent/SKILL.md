---
name: legal-ko-precedent
description: >-
  Find relevant Korean court precedents and cross-reference them with statutes
  using the legal-ko-cli tool. Translates legal questions into structured
  precedent searches, retrieves rulings, extracts key sections (판시사항,
  판결요지, 참조조문), and maps precedents to the laws they cite. USE FOR:
  find Korean precedent, search court ruling, 판례 찾기, 판례 검색, 관련 판례
  찾아줘, 대법원 판결, case law Korea, how did the court rule, 참조조문 찾기,
  which precedents cite this law, 민법 제840조 판례, 형법 판례, find judge,
  find attorney, 법조인 찾기, 판사 찾기, 변호사 찾기, 검사 찾기, cases by judge,
  cases by lawyer. DO NOT USE FOR: legal advice or interpretation (always add a
  disclaimer), non-Korean jurisdictions, searching statutes without precedent
  context (use legal-ko-search for pure statute lookup), creating or editing
  files.
license: MIT
metadata:
  author: legal-ko contributors
  version: "1.0.0"
---

# Korean Precedent Search

Find relevant Korean court precedents (판례) and cross-reference them with
statutes using `legal-ko-cli`.

| What | Answer |
|------|--------|
| **Tool** | `legal-ko-cli` (installed to `~/bin` via `task install`) |
| **Data source** | 123,000+ precedents from [precedent-kr](https://github.com/legalize-kr/precedent-kr) |
| **Law data** | All Korean statutes from [legalize-kr](https://github.com/legalize-kr/legalize-kr) |
| **Search method** | Case name / case number substring match; auto-fallback to 법조인 name search |
| **Output** | Precedent metadata, sections, statute cross-references, 법조인 (legal professional) extraction |
| **Language** | Respond in the same language the user used (Korean or English) |

## Install

```bash
# From the legal-ko workspace root
task install
```

Verify: `legal-ko-cli precedent-list --limit 1 --json` should return a JSON array.

## Data Model

### Precedent Structure

Each precedent is a markdown file with YAML frontmatter:

```yaml
---
판례일련번호: '81927'
사건번호: 2000다10048
사건명: 소유권이전등기등
법원명: 대법원
법원등급: 대법원
사건종류: 민사
출처: https://www.law.go.kr/판례/81927
선고일자: '2002-09-27'
---
```

The body contains these standard sections (all as `## ` headings):

| Section | Content | Always Present |
|---------|---------|----------------|
| `## 판시사항` | Summary of legal points decided | Most cases |
| `## 판결요지` | Concise holding / ratio decidendi | Most cases |
| `## 참조조문` | Statutes cited (links to laws) | Most cases |
| `## 참조판례` | Other precedents cited (citation graph) | Many cases |
| `## 판례내용` | Full text of the ruling | Always |

### Case Types (사건종류)

| Case Type | Count | Description |
|-----------|-------|-------------|
| `민사` | 42,016 | Civil cases |
| `일반행정` | 45,028 | Administrative cases |
| `형사` | 21,624 | Criminal cases |
| `세무` | 10,024 | Tax cases |
| `특허` | 3,371 | Patent/IP cases |
| `가사` | 1,387 | Family cases |
| `기타` | 11 | Other |
| `선거·특별` | 8 | Election/special |

### Court Levels (법원명)

| Court | Count | Description |
|-------|-------|-------------|
| `대법원` | 68,002 | Supreme Court |
| `하급심` | 55,466 | Lower courts |

### Precedent ID Format

`{사건종류}/{법원명}/{사건번호}` — e.g., `민사/대법원/2000다10048`

The file path is the same with `.md` appended.

## When to Use

- User asks about court rulings on a legal topic
- User wants to know how a specific statute has been interpreted
- User names a case number and wants to see the ruling
- User asks "관련 판례를 찾아줘", "대법원이 어떻게 판결했어?", "What did the court rule on...?"
- User is viewing a law article and wants related precedents
- User wants to trace the citation chain of a ruling (참조판례)
- User asks about cases involving a specific judge, attorney, or prosecutor (법조인)
- User wants to know which judges presided over a certain type of case

## Workflow

### Phase 1 — Understand the Question

Parse the user's question to identify:

1. **Legal topic** — What area of law? (contract, tort, criminal, tax, etc.)
2. **Specific question** — What legal issue? (breach of contract, self-defense, etc.)
3. **Desired output** — Do they want a ruling summary, specific statute interpretation, or citation chain?
4. **Scope** — Supreme Court only? Specific case type? Date range?

### Phase 2 — Translate to Search Terms

> **CRITICAL:** `legal-ko-cli precedent-search` matches against **case names**
> (사건명) and **case numbers** (사건번호). Case names are formal Korean legal
> descriptions, not colloquial terms. If the query looks like a Korean name
> (2-4 Hangul syllables) and returns no metadata matches, the CLI automatically
> falls back to 법조인 (legal professional) search across document content.

**Case name patterns** — Korean precedent case names follow standard legal
terminology. Common patterns:

| Colloquial | Case Name Search Terms |
|------------|----------------------|
| 계약 위반, 손해배상 | `손해배상`, `채무불이행` |
| 부동산 문제 | `소유권`, `등기`, `명도`, `토지` |
| 이혼 | `이혼`, `재산분할`, `양육` |
| 해고 | `해고`, `부당해고`, `근로` |
| 사기, 횡령 | `사기`, `횡령`, `배임` |
| 교통사고 | `교통`, `손해배상` |
| 세금 | `부과처분취소`, `경정청구` |
| 의료사고 | `의료`, `손해배상` |
| 특허 침해 | `특허`, `침해`, `무효` |
| 명예훼손 | `명예훼손`, `모욕` |
| 상속 | `상속`, `유류분` |

### Phase 3 — Search and Filter

Run searches with appropriate filters:

```bash
# Basic search by case name
legal-ko-cli precedent-search "손해배상" --json --limit 30

# Filter by case type
legal-ko-cli precedent-list --case-type 민사 --json --limit 30

# Filter by court
legal-ko-cli precedent-list --court 대법원 --json --limit 30

# Combined filter
legal-ko-cli precedent-list --case-type 형사 --court 대법원 --json --limit 30

# Sort by ruling date (newest first)
legal-ko-cli precedent-list --case-type 민사 --sort date --json --limit 20
```

> **Note:** The initial listing from the Trees API provides case numbers
> and directory-derived metadata only. Case names and ruling dates are
> populated from frontmatter when you `precedent-show` a specific case.
> For broad searches, use `precedent-search` which matches against whatever
> metadata is available.

Review results and select the most relevant precedents — typically 1-5 cases.

### Phase 4 — Read Precedent Content

For each relevant precedent, fetch its full content:

```bash
legal-ko-cli precedent-show "민사/대법원/2000다10048" --json
```

The JSON response includes:
- All frontmatter metadata (case_name, ruling_date, court, etc.)
- `sections` — array of section labels found in the document
- `content` — full ruling text (frontmatter stripped)

### Phase 5 — Extract Key Sections

List the sections in a precedent:

```bash
legal-ko-cli precedent-sections "민사/대법원/2000다10048" --json
```

Returns `{ "sections": [{ "label": "판시사항", "line_index": 2 }, ...] }`.

**Reading priority by use case:**

| User's Need | Read These Sections |
|-------------|-------------------|
| Quick summary | 판시사항 → 판결요지 |
| Which laws apply | 참조조문 |
| Full reasoning | 판례내용 |
| Related cases | 참조판례 |
| How a statute was interpreted | 판결요지 + 참조조문 |

### Phase 6 — Cross-Reference with Statutes

This is the key differentiator of this skill. The `precedent-laws` command
runs a 4-approach fallback cross-reference automatically:

1. **Exact parse** — Extracts structured `StatuteRef` from 참조조문 (law name + article + detail)
2. **Case citation parse** — Extracts `CaseRef` from 참조판례 (court + case number + date)
3. **Fuzzy law-name matching** — Matches extracted law names against all known law directory names (exact → strip "구 " → substring)
4. **Case-type affinity** — When no structured refs exist, suggests related statutes by case type

**Precedent → Laws (forward reference):**

```bash
# Automatic 4-approach cross-reference
legal-ko-cli precedent-laws "민사/대법원/2000다10048" --json
```

The JSON response includes:
- `statute_refs` — All structured statute references from 참조조문
- `case_refs` — All case citations from 참조판례
- `law_matches` — Each statute ref matched to a law ID (with match_type: Exact/OldVersion/Substring/Unmatched)
- `affinity` — Fallback suggestions based on case type
- `resolution` — Which approach produced the result (ExactStatuteMatch/PartialStatuteMatch/CaseRefsOnly/AffinityFallback)

Then fetch the actual statute text for any matched laws:

```bash
# Read the specific article
legal-ko-cli show "kr/민법/법률" --json
legal-ko-cli articles "kr/민법/법률" --json
```

**Laws → Precedents (reverse lookup):**

```bash
# Find precedents citing 민법
legal-ko-cli law-precedents "민법" --json --limit 20

# Find precedents citing a specific article
legal-ko-cli law-precedents "민법" --article "제840조" --json --limit 20
```

> **Note:** `law-precedents` pre-filters by matching the law name in case names,
> then fetches and parses 참조조문 to confirm. This is a best-effort scan —
> for comprehensive results on popular statutes, consider narrowing with
> `--article` and adjusting `--limit`.

### Phase 7 — Present Findings

Structure the response as:

1. **Summary** — One-paragraph answer to the user's question
2. **Key Precedents** — Table with case number, case name, court, date, and relevance
3. **Holdings** — 판결요지 excerpts from the most relevant cases
4. **Referenced Statutes** — Cross-reference table linking precedents to statute articles
5. **Citation Chain** — If relevant, show the 참조판례 links between rulings
6. **Disclaimer** — Always include (see below)

**Example response structure:**

> 민법 제840조 제6호(기타 혼인을 계속하기 어려운 중대한 사유)에 대한
> 대법원 판례를 검토하였습니다.
>
> | 사건번호 | 사건명 | 선고일 | 핵심 판시 |
> |---------|--------|--------|----------|
> | 2000므1561 | 이혼등 | 2001-02-23 | 혼인파탄의 책임이 ... |
> | 2001므725 | 이혼·위자료 | 2001-09-25 | 유책배우자의 이혼청구는 ... |
>
> **참조 법조문:**
> - 민법 제840조 제6호 (이혼 사유)
> - 민법 제842조 (부양의무)
>
> ⚠️ 이 정보는 판례 검색 결과이며, 법률 자문이 아닙니다.

### Disclaimer (Required)

**Always** append one of these disclaimers:

Korean:
> ⚠️ 이 정보는 판례 검색 결과이며, 법률 자문이 아닙니다.
> 구체적인 사안은 반드시 변호사 등 전문가와 상담하세요.

English:
> ⚠️ This is a search result from Korean court precedents, not legal advice.
> Consult a licensed attorney for advice on your specific situation.

## 참조조문 Parsing Reference

The 참조조문 section follows consistent formatting patterns. Use these to
extract structured statute references:

### Common Patterns

```
{법령명} 제{N}조                          → law + article
{법령명} 제{N}조 제{M}항                   → law + article + paragraph
{법령명} 제{N}조 제{M}항 제{K}호           → law + article + paragraph + item
{법령명} 제{N}조, 제{M}조                  → law + multiple articles
[{번호}] {법령명} 제{N}조 / [{번호}] ...   → numbered groups (one per 판시사항)
```

### Mapping Law Names to IDs

Common law names found in 참조조문 and their `legal-ko-cli` search terms:

| 참조조문 Name | Search Term | Typical ID |
|--------------|-------------|------------|
| 민법 | `민법` | `kr/민법/법률` |
| 형법 | `형법` | `kr/형법/법률` |
| 상법 | `상법` | `kr/상법/법률` |
| 민사소송법 | `민사소송` | `kr/민사소송법/법률` |
| 형사소송법 | `형사소송` | `kr/형사소송법/법률` |
| 행정소송법 | `행정소송` | `kr/행정소송법/법률` |
| 헌법 | `헌법` | `kr/대한민국헌법/법률` |
| 근로기준법 | `근로기준` | `kr/근로기준법/법률` |
| 국세기본법 | `국세기본` | `kr/국세기본법/법률` |
| 주택임대차보호법 | `임대차` | `kr/주택임대차보호법/법률` |
| 저작권법 | `저작권` | `kr/저작권법/법률` |
| 특허법 | `특허법` | `kr/특허법/법률` |

> For older precedents, law names may use outdated titles (e.g.,
> "집합건물의소유및관리에관한법률" without spaces). Search with a
> distinctive keyword fragment to find the current law.

## 참조판례 Parsing Reference

The 참조판례 section contains citations to other rulings:

```
대법원 2000. 11. 10. 선고 2000다24061 판결(공2001상, 12)
대법원 2000. 6. 27. 선고 2000다11621 판결
```

### Extracting Case Numbers

The case number (e.g., `2000다24061`) maps directly to a precedent file.
To find it:

1. The case type prefix in the number hints at the case type:
   - `다` → 민사 (civil)
   - `도` → 형사 (criminal)
   - `두` → 세무/행정 (tax/admin)
   - `므` → 가사 (family)
   - `후` → 특허 (patent)
   - `그` → 민사 (civil, special)
2. Construct the ID: `{사건종류}/{법원명}/{사건번호}`
3. Fetch with `precedent-show`

```bash
# From: "대법원 2000. 6. 27. 선고 2000다11621 판결"
legal-ko-cli precedent-show "민사/대법원/2000다11621" --json
```

### Case Number Prefix Reference

| Prefix | Case Type | Directory |
|--------|-----------|-----------|
| 다, 그, 마, 카 | 민사 | `민사/` |
| 도, 모, 오, 초, 감 | 형사 | `형사/` |
| 두, 누 | 세무 or 일반행정 | `세무/` or `일반행정/` |
| 므, 스, 으, 즈, 브 | 가사 | `가사/` |
| 후, 허, 흐 | 특허 | `특허/` |

> When a prefix is ambiguous (e.g., `두` could be 세무 or 일반행정), try both
> directories. One will return HTTP 404; use the one that succeeds.

## Topic → Case Type Mapping

Use this to narrow searches to the right case type:

| Topic (Korean) | Topic (English) | Case Type | Search Terms |
|----------------|-----------------|-----------|--------------|
| 계약, 매매, 채권 | Contract, sale, debt | 민사 | `손해배상`, `계약`, `채무` |
| 부동산, 소유권 | Real estate, ownership | 민사 | `소유권`, `등기`, `명도` |
| 불법행위, 사고 | Tort, accident | 민사 | `손해배상`, `불법행위` |
| 이혼, 상속, 양육 | Divorce, inheritance, custody | 가사 | `이혼`, `상속`, `양육` |
| 범죄, 폭행, 사기 | Crime, assault, fraud | 형사 | `사기`, `폭행`, `횡령` |
| 세금, 부과처분 | Tax assessment | 세무 | `부과처분`, `취소`, `경정` |
| 행정처분, 인허가 | Administrative action | 일반행정 | `처분취소`, `재량`, `인허가` |
| 특허, 상표, 디자인 | Patent, trademark | 특허 | `특허`, `침해`, `무효` |

## CLI Command Reference

### Precedent Commands

| Command | Purpose | Key Flags |
|---------|---------|-----------|
| `legal-ko-cli precedent-list` | List precedents | `--case-type`, `--court`, `--sort`, `--json`, `--limit` |
| `legal-ko-cli precedent-search <query>` | Search by case name / number; auto-falls back to 법조인 search if query looks like a Korean name | `--json`, `--limit` |
| `legal-ko-cli precedent-show <id>` | Read full precedent text | `--json` |
| `legal-ko-cli precedent-sections <id>` | List sections in a precedent | `--json` |
| `legal-ko-cli precedent-laws <id>` | Cross-reference: find laws cited by a precedent | `--json` |
| `legal-ko-cli precedent-persons <id>` | Extract judges, attorneys, prosecutors from a precedent | `--json` |
| `legal-ko-cli precedent-search-person <name>` | Search precedents by person name | `--role`, `--case-type`, `--court`, `--json`, `--limit` |
| `legal-ko-cli law-precedents <law_name>` | Reverse: find precedents citing a law | `--article`, `--json`, `--limit` |

### Law Commands (for cross-referencing)

| Command | Purpose | Key Flags |
|---------|---------|-----------|
| `legal-ko-cli search <query>` | Search laws by title | `--json`, `--limit` |
| `legal-ko-cli show <id>` | Read full law text | `--json` |
| `legal-ko-cli articles <id>` | List articles in a law | `--json` |

> **Always use `--json`** for structured output that's easier to parse.

## OpenCode Integration (TUI Context)

When running alongside the TUI, use `legal-ko-cli context --json` to check
if the user is currently viewing a law. If they are, proactively find
precedents that cite the statute article they're reading.

```bash
# Check what the user is reading
legal-ko-cli context --json

# If they're on 민법 제840조, use the reverse lookup command
legal-ko-cli law-precedents "민법" --article "제840조" --json --limit 20
```

## Error Handling

| Error | Cause | Resolution |
|-------|-------|------------|
| `command not found: legal-ko-cli` | CLI not installed | Run `task install` from workspace root |
| Empty search results | Search term too specific or no matches | Try shorter keywords, check topic map |
| `HTTP 404` on `precedent-show` | Invalid ID or wrong case type directory | Try alternative directory (e.g., `세무/` vs `일반행정/`) |
| `HTTP 403 rate limit` | GitHub API rate limit | Wait and retry; set `GITHUB_TOKEN` env var for higher limits |
| Tree API truncated | Repository has >100K files | This is a known limitation; the API may not return all entries |
| Empty `case_name` in list | Expected — metadata is lazy | Case names populate after `precedent-show` fetches frontmatter |

## Examples

### Example 1: Finding precedents on lease deposit recovery

**User:** "전세 보증금을 못 돌려받고 있어요. 관련 판례가 있나요?"

**Agent workflow:**

```bash
# Search for relevant precedents
legal-ko-cli precedent-search "보증금" --json --limit 20
legal-ko-cli precedent-search "임대차" --json --limit 20
legal-ko-cli precedent-search "명도" --json --limit 20

# Read the most relevant ruling
legal-ko-cli precedent-show "민사/대법원/2002다38361" --json

# Cross-reference: find the statute
legal-ko-cli search "임대차" --json --limit 10
legal-ko-cli articles "kr/주택임대차보호법/법률" --json
```

### Example 2: How courts interpret a specific statute article

**User:** "형법 제138조가 실제로 어떻게 적용되나요?"

**Agent workflow:**

```bash
# Read the statute article first
legal-ko-cli show "kr/형법/법률" --json
# → Find 제138조 in the content

# Use reverse lookup to find precedents citing this article
legal-ko-cli law-precedents "형법" --article "제138조" --json --limit 20

# Read a promising result
legal-ko-cli precedent-show "형사/대법원/2020도12017" --json

# Or use the full cross-reference to see all laws cited by this precedent
legal-ko-cli precedent-laws "형사/대법원/2020도12017" --json
```

### Example 3: Tracing a citation chain

**User:** "2000다10048 판결의 참조판례를 추적해줘."

**Agent workflow:**

```bash
# Cross-reference the source precedent (extracts both statute refs and case refs)
legal-ko-cli precedent-laws "민사/대법원/2000다10048" --json
# → case_refs contains: 2000다24061, 2000다11621, 2000다22812

# Follow each citation
legal-ko-cli precedent-show "민사/대법원/2000다24061" --json
legal-ko-cli precedent-show "민사/대법원/2000다11621" --json
legal-ko-cli precedent-show "민사/대법원/2000다22812" --json

# Present as a citation graph with key holdings from each
```

### Example 4: English query about wrongful dismissal precedents

**User:** "Are there Korean Supreme Court rulings on wrongful termination?"

**Agent workflow:**

```bash
# Search with Korean legal terms
legal-ko-cli precedent-search "부당해고" --json --limit 20
legal-ko-cli precedent-search "해고" --json --limit 20
legal-ko-cli precedent-list --case-type 민사 --court 대법원 --sort date --json --limit 30

# Read the most relevant
legal-ko-cli precedent-show "민사/대법원/..." --json

# Cross-reference with labor law
legal-ko-cli search "근로기준" --json --limit 5
legal-ko-cli articles "kr/근로기준법/법률" --json
```

### Example 5: Finding cases by a specific judge

**User:** "김선수 대법관이 참여한 민사 판례를 찾아줘."

**Agent workflow:**

```bash
# Search by judge name, filtered by case type
legal-ko-cli precedent-search-person "김선수" --role judge --case-type 민사 --json --limit 20

# Read a specific result to see full details
legal-ko-cli precedent-show "민사/대법원/2018다12345" --json

# Extract all persons from that precedent
legal-ko-cli precedent-persons "민사/대법원/2018다12345" --json
```

### Example 6: Finding cases by attorney

**User:** "변호사 이름으로 판례를 검색할 수 있어?"

**Agent workflow:**

```bash
# Search by attorney name across all case types
legal-ko-cli precedent-search-person "홍길동" --role attorney --json --limit 30

# Narrow to a specific court
legal-ko-cli precedent-search-person "홍길동" --role attorney --court 대법원 --json --limit 20
```

> **Performance note:** 법조인 search uses a cached person index
> (`~/.cache/legal-ko/person_index.json`). The first search builds the
> index by scanning all 123K+ precedents concurrently (~3 min with 50
> parallel fetches). Subsequent searches are instant (HashMap lookup).
> The index is rebuilt automatically when it expires (7-day TTL) or when
> the number of precedents grows >5%. Use `--case-type` and `--court`
> flags to post-filter results after the instant lookup.

## 법조인 (Legal Professional) Extraction Reference

법조인 names (judges, attorneys, prosecutors) are extracted from the `판례내용`
(full ruling text) section, not from metadata. The extraction handles these
patterns:

### Judges (판사/대법관)

Found at the end of the ruling text:

```
대법관   이름(재판장) 이름(주심) 이름 이름
판사   이름(재판장) 이름 이름
```

Qualifiers like `(재판장)` and `(주심)` are preserved in the output.

### Attorneys (변호사)

Found in representation lines:

```
소송대리인 변호사 이름
(소송대리인 변호사 이름, 이름)
【변 호 인】 변호사 이름
```

### Prosecutors (검사)

Found in `【검    사】` lines (spacing inside 검사 varies):

```
【검    사】 이름 외 2인
【검 사】 이름
```

### Output Format

```json
{
  "persons": [
    { "name": "이름", "role": "judge", "qualifier": "재판장" },
    { "name": "이름", "role": "attorney", "qualifier": null },
    { "name": "이름", "role": "prosecutor", "qualifier": null }
  ]
}
