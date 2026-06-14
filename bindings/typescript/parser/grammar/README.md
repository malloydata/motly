# source.motly — TextMate grammar

Syntax highlighting for MOTLY. The grammar lives here, beside the language it
describes, and ships as a static asset in `@malloydata/motly-ts-parser` so
editors and other grammars (notably Malloy, where tag bodies *are* MOTLY) can
consume it.

## The corpus is the spec

`corpus.json` is the source of truth for *what the grammar should do*. Each case
pairs a MOTLY snippet with the scopes the grammar must assign:

```json
{ "name": "…", "code": "<motly>",
  "expect": [ { "text": "…", "scope": "<TextMate scope>" } ] }
```

It is checked two ways, so the grammar can't silently drift from the language:

1. **Scope test** — `test/grammar-scope.test.ts` tokenizes each `code` with
   `vscode-textmate` + Oniguruma and asserts every `expect[].text` span carries
   its `scope`. Hermetic: needs only the grammar and the corpus.
2. **Conformance test** — `test/grammar-conformance.test.ts` feeds each `code`
   to the real MOTLY parser (`MOTLYSession.parse`) and asserts it parses with no
   syntax errors. If the language changes such that a corpus snippet stops being
   valid MOTLY, this fails and the grammar gets revisited.

The parser exposes `parse → statements` (errors reported, not thrown) with spans
at statement granularity — no per-token lexer stream — so conformance verifies
*"the corpus stays valid MOTLY"* rather than per-span token kinds. `scope`
(authored design intent) stays decoupled from that check; the decoupling is what
stops a confidently-wrong scope from shipping green.

Both tests run under the package's normal `npm test` (they are plain
`node --test` files). `corpus.json` is hand-authored and **not** published.

## Built from the EBNF

The grammar implements `docs/motly-grammar.md` (the EBNF). Where a highlighting
choice isn't dictated by the EBNF (e.g. key-vs-value coloring), the decision is
recorded as a corpus case.

## Scope conventions

Conventional role-family scopes, each suffixed `.motly`. Themes match by *prefix*
(`keyword.operator`, `string.quoted`, …), so MOTLY colors correctly under any
theme with no custom theme rules.

| Construct | Scope |
|---|---|
| property name (key), incl. dotted / backtick | `entity.name.tag.motly` |
| `=` `:=` `:` | `keyword.operator.assignment.motly` |
| `-name` (delete), `-...` (clearAll) | `keyword.operator.negation.motly` |
| `$path` `$^` `$^.foo` reference | `variable.other.reference.motly` (`$` punctuation, `^` operator, `.` accessor, `[n]` `constant.numeric.index`) |
| `@true` `@false` `@none` | `constant.language.motly` |
| `@env.NAME` | `support.variable.motly` |
| `@<isoDate>` | `constant.other.date.motly` |
| number | `constant.numeric.motly` |
| `"…"` `"""…"""` | `string.quoted.double` / `.triple.double` + `constant.character.escape` |
| `'…'` `'''…'''` (raw) | `string.quoted.single` / `.triple.single` (no escape scope) |
| `<<<…>>>` heredoc (raw) | `string.unquoted.heredoc.motly` |
| bareString in value position | `string.unquoted.motly` |
| `# …` comment | `comment.line.number-sign.motly` |
| `{ }` / `[ ]` | `meta.block.motly` / `meta.array.motly` + `punctuation.*` |

## How key-vs-value coloring works

A bare word is a *property name* on the left of `=`/`:=`/`:` but an *unquoted
string* on the right — same characters, different role. The grammar tracks this
by **position**, not lookahead:

- `#statements` runs only in statement position (the document top and inside
  `{ }` blocks). A bare word matched there is a key.
- `=` / `:=` open a short-lived **value region**; array elements and the
  trailing-props block are the other value positions. A bare word matched there
  is an unquoted string.

The value region's `end` is *the first whitespace not consumed by a sub-rule*
(strings, arrays, and references consume their own internal whitespace). That is
the load-bearing trick: it's what makes `size=10 color=red` on one line color
`color` as a key, not as part of `size`'s value — the common Malloy-tag shape.
(A value continued onto the *next* line — legal but rare — is not covered; the
region ends at end-of-line so multi-statement files don't bleed.)

## Hazards handled

- **vscode-textmate begin-scanner prefix bug** — when begins share a literal
  prefix and differ only further in (`"""` vs `"`, `'''` vs `'`), the scanner can
  silently keep only the longest. Mitigated by listing the triple forms before
  their single-char prefix, with an explicit corpus case for every string form so
  the bug would surface immediately.
- **Recursion** — `{ }` blocks re-enter `#statements`; arrays re-enter `#value`;
  values may carry a trailing `{ }`. Standard TextMate self-reference.
- **Raw vs escape strings** — only `"` / `"""` get the `constant.character.escape`
  sub-rule; `'` / `'''` / `<<<>>>` are raw. Raw single strings still consume
  `\<char>` (without scoping it) so a `\'` doesn't falsely close the string,
  matching the parser.
- **backtick is an identifier, not a string** (EBNF) — it only appears in key
  position, scoped `entity.name.tag.motly`; there is no backtick value rule.
- **No interpolation** — MOTLY strings have none (no `%{ }`/`${ }`). Escapes are
  the fixed set `\b \f \n \r \t \uXXXX \<char>`.

## Consuming the grammar (e.g. from Malloy)

The grammar declares `scopeName: source.motly`. It is published in this package
at a stable path:

```
@malloydata/motly-ts-parser/grammar/source.motly.tmGrammar.json
```

A consumer registers it by that scope and either highlights `.motly` files
directly or embeds it by scope from another grammar — the same mechanism Malloy
uses to embed `source.sql`. Malloy already depends on this package for the
parser, so the grammar arrives with it; the Malloy-side embedding (replacing its
inline tag-body approximation with an include of `source.motly`) is a separate
change in the Malloy repo.

## Files

```
grammar/
  source.motly.tmGrammar.json   — the grammar (published)
  corpus.json                   — the spec (hand-authored; not published)
  motly.sample                  — varied syntax, for eyeballing / future tooling
  README.md                     — this file
test/
  grammar-scope.test.ts         — scope test (vscode-textmate)
  grammar-conformance.test.ts   — conformance test (real MOTLY parser)
```
