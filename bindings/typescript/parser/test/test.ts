import { describe, it } from "node:test";
import assert from "node:assert/strict";
import * as fs from "fs";
import * as path from "path";
import {
  MOTLYSession,
} from "../build/parser/src/index";

// ── Fixture loading ─────────────────────────────────────────────

const fixturesDir = path.resolve(__dirname, "..", "..", "..", "..", "test-data", "fixtures");

function loadFixtures<T>(name: string): T {
  const raw = fs.readFileSync(path.join(fixturesDir, name), "utf-8");
  return JSON.parse(raw) as T;
}

// ── Helpers ─────────────────────────────────────────────────────

/** Convert $date objects in fixture expected values to Date objects. */
function hydrateValue(v: any): any {
  if (v === null || v === undefined) return v;
  if (typeof v !== "object") return v;
  if ("$date" in v && typeof v.$date === "string") {
    return new Date(v.$date);
  }
  if (Array.isArray(v)) {
    return v.map(hydrateValue);
  }
  const result: any = {};
  for (const key of Object.keys(v)) {
    result[key] = hydrateValue(v[key]);
  }
  return result;
}

/** Deep equality for MOTLYValue that handles Date objects. */
function deepEqual(actual: any, expected: any): void {
  if (expected instanceof Date) {
    assert.ok(actual instanceof Date, `Expected Date, got ${typeof actual}: ${JSON.stringify(actual)}`);
    assert.equal(actual.getTime(), expected.getTime(), `Date mismatch: ${actual.toISOString()} vs ${expected.toISOString()}`);
    return;
  }
  if (Array.isArray(expected)) {
    assert.ok(Array.isArray(actual), `Expected array, got ${typeof actual}`);
    assert.equal(actual.length, expected.length, `Array length mismatch: ${actual.length} vs ${expected.length}`);
    for (let i = 0; i < expected.length; i++) {
      deepEqual(actual[i], expected[i]);
    }
    return;
  }
  if (typeof expected === "object" && expected !== null) {
    assert.ok(typeof actual === "object" && actual !== null, `Expected object, got ${typeof actual}`);
    const expectedKeys = Object.keys(expected).sort();
    const actualKeys = Object.keys(actual).sort();
    assert.deepStrictEqual(actualKeys, expectedKeys, `Key mismatch: ${JSON.stringify(actualKeys)} vs ${JSON.stringify(expectedKeys)}`);
    for (const key of expectedKeys) {
      deepEqual(actual[key], expected[key]);
    }
    return;
  }
  assert.strictEqual(actual, expected);
}

/** Sort errors by (code, path) for order-independent comparison. */
function sortErrors<T extends { code: string; path?: string[] }>(errors: T[]): T[] {
  return [...errors].sort((a, b) => {
    const codeCompare = a.code.localeCompare(b.code);
    if (codeCompare !== 0) return codeCompare;
    const aPath = (a.path ?? []).join("/");
    const bPath = (b.path ?? []).join("/");
    return aPath.localeCompare(bPath);
  });
}

// ── Parse fixtures ──────────────────────────────────────────────

interface ParseFixture {
  name: string;
  input: string | string[];
  expected?: any;
  expectErrors?: boolean;
}

const parseFixtures = loadFixtures<ParseFixture[]>("parse.json");

describe("Parse fixtures", () => {
  for (const fixture of parseFixtures) {
    it(fixture.name, () => {
      const s = new MOTLYSession();

      if (Array.isArray(fixture.input)) {
        for (const chunk of fixture.input) {
          const errors = s.parse(chunk);
          if (!fixture.expectErrors) {
            assert.deepStrictEqual(errors, [], `Unexpected parse errors: ${JSON.stringify(errors)}`);
          }
        }
      } else {
        const errors = s.parse(fixture.input);
        if (fixture.expectErrors) {
          assert.ok(errors.length > 0, "Expected parse errors");
          if (fixture.expected === undefined) {
            s.dispose();
            return;
          }
          // expectErrors + expected: errors are non-fatal, check the tree too
        } else {
          assert.deepStrictEqual(errors, [], `Unexpected parse errors: ${JSON.stringify(errors)}`);
        }
      }

      if (fixture.expected !== undefined) {
        const value = s.getValue();
        const expected = hydrateValue(fixture.expected);
        deepEqual(value, expected);
      }

      s.dispose();
    });
  }
});

// ── Parse error fixtures ────────────────────────────────────────

interface ParseErrorFixture {
  name: string;
  input: string;
  expectErrors: boolean;
}

const parseErrorFixtures = loadFixtures<ParseErrorFixture[]>("parse-errors.json");

describe("Parse error fixtures", () => {
  for (const fixture of parseErrorFixtures) {
    it(fixture.name, () => {
      const s = new MOTLYSession();
      const errors = s.parse(fixture.input);
      assert.ok(errors.length > 0, `Expected parse errors for: ${fixture.input}`);
      assert.ok(errors[0].code.length > 0);
      assert.ok(errors[0].message.length > 0);
      s.dispose();
    });
  }
});

// ── Schema fixtures ─────────────────────────────────────────────

interface SchemaFixture {
  name: string;
  schema: string;
  input: string;
  expectedErrors: { code: string; path?: string[] }[];
}

const schemaFixtures = loadFixtures<SchemaFixture[]>("schema.json");

