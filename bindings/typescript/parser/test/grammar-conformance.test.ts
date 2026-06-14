import { describe, it } from "node:test";
import assert from "node:assert/strict";
import * as fs from "fs";
import * as path from "path";
import { MOTLYSession } from "../build/parser/src/index";

// Conformance guard: every snippet in the grammar corpus must be valid MOTLY.
// We feed each `code` to the real parser and assert it produces no syntax
// errors. parse() defers semantic errors (e.g. unresolved references) to
// finish(), which is exactly what we want — the corpus pins highlighting, not
// resolution. This is the depth the parser API supports: it exposes parse ->
// statements (errors reported, not thrown) and spans at statement granularity,
// but no per-token lexer stream, so we verify "the corpus stays valid MOTLY"
// rather than per-span token kinds.

const corpusPath = path.resolve(
  __dirname,
  "..",
  "grammar",
  "corpus.json"
);

interface CorpusCase {
  name: string;
  note?: string;
  code: string;
}
interface Corpus {
  cases: CorpusCase[];
}

const corpus = JSON.parse(fs.readFileSync(corpusPath, "utf8")) as Corpus;

describe("source.motly corpus — parses as valid MOTLY", () => {
  for (const c of corpus.cases) {
    it(c.name, () => {
      const session = new MOTLYSession();
      const result = session.parse(c.code);
      assert.equal(
        result.errors.length,
        0,
        `expected ${JSON.stringify(c.code)} to parse clean, got: ${result.errors
          .map((e) => e.message)
          .join("; ")}`
      );
    });
  }
});
