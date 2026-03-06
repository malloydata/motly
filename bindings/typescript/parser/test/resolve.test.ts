import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { MOTLYSession } from "../build/parser/src/index";

describe("resolve()", () => {
  it("resolves scalar eq only", () => {
    const s = new MOTLYSession();
    s.parse("name = hello");
    const result = s.resolve() as any;
    assert.deepStrictEqual(result, { name: "hello" });
  });

  it("resolves number", () => {
    const s = new MOTLYSession();
    s.parse("port = 8080");
    assert.deepStrictEqual(s.resolve(), { port: 8080 });
  });

  it("resolves boolean", () => {
    const s = new MOTLYSession();
    s.parse("enabled = @true");
    assert.deepStrictEqual(s.resolve(), { enabled: true });
  });

  it("resolves date", () => {
    const s = new MOTLYSession();
    s.parse("created = @2024-01-15");
    const result = s.resolve() as any;
    assert.ok(result.created instanceof Date);
  });

  it("resolves nested properties", () => {
    const s = new MOTLYSession();
    s.parse("server { host = localhost\n port = 3000 }");
    assert.deepStrictEqual(s.resolve(), {
      server: { host: "localhost", port: 3000 },
    });
  });

  it("resolves node with both eq and properties", () => {
    const s = new MOTLYSession();
    s.parse("item := hello { sub = world }");
    assert.deepStrictEqual(s.resolve(), {
      item: { "=": "hello", sub: "world" },
    });
  });

  it("resolves arrays", () => {
    const s = new MOTLYSession();
    s.parse("tags = [a, b, c]");
    const result = s.resolve() as any;
    assert.deepStrictEqual(result, { tags: ["a", "b", "c"] });
  });

  it("resolves array elements with properties", () => {
    const s = new MOTLYSession();
    s.parse("items = [one { x = 1 }]");
    const result = s.resolve() as any;
    assert.deepStrictEqual(result, {
      items: [{ "=": "one", x: 1 }],
    });
  });

  it("resolves $ref links", () => {
    const s = new MOTLYSession();
    s.parse("defaults { color = red }\ntheme = $defaults");
    const result = s.resolve() as any;
    assert.deepStrictEqual(result, {
      defaults: { color: "red" },
      theme: { color: "red" },
    });
  });

  it("resolves chained $ref links", () => {
    const s = new MOTLYSession();
    s.parse("a { x = 1 }\nb = $a\nc = $b");
    const result = s.resolve() as any;
    assert.deepStrictEqual(result, {
      a: { x: 1 },
      b: { x: 1 },
      c: { x: 1 },
    });
  });

  it("throws on circular references", () => {
    const s = new MOTLYSession();
    s.parse("a = $b\nb = $a");
    assert.throws(() => s.resolve(), /[Cc]ircular/);
  });

  it("throws on unresolved references", () => {
    const s = new MOTLYSession();
    s.parse("a = $nonexistent");
    assert.throws(() => s.resolve(), /[Uu]nresolved/);
  });

  it("returns undefined for env refs when no env map provided", () => {
    const s = new MOTLYSession();
    s.parse("key = @env.SOME_VAR");
    const result = s.resolve() as any;
    assert.equal(result.key, undefined);
  });

  it("resolves @env refs from custom env map", () => {
    const s = new MOTLYSession();
    s.parse("key = @env.MY_VAR");
    const result = s.resolve({ env: { MY_VAR: "custom" } }) as any;
    assert.equal(result.key, "custom");
  });

  it("returns undefined for missing env var", () => {
    const s = new MOTLYSession();
    s.parse("key = @env.DOES_NOT_EXIST_XYZ");
    const result = s.resolve({ env: {} }) as any;
    assert.equal(result.key, undefined);
  });

  it("omits deleted nodes", () => {
    const s = new MOTLYSession();
    s.parse("a = 1\nb = 2\n-b");
    const result = s.resolve() as any;
    assert.deepStrictEqual(result, { a: 1 });
  });

  it("resolves empty session to empty object", () => {
    const s = new MOTLYSession();
    assert.deepStrictEqual(s.resolve(), {});
  });

  it("throws after dispose", () => {
    const s = new MOTLYSession();
    s.dispose();
    assert.throws(() => s.resolve(), /disposed/);
  });

  it("resolves deep nested ref path", () => {
    const s = new MOTLYSession();
    s.parse("config { db { host = localhost } }\ndbhost = $config.db.host");
    const result = s.resolve() as any;
    assert.equal(result.dbhost, "localhost");
  });

  it("resolves relative ^ ref", () => {
    const s = new MOTLYSession();
    // $^ from inner goes up to parent, finds sibling x
    s.parse("parent { x = 1\n inner { child = $^x } }");
    const result = s.resolve() as any;
    assert.deepStrictEqual(result, {
      parent: { x: 1, inner: { child: 1 } },
    });
  });

  it("throws on ^ ref that escapes root", () => {
    const s = new MOTLYSession();
    s.parse("a = $^x");
    assert.throws(() => s.resolve(), /[Uu]nresolved/);
  });

  it("resolves ref chain through relative refs at different depths", () => {
    const s = new MOTLYSession();
    s.parse("top { x = hello\n mid { x = $^x\n bottom { x = $^x } } }");
    const result = s.resolve() as any;
    assert.deepStrictEqual(result, {
      top: { x: "hello", mid: { x: "hello", bottom: { x: "hello" } } },
    });
  });

  it("resolves $^^ (two ups)", () => {
    const s = new MOTLYSession();
    s.parse("x = root_x\n a { b { ref = $^^x } }");
    const result = s.resolve() as any;
    assert.equal(result.a.b.ref, "root_x");
  });

  it("resolves ref inside array element", () => {
    const s = new MOTLYSession();
    s.parse("target = hello\nlist = [$target]");
    const result = s.resolve() as any;
    assert.deepStrictEqual(result, { target: "hello", list: ["hello"] });
  });

  it("resolves multi-segment absolute ref to a chained relative ref", () => {
    const s = new MOTLYSession();
    s.parse("a { y = found\n b { c = $^y } }\nalias = $a.b.c");
    const result = s.resolve() as any;
    assert.equal(result.alias, "found");
  });

  it("resolves backtick-quoted ^ property via relative ref", () => {
    const s = new MOTLYSession();
    s.parse("a { `^` = 7\n b { y = $^`^` } }");
    const result = s.resolve() as any;
    assert.equal(result.a.b.y, 7);
  });
});
