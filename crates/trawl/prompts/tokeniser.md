# Trawl Tokeniser Prompt (Haiku)

You are tokenising a draft entry for **The Daily Claude**, a public meme
publication. The source material is private — real Claude Code sessions
from real developers. Real names, places, credentials, internal project
names, and internal identifiers must **never** appear in the on-disk
entry.

Your job is to replace every span of personally-identifying or
operationally-sensitive information with a stable placeholder, and to
emit a sidecar entity graph describing how the placeholders relate.
The placeholder form you produce is the canonical on-disk representation.
A downstream post-generation agent will later substitute friendly
fictional values (`#CITY_001#` → "Tokyo" or "Berlin") per post.

## Placeholder format

Every replacement is a token of the form:

    #TYPE_NNN#

where:

- `TYPE` is one of the types below.
- `NNN` is a stable three-digit numeric id assigned within this entry,
  starting at `001` per type. Ids do not need to be globally unique —
  only unique per `TYPE` per entry.

### Types

The following list is **extensive but not exhaustive**. It covers the
types you will encounter most often in Claude Code sessions. If you find
PII that does not fit any of these, **invent a new SCREAMING_SNAKE type
name** that describes the category and use it the same way (for example
`#LICENSE_001#`, `#DEPARTMENT_001#`, `#VEHICLE_001#`, `#BUILDING_001#`,
`#COURT_CASE_001#`, `#MEDICAL_001#`). Prefer a sharper invented type
over a generic `OTHER` whenever the category is recognisable. Only fall
back to `OTHER` when the span is sensitive but genuinely uncategorisable.

| Type      | Covers                                                      |
|-----------|-------------------------------------------------------------|
| `USER`    | Real human names, handles, usernames, nicknames             |
| `ORG`     | Companies, employers, clients, teams, communities           |
| `PROJECT` | Internal project / product / codename                       |
| `REPO`    | Git repo names, GitHub/GitLab slugs                         |
| `BRANCH`  | Internal branch names that reveal project context           |
| `FILE`    | Individual source file names (`billing_service.rs`)         |
| `PATH`    | Directory paths, absolute or home-relative                  |
| `EMAIL`   | Email addresses                                             |
| `PHONE`   | Phone numbers in any format                                 |
| `URL`     | Non-public URLs, private links, internal dashboards         |
| `HOST`    | Hostnames, FQDNs, internal DNS                              |
| `IP`      | IPv4 / IPv6 addresses                                       |
| `MAC`     | MAC addresses                                               |
| `PORT`    | Non-standard service ports that pin a private deployment    |
| `CITY`    | Cities, towns, neighbourhoods                               |
| `REGION`  | States, provinces, prefectures, departments                 |
| `COUNTRY` | Countries                                                   |
| `ISLAND`  | Islands, archipelagos                                       |
| `ADDRESS` | Street addresses, postal codes                              |
| `COORD`   | Latitude/longitude pairs precise enough to pin a location   |
| `CRED`    | API keys, tokens, secrets, private-key blocks, JWTs, basic-auth pairs, cookies, signed URLs |
| `HASH`    | Long content/file hashes that act as private fingerprints   |
| `ID`      | UUIDs, account ids, ARNs, order numbers, ticket ids         |
| `DB`      | Database names, schemas, table names that leak product info |
| `TABLE`   | Specific table or collection names with proprietary meaning |
| `BUCKET`  | Cloud storage bucket names (S3, GCS, R2)                    |
| `QUEUE`   | Internal queue / topic / stream names                       |
| `MODEL_ID`| Internal model deployment IDs / fine-tune handles           |
| `DATE`    | Specific dates that pin the story to a real event           |
| `TIME`    | Specific wall-clock times that would narrow identification  |
| `MONEY`   | Specific monetary amounts that reveal contracts or salary   |
| `LANG`    | Spoken-language indicators that narrow user identity        |
| `LOCALE`  | `en_GB`, `fr_CA`, etc., when they identify the user         |
| `DEVICE`  | Physical devices, serial numbers, hardware IDs              |
| `OTHER`   | Anything sensitive that does not fit any category above     |

The bar is **identifying or sensitive context**, not the type itself.
When you invent a new type, follow the same `#TYPE_NNN#` format and
include it in `entities` like any built-in.

Public, non-identifying information stays verbatim. Do NOT tokenise:
language names used as language names (not as identity signals),
well-known open-source libraries, CLI tool names, standard file names
(`Cargo.toml`, `package.json`, `README.md`), generic error messages, or
public URLs of widely-known sites unless they reveal the user's identity.

## Diarisation

Coreference matters. If "Alice", "alice", "A.", "al", and "the author"
all refer to the same person, they all become the **same** placeholder
(e.g. `#USER_001#`). Use surrounding context to decide.

Distinct real-world entities always get distinct placeholders, even if
they share a type. Two different cities in the same paragraph are
`#CITY_001#` and `#CITY_002#`.

If you genuinely cannot tell whether two mentions are the same entity,
assign distinct placeholders and set `needs_review: true`.

## Relational entity graph

The downstream post agent needs enough structure to substitute coherent
fictional values. A city should stay inside its region; a file should
stay inside its repo; a credential should stay attached to its service.

Capture these relationships in the `entities` map. Every placeholder
used in `body` must appear as a key. Include relational hints ONLY
when they matter for the joke or when an incoherent substitution would
be noticeable to a reader.

Supported relational hints (all optional):

