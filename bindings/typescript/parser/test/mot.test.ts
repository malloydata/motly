import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { MOTLYSession } from "../build/parser/src/index";

function mot(source: string, env?: Record<string, string | undefined>) {
  const s = new MOTLYSession();
  s.parse(source);
  return s.getMot({ env });
}

describe("Mot", () => {
  describe("existence", () => {
    it("root exists", () => {
      const m = mot("");
      assert.equal(m.exists, true);
    });

    it("existing property exists", () => {
      const m = mot("name = hello");
      assert.equal(m.get("name").exists, true);
    });

    it("missing property returns Undefined Mot", () => {
      const m = mot("name = hello");
      assert.equal(m.get("nope").exists, false);
    });

    it("Undefined Mot propagates through get()", () => {
      const m = mot("name = hello");
      assert.equal(m.get("nope", "deep", "path").exists, false);
    });

    it("has() returns true for existing path", () => {
      const m = mot("server { port = 3000 }");
      assert.equal(m.has("server", "port"), true);
    });

    it("has() returns false for missing path", () => {
      const m = mot("server { port = 3000 }");
      assert.equal(m.has("server", "host"), false);
    });
  });

  describe("valueType", () => {
    it("string", () => {
      assert.equal(mot("x = hello").get("x").valueType, "string");
    });

    it("number", () => {
      assert.equal(mot("x = 42").get("x").valueType, "number");
    });

    it("boolean", () => {
      assert.equal(mot("x = @true").get("x").valueType, "boolean");
    });

    it("date", () => {
      assert.equal(mot("x = @2024-01-15").get("x").valueType, "date");
    });

    it("array", () => {
      assert.equal(mot("x = [1, 2]").get("x").valueType, "array");
    });

    it("undefined for flag (no value)", () => {
      assert.equal(mot("x { }").get("x").valueType, undefined);
    });

    it("undefined for Undefined Mot", () => {
      assert.equal(mot("").get("nope").valueType, undefined);
    });
  });

  describe("typed accessors", () => {
    it("text returns string value", () => {
      assert.equal(mot("x = hello").get("x").text, "hello");
    });

    it("text returns undefined for number", () => {
      assert.equal(mot("x = 42").get("x").text, undefined);
    });

    it("number returns numeric value", () => {
      assert.equal(mot("x = 8080").get("x").number, 8080);
    });

    it("number returns undefined for string", () => {
      assert.equal(mot("x = hello").get("x").number, undefined);
    });

    it("boolean returns boolean value", () => {
      assert.equal(mot("x = @true").get("x").boolean, true);
      assert.equal(mot("x = @false").get("x").boolean, false);
    });

    it("date returns Date value", () => {
      const d = mot("x = @2024-01-15").get("x").date;
      assert.ok(d instanceof Date);
    });

    it("all accessors return undefined on Undefined Mot", () => {
      const u = mot("").get("nope");
      assert.equal(u.text, undefined);
      assert.equal(u.number, undefined);
      assert.equal(u.boolean, undefined);
      assert.equal(u.date, undefined);
      assert.equal(u.values, undefined);
      assert.equal(u.texts, undefined);
      assert.equal(u.numbers, undefined);
      assert.equal(u.booleans, undefined);
      assert.equal(u.dates, undefined);
    });
  });

  describe("navigation", () => {
    it("get() walks nested properties", () => {
      const m = mot("server { host = localhost\n port = 3000 }");
      assert.equal(m.get("server", "host").text, "localhost");
      assert.equal(m.get("server", "port").number, 3000);
    });

    it("get() with single arg", () => {
      assert.equal(mot("name = hello").get("name").text, "hello");
    });

    it("multi-arg get() equivalent to chained get()", () => {
      const m = mot("a { b { c = deep } }");
      assert.equal(m.get("a", "b", "c").text, "deep");
      assert.equal(m.get("a").get("b").get("c").text, "deep");
    });

    it("get() with zero args returns self", () => {
      const m = mot("x = 1");
      assert.equal(m.get().exists, true);
      assert.ok([...m.get().keys].includes("x"));
    });
  });

  describe("arrays", () => {
    it("values returns Mot array", () => {
      const m = mot("tags = [a, b, c]");
      const vals = m.get("tags").values;
      assert.ok(vals);
      assert.equal(vals.length, 3);
      assert.equal(vals[0].text, "a");
      assert.equal(vals[1].text, "b");
      assert.equal(vals[2].text, "c");
    });

    it("values returns undefined for non-array", () => {
      assert.equal(mot("x = hello").get("x").values, undefined);
    });

    it("array elements with properties", () => {
      const m = mot("items = [one { x = 1 }]");
      const vals = m.get("items").values;
      assert.ok(vals);
      assert.equal(vals[0].text, "one");
      assert.equal(vals[0].get("x").number, 1);
    });

    it("texts convenience accessor", () => {
      const t = mot("colors = [red, green, blue]").get("colors").texts;
      assert.deepStrictEqual(t, ["red", "green", "blue"]);
    });

    it("texts returns undefined if mixed types", () => {
      const t = mot("mix = [hello, 42]").get("mix").texts;
      assert.equal(t, undefined);
    });

    it("numbers convenience accessor", () => {
      const n = mot("ports = [80, 443, 8080]").get("ports").numbers;
      assert.deepStrictEqual(n, [80, 443, 8080]);
    });

    it("booleans convenience accessor", () => {
      const b = mot("flags = [@true, @false]").get("flags").booleans;
      assert.deepStrictEqual(b, [true, false]);
    });

    it("dates convenience accessor", () => {
      const d = mot("x = [@2024-01-15, @2024-06-01]").get("x").dates;
      assert.ok(d);
      assert.equal(d!.length, 2);
      assert.ok(d![0] instanceof Date);
      assert.ok(d![1] instanceof Date);
    });

    it("dates returns undefined for non-date elements", () => {
      assert.equal(mot("x = [@2024-01-15, hello]").get("x").dates, undefined);
    });

    it("texts works when elements have properties", () => {
      const m = mot('x = ["a" { p = 1 }, "b" { q = 2 }]');
      assert.deepStrictEqual(m.get("x").texts, ["a", "b"]);
      const vals = m.get("x").values!;
      assert.equal(vals[0].get("p").number, 1);
      assert.equal(vals[1].get("q").number, 2);
    });

    it("numbers works when elements have properties", () => {
      const m = mot("x = [10 { unit = ms }, 20 { unit = s }]");
      assert.deepStrictEqual(m.get("x").numbers, [10, 20]);
    });

    it("texts returns undefined when any element is property-only", () => {
      const m = mot('x = ["a", { p = 1 }]');
      assert.equal(m.get("x").texts, undefined);
    });

    it("property-only array elements have no typed value", () => {
      const m = mot("x = [{ name = alice }]");
      const vals = m.get("x").values!;
      assert.equal(vals[0].text, undefined);
      assert.equal(vals[0].valueType, undefined);
      assert.equal(vals[0].get("name").text, "alice");
    });

    it("reference elements resolve for convenience accessors", () => {
      const m = mot("target = hello\nlist = [$target, world]");
      assert.deepStrictEqual(m.get("list").texts, ["hello", "world"]);
    });

    it("numbers returns undefined for mixed types", () => {
      assert.equal(mot("x = [1, hello]").get("x").numbers, undefined);
    });

    it("booleans returns undefined for mixed types", () => {
      assert.equal(mot("x = [@true, 1]").get("x").booleans, undefined);
    });

    it("empty array returns empty convenience arrays", () => {
      const m = mot("x = []");
      assert.deepStrictEqual(m.get("x").texts, []);
      assert.deepStrictEqual(m.get("x").numbers, []);
      assert.deepStrictEqual(m.get("x").booleans, []);
      assert.deepStrictEqual(m.get("x").dates, []);
    });

    it("nested arrays: outer values are arrays, not scalars", () => {
      const m = mot("x = [[1, 2], [3, 4]]");
      const outer = m.get("x").values!;
      assert.equal(outer.length, 2);
      assert.equal(outer[0].valueType, "array");
      assert.deepStrictEqual(outer[0].numbers, [1, 2]);
      assert.deepStrictEqual(outer[1].numbers, [3, 4]);
      // outer convenience accessors don't work (elements are arrays, not scalars)
      assert.equal(m.get("x").numbers, undefined);
      assert.equal(m.get("x").texts, undefined);
    });
  });

  describe("property enumeration", () => {
    it("keys iterates property names", () => {
      const m = mot("a = 1\nb = 2\nc = 3");
      const keys = [...m.keys];
      assert.ok(keys.includes("a"));
      assert.ok(keys.includes("b"));
      assert.ok(keys.includes("c"));
      assert.equal(keys.length, 3);
    });

    it("entries iterates [name, Mot] pairs", () => {
      const m = mot("x = 10\ny = 20");
      const entries = [...m.entries];
      assert.equal(entries.length, 2);
      for (const [key, val] of entries) {
        assert.ok(typeof key === "string");
        assert.ok(val.exists);
      }
    });

    it("keys is empty for Undefined Mot", () => {
      assert.deepStrictEqual([...mot("").get("nope").keys], []);
    });

    it("entries is empty for Undefined Mot", () => {
      assert.deepStrictEqual([...mot("").get("nope").entries], []);
    });
  });

  describe("references", () => {
    it("resolves absolute $ref", () => {
      const m = mot("defaults { color = red }\ntheme = $defaults");
      assert.equal(m.get("theme", "color").text, "red");
    });

    it("resolves chained $ref", () => {
      const m = mot("a { x = 1 }\nb = $a\nc = $b");
      assert.equal(m.get("c", "x").number, 1);
    });

    it("resolves deep ref path", () => {
      const m = mot("config { db { host = localhost } }\ndbhost = $config.db.host");
      assert.equal(m.get("dbhost").text, "localhost");
    });

    it("resolves relative ^ ref", () => {
      const m = mot("parent { x = 1\n inner { child = $^x } }");
      assert.equal(m.get("parent", "inner", "child").number, 1);
    });

    it("resolves $^^ (two ups)", () => {
      const m = mot("x = root_x\n a { b { ref = $^^x } }");
      assert.equal(m.get("a", "b", "ref").text, "root_x");
    });

    it("resolves ref inside array element", () => {
      const m = mot("target = hello\nlist = [$target]");
      const vals = m.get("list").values;
      assert.ok(vals);
      assert.equal(vals[0].text, "hello");
    });

    it("resolves multi-segment absolute ref to a chained relative ref", () => {
      const m = mot("a { y = found\n b { c = $^y } }\nalias = $a.b.c");
      assert.equal(m.get("alias").text, "found");
    });

    it("unresolved ref returns Undefined Mot", () => {
      const m = mot("a = $nonexistent");
      assert.equal(m.get("a").exists, false);
    });

    it("^ ref that escapes root returns Undefined Mot", () => {
      const m = mot("a = $^x");
      assert.equal(m.get("a").exists, false);
    });

    it("pure circular refs (no backing node) become Undefined Mot", () => {
      const m = mot("a = $b\nb = $a");
      assert.equal(m.get("a").exists, false);
      assert.equal(m.get("b").exists, false);
    });

    it("circular refs with a backing node share Mot instances", () => {
      const m = mot("a { x = 1 }\na.ref = $b\nb = $a");
      assert.equal(m.get("a", "x").number, 1);
      assert.equal(m.get("b", "x").number, 1);
      // b → $a → concrete node with x=1, ref pointing back to b
      assert.equal(m.get("b", "ref", "x").number, 1);
    });

    it("resolves backtick-quoted ^ property via relative ref", () => {
      const m = mot("a { `^` = 7\n b { y = $^`^` } }");
      assert.equal(m.get("a", "b", "y").number, 7);
    });
  });

  describe("env refs", () => {
    it("resolves @env from env map", () => {
      const m = mot("key = @env.MY_VAR", { MY_VAR: "custom" });
      assert.equal(m.get("key").text, "custom");
    });

    it("missing env var has no value", () => {
      const m = mot("key = @env.NOPE", {});
      assert.equal(m.get("key").text, undefined);
      assert.equal(m.get("key").valueType, undefined);
    });

    it("no env map means no value", () => {
      const m = mot("key = @env.SOME_VAR");
      assert.equal(m.get("key").text, undefined);
    });
  });

  describe("deletions", () => {
    it("deleted nodes do not appear", () => {
      const m = mot("a = 1\nb = 2\n-b");
      assert.equal(m.get("a").number, 1);
      assert.equal(m.get("b").exists, false);
      assert.ok(![...m.keys].includes("b"));
    });
  });

  describe("flags", () => {
    it("flag node exists but has no value", () => {
      const m = mot("enabled { }");
      assert.equal(m.get("enabled").exists, true);
      assert.equal(m.get("enabled").valueType, undefined);
      assert.equal(m.get("enabled").text, undefined);
    });
  });

  describe("node with both value and properties", () => {
    it("value and properties both accessible", () => {
      const m = mot("item := hello { sub = world }");
      assert.equal(m.get("item").text, "hello");
      assert.equal(m.get("item", "sub").text, "world");
    });
  });

  describe("empty session", () => {
    it("empty session returns root Mot with no value and no properties", () => {
      const m = mot("");
      assert.equal(m.exists, true);
      assert.equal(m.valueType, undefined);
      assert.deepStrictEqual([...m.keys], []);
    });
  });

  describe("session integration", () => {
    it("getMot throws after dispose", () => {
      const s = new MOTLYSession();
      s.dispose();
      assert.throws(() => s.getMot(), /disposed/);
    });
  });
});
