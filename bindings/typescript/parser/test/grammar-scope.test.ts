import { describe, it, before } from "node:test";
import assert from "node:assert/strict";
import * as fs from "fs";
import * as path from "path";
import { Registry, parseRawGrammar, INITIAL } from "vscode-textmate";
import type { IGrammar } from "vscode-textmate";
import { loadWASM, OnigScanner, OnigString } from "vscode-oniguruma";

// Hermetic scope check for source.motly: load the TextMate grammar with
// vscode-textmate + Oniguruma, tokenize each corpus snippet, and assert the
// authored scope lands on the span. Needs only the grammar and the corpus — no
// parser, no build of the rest of the package. The companion conformance test
// (grammar-conformance.test.ts) checks the corpus snippets against the real
// MOTLY parser so the spec can't drift from the language.

const grammarDir = path.resolve(__dirname, "..", "grammar");
const grammarPath = path.join(grammarDir, "source.motly.tmGrammar.json");
const corpusPath = path.join(grammarDir, "corpus.json");
const onigWasmPath = path.resolve(
  __dirname,
  "..",
  "node_modules",
  "vscode-oniguruma",
  "release",
  "onig.wasm"
);

interface CorpusExpectation {
  text: string;
  scope: string;
}
interface CorpusCase {
  name: string;
  note?: string;
  code: string;
  expect: CorpusExpectation[];
}
interface Corpus {
  cases: CorpusCase[];
}

const corpus = JSON.parse(fs.readFileSync(corpusPath, "utf8")) as Corpus;

async function loadGrammar(): Promise<IGrammar> {
  const wasmBin = fs.readFileSync(onigWasmPath).buffer;
  const onigLib = loadWASM(wasmBin).then(() => ({
    createOnigScanner: (patterns: string[]) => new OnigScanner(patterns),
    createOnigString: (s: string) => new OnigString(s),
  }));
  const registry = new Registry({
    onigLib,
    loadGrammar: async (scopeName) => {
      if (scopeName === "source.motly") {
        return parseRawGrammar(fs.readFileSync(grammarPath, "utf8"), grammarPath);
      }
      return null;
    },
  });
  const grammar = await registry.loadGrammar("source.motly");
  if (!grammar) {
    throw new Error("could not load source.motly grammar");
  }
  return grammar;
}

/** Scopes the grammar assigns to the first occurrence of `text` in `code`. */
function scopesAt(
  grammar: IGrammar,
  code: string,
  text: string
): string[] | undefined {
  let ruleStack = INITIAL;
  for (const line of code.split("\n")) {
    const { tokens, ruleStack: nextStack } = grammar.tokenizeLine(line, ruleStack);
    const col = line.indexOf(text);
    if (col >= 0) {
      for (const token of tokens) {
        if (token.startIndex <= col && col < token.endIndex) {
          return token.scopes;
        }
      }
    }
    ruleStack = nextStack;
  }
  return undefined;
}

describe("source.motly — TextMate scopes", () => {
  let grammar: IGrammar;
  before(async () => {
    grammar = await loadGrammar();
  });

  for (const c of corpus.cases) {
    for (const e of c.expect) {
      it(`${c.name}: ${JSON.stringify(e.text)} -> ${e.scope}`, () => {
        const scopes = scopesAt(grammar, c.code, e.text);
        assert.ok(
          scopes !== undefined,
          `no token covers ${JSON.stringify(e.text)} in ${JSON.stringify(c.code)}`
        );
        assert.ok(
          scopes.includes(e.scope),
          `expected ${e.scope} on ${JSON.stringify(e.text)}, got ${scopes.join(", ")}`
        );
      });
    }
  }
});