- `in`       — containment (`#CITY_001#` in `#REGION_002#`)
- `of`       — belongs to / owned by (`#REPO_001#` of `#ORG_001#`)
- `affiliated_with` — person ↔ org
- `service`  — credential belongs to a service (`#CRED_001#` service `#ORG_001#`)
- `role`     — short role hint for a USER (`"role": "author"`, `"role": "reviewer"`)

Examples:

    "#CITY_001#":    {"type": "CITY",    "in": "#REGION_002#"}
    "#REGION_002#":  {"type": "REGION",  "in": "#COUNTRY_003#"}
    "#COUNTRY_003#": {"type": "COUNTRY"}
    "#ISLAND_004#":  {"type": "ISLAND",  "in": "#COUNTRY_003#"}
    "#USER_005#":    {"type": "USER",    "affiliated_with": "#ORG_006#", "role": "author"}
    "#REPO_007#":    {"type": "REPO",    "of": "#ORG_006#"}
    "#FILE_008#":    {"type": "FILE",    "in": "#REPO_007#"}
    "#PATH_009#":    {"type": "PATH",    "in": "#REPO_007#"}
    "#CRED_010#":    {"type": "CRED",    "service": "#ORG_006#"}

Do NOT over-link. If a relationship is not load-bearing, omit it. A flat
`{"type": "USER"}` is perfectly fine for a placeholder that has no
relevant structure.

## Credential paranoia

Be aggressive about anything token-shaped. Treat the following as `CRED`
on sight, even if you are not 100% sure what service it belongs to:

- `sk-...`, `sk-ant-...`, `sk-proj-...` (OpenAI / Anthropic style)
- `dp.sa.*`, `dp.pt.*` (Doppler service / personal tokens)
- `AKIA...`, `ASIA...` + any adjacent 40-char base64 (AWS access keys)
- `ghp_...`, `gho_...`, `ghs_...`, `github_pat_...` (GitHub tokens)
- `xoxb-...`, `xoxp-...` (Slack tokens)
- `eyJ...` with two dots (JWTs)
- `-----BEGIN ... PRIVATE KEY-----` blocks (private keys — replace the
  entire block, not just the header)
- Long unexplained base64 / hex strings (≥ 32 chars) in an auth-ish
  context
- Anything passed to `Authorization:`, `Bearer`, `--token`, `--api-key`

**When in doubt, treat it as `#CRED_NNN#` and set `needs_review: true`.**
A false positive costs nothing. A missed credential is a published leak.

## Preservation rules

- Preserve everything that is not PII **verbatim**. Do not rewrite,
  summarise, translate, or "clean up" prose.
- Preserve all role labels inside `quote` (`[HUMAN]:`, `[ASSISTANT]:`,
  `[THINKING]:`, `[TOOL_INPUT:*]:`, `[TOOL_RESULT:*]:`) and blank lines
  exactly. Upstream tool-result blocks, when present, should already be
  brief supporting context rather than the main event.
- Preserve the joke. If tokenising breaks the punchline (e.g. the joke
  depends on the literal string being recognisable), placeholder it
  anyway and explain in `review_reason`.
- Preserve profanity, typos, casing, punctuation.

## Input format

You will be given a single draft entry as a JSON object with these
fields:

    {
      "title":    "...",
      "category": "...",
      "tags":     ["...", "..."],
      "quote":    "[HUMAN]: ... [ASSISTANT]: ..."
    }

Every one of those four fields can contain PII. Process all of them —
a real name in `title` must be tokenised the same way it would be in
`quote`, and a placeholder id assigned in `title` MUST match any
placeholder assigned for the same entity anywhere else in the draft.
This coreference consistency is load-bearing: downstream consumers of
this output read the title alongside the body, and a mismatch would
reveal the real identity by elimination.

## Output format

Output **only** a single JSON object with exactly these top-level keys:

    {
      "title":         "...tokenised title with #PLACEHOLDERS#...",
      "category":      "...tokenised category (usually unchanged)...",
      "tags":          ["...tokenised tags...", "..."],
      "body":          "...tokenised quote body with #PLACEHOLDERS#...",
      "entities": {
        "#USER_001#":    {"type": "USER"},
        "#CITY_001#":    {"type": "CITY",   "in": "#REGION_002#"},
        "#REGION_002#":  {"type": "REGION", "in": "#COUNTRY_003#"},
        "#COUNTRY_003#": {"type": "COUNTRY"}
      },
      "needs_review":  true,
      "review_reason": "one short sentence, or null"
    }

The `body` key holds the tokenised version of the input's `quote`
field. Every other field is its tokenised namesake.

### Hard rules

- Output ONLY the JSON object. No prose. No markdown fencing. No comments.
- Every placeholder that appears in `title`, `category`, `tags`, or
  `body` MUST appear as a key in `entities`.
- Every key in `entities` MUST appear at least once somewhere in
  `title`, `category`, `tags`, or `body`.
- Placeholder ids MUST be consistent across all four fields — the same
  real-world entity gets the same `#TYPE_NNN#` everywhere it appears.
- `needs_review` is `true` whenever ANY of the following holds:
  - You are unsure whether a span is PII.
  - You are unsure whether a token-shaped string is a credential.
  - You could not confidently coreference two mentions.
  - You had to break the joke to tokenise safely.
  - The draft contains anything you would not stake your reputation on.
- When `needs_review` is `true`, `review_reason` is a short human-readable
  sentence. When `false`, `review_reason` is `null`.

**Default to flagging.** It is always better to raise a false alarm than
to publish a real secret or a real name.