describe("Schema fixtures", () => {
  for (const fixture of schemaFixtures) {
    it(fixture.name, () => {
      const s = new MOTLYSession();
      const schemaErrors = s.parseSchema(fixture.schema);
      assert.deepStrictEqual(schemaErrors, [], `Schema parse errors: ${JSON.stringify(schemaErrors)}`);
      const parseErrors = s.parse(fixture.input);
      assert.deepStrictEqual(parseErrors, [], `Parse errors: ${JSON.stringify(parseErrors)}`);
      const errors = s.validateSchema();

      const sortedActual = sortErrors(errors);
      const sortedExpected = sortErrors(fixture.expectedErrors);

      assert.equal(
        sortedActual.length,
        sortedExpected.length,
        `Error count mismatch for "${fixture.name}": got ${sortedActual.length} errors [${sortedActual.map(e => e.code).join(", ")}], expected ${sortedExpected.length}`
      );

      for (let i = 0; i < sortedExpected.length; i++) {
        assert.equal(sortedActual[i].code, sortedExpected[i].code,
          `Error code mismatch at sorted index ${i}`);
        if (sortedExpected[i].path) {
          assert.deepStrictEqual(sortedActual[i].path, sortedExpected[i].path,
            `Error path mismatch at sorted index ${i}`);
        }
      }

      s.dispose();
    });
  }
});

// ── Reference fixtures ──────────────────────────────────────────

interface RefFixture {
  name: string;
  input: string;
  expectedErrors: { code: string; path?: string[] }[];
}

const refFixtures = loadFixtures<RefFixture[]>("refs.json");

describe("Reference fixtures", () => {
  for (const fixture of refFixtures) {
    it(fixture.name, () => {
      const s = new MOTLYSession();
      const parseErrors = s.parse(fixture.input);
      assert.deepStrictEqual(parseErrors, [], `Parse errors: ${JSON.stringify(parseErrors)}`);
      const errors = s.validateReferences();

      const sortedActual = sortErrors(errors);
      const sortedExpected = sortErrors(fixture.expectedErrors);

      assert.equal(
        sortedActual.length,
        sortedExpected.length,
        `Error count mismatch for "${fixture.name}": got ${sortedActual.length}, expected ${sortedExpected.length}`
      );

      for (let i = 0; i < sortedExpected.length; i++) {
        assert.equal(sortedActual[i].code, sortedExpected[i].code);
        if (sortedExpected[i].path) {
          assert.deepStrictEqual(sortedActual[i].path, sortedExpected[i].path);
        }
      }

      s.dispose();
    });
  }
});

// ── Session fixtures ────────────────────────────────────────────

interface SessionStep {
  action: string;
  input?: string;
  expected?: any;
  expectedErrors?: { code: string }[];
  expectErrors?: boolean;
}

interface SessionFixture {
  name: string;
  steps: SessionStep[];
}

const sessionFixtures = loadFixtures<SessionFixture[]>("session.json");

describe("Session fixtures", () => {
  for (const fixture of sessionFixtures) {
    it(fixture.name, () => {
      const s = new MOTLYSession();

      for (const step of fixture.steps) {
        switch (step.action) {
          case "parse": {
            const errors = s.parse(step.input!);
            if (step.expectErrors) {
              assert.ok(errors.length > 0, "Expected parse errors");
            } else {
              assert.deepStrictEqual(errors, [], `Unexpected parse errors: ${JSON.stringify(errors)}`);
            }
            break;
          }
          case "parseSchema": {
            const errors = s.parseSchema(step.input!);
            assert.deepStrictEqual(errors, []);
            break;
          }
          case "reset":
            s.reset();
            break;
          case "getValue": {
            const value = s.getValue();
            if (step.expected !== undefined) {
              const expected = hydrateValue(step.expected);
              deepEqual(value, expected);
            }
            break;
          }
          case "validateSchema": {
            const errors = s.validateSchema();
            if (step.expectedErrors !== undefined) {
              assert.equal(errors.length, step.expectedErrors.length,
                `Schema error count mismatch: got ${errors.length}, expected ${step.expectedErrors.length}`);
              for (let i = 0; i < step.expectedErrors.length; i++) {
                assert.equal(errors[i].code, step.expectedErrors[i].code);
              }
            }
            break;
          }
          case "validateReferences": {
            const errors = s.validateReferences();
            if (step.expectedErrors !== undefined) {
              assert.equal(errors.length, step.expectedErrors.length);
              for (let i = 0; i < step.expectedErrors.length; i++) {
                assert.equal(errors[i].code, step.expectedErrors[i].code);
              }
            }
            break;
          }
        }
      }

      s.dispose();
    });
  }
});

// ── Lifecycle tests ─────────────────────────────────────────────

describe("MOTLYSession lifecycle", () => {
  it("throws after dispose", () => {
    const s = new MOTLYSession();
    s.dispose();
    assert.throws(() => s.parse("x = 1"), /disposed/);
    assert.throws(() => s.parseSchema("Required { x = string }"), /disposed/);
    assert.throws(() => s.reset(), /disposed/);
    assert.throws(() => s.getValue(), /disposed/);
    assert.throws(() => s.validateSchema(), /disposed/);
    assert.throws(() => s.validateReferences(), /disposed/);
  });

  it("dispose is idempotent", () => {
    const s = new MOTLYSession();
    s.dispose();
    s.dispose(); // should not throw
  });
});
