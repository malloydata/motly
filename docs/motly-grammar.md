# MOTLY Grammar (EBNF)

Changes from the previous grammar:

- **Added** `assignBoth` production for the `:=` operator.
- **Added** `none` literal (`@none`) to the `value` production.
- **Added** `heredocString` (`<<<...>>>`) to the `string` production — a raw multi-line string with automatic dedentation based on the first non-empty line's indentation.
- **Removed** `propName "=" [ "..." ] properties` from `replaceProps` (was a synonym for `:`; confusing under new semantics where `=` only touches the value slot).
- **Removed** `"{" "..." "}"` from `properties` (the `{ ... }` preserve-properties form is superseded by the orthogonality of `=` and `:`).
- **Renamed** statement productions for clarity: `assignValue`, `assignBoth`, `replaceProps`, `mergeProps`.

```ebnf
(* Entry point — commas are optional separators between statements *)
document        ::= statementList
statementList   ::= { "," } { statement { "," } }

(* ================================================================
   Statements
   ================================================================
   assignValue   — sets the value slot only; properties unchanged.
                   Optional trailing { } merges additional properties.
   assignBoth    — sets value AND replaces properties from the source.
                   With a reference, clones value + subtree.
                   Optional trailing { } merges overrides on top.
   replaceProps  — replaces properties only; value unchanged.
   mergeProps    — merges into existing properties; value unchanged.
   definition    — creates a flag (exists with no value/properties),
                   or with "-" prefix, deletes a property.
   clearAll      — removes all properties from current scope.
   ================================================================ *)
statement       ::= assignValue
                  | assignBoth
                  | replaceProps
                  | mergeProps
                  | clearAll
                  | definition

assignValue     ::= propName "=" value [ properties ]
assignBoth      ::= propName ":=" value [ properties ]
replaceProps    ::= propName ":" properties
mergeProps      ::= propName properties
definition      ::= [ "-" ] propName
clearAll        ::= "-..."

(* Property paths *)
propName        ::= identifier { "." identifier }

(* Values *)
value           ::= array | boolean | none | date | number | string | reference

boolean         ::= "@true" | "@false"
none            ::= "@none"
date            ::= "@" isoDate
number          ::= [ "-" ] digits [ "." digits ] [ exponent ]
                  | [ "-" ] "." digits [ exponent ]
string          ::= tripleString | tripleSingleString | heredocString | sqString | dqString | bareString
reference       ::= "$" { "^" } refPath
refPath         ::= refElement { "." refElement }
refElement      ::= identifier [ "[" digits "]" ]

exponent        ::= ( "e" | "E" ) [ "+" | "-" ] digits
digits          ::= digit { digit }
digit           ::= "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"

(* ISO 8601 date/datetime *)
isoDate         ::= year "-" month "-" day [ "T" time [ timezone ] ]
time            ::= hour ":" minute [ ":" second [ "." fraction ] ]
timezone        ::= "Z" | ( "+" | "-" ) hour [ ":" ] minute
year            ::= digit digit digit digit
month           ::= digit digit
day             ::= digit digit
hour            ::= digit digit
minute          ::= digit digit
second          ::= digit digit
fraction        ::= digits

(* Arrays *)
array           ::= "[" [ arrayElements ] "]"
arrayElements   ::= arrayElement { "," arrayElement } [ "," ]
arrayElement    ::= scalarValue [ properties ]
                  | properties
                  | array

scalarValue     ::= boolean | none | date | number | string

(* Properties block *)
properties      ::= "{" statementList "}"

(* Identifiers — for property names *)
identifier      ::= bqString | bareString

(* String literals *)
bareString      ::= bareChar { bareChar }
bareChar        ::= letter | digit | "_"
letter          ::= "A"-"Z" | "a"-"z" | extendedLatin
extendedLatin   ::= (* Unicode: U+00C0–U+024F, U+1E00–U+1EFF *)

tripleString    ::= '"""' { tripleChar } '"""'
tripleChar      ::= (* any character except unescaped """, or escape sequence *)

tripleSingleString ::= "'''" { tripleSingleChar } "'''"
tripleSingleChar   ::= (* any character; backslash pairs with next char; only ''' closes *)

heredocString      ::= "<<<" { heredocChar } ">>>"
heredocChar        ::= (* any character; raw semantics; only >>> closes *)
                       (* The indentation of the first non-empty line sets the baseline.
                          That amount of leading whitespace is stripped from all lines. *)

dqString        ::= '"' { dqChar } '"'
dqChar          ::= (* any character except ", \, newline, or escape sequence *)

sqString        ::= "'" { sqChar } "'"
sqChar          ::= (* any character except ', newline; backslash pairs with next char literally *)

bqString        ::= "`" { bqChar } "`"
bqChar          ::= (* any character except `, \, newline, or escape sequence *)

(* Escape sequences (dqString, tripleString, bqString): \b \f \n \r \t \uXXXX \char *)
(* Raw strings (sqString, tripleSingleString, heredocString): backslash is literal *)

(* Whitespace and comments — allowed between tokens *)
whitespace      ::= " " | "\t" | "\r" | "\n"
comment         ::= "#" { (* any char except newline *) } newline
```
