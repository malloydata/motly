import {
  Statement,
  ScalarValue,
  TagValue,
  ArrayElement,
  RefPathSegment,
} from "./ast";
import { MOTLYError } from "motly-ts-interface";

interface Position {
  line: number;
  column: number;
  offset: number;
}

class Parser {
  private input: string;
  private pos: number;

  constructor(input: string) {
    this.input = input;
    this.pos = 0;
  }

  // ── Helpers ──────────────────────────────────────────────────────

  private peekChar(): string | undefined {
    return this.pos < this.input.length ? this.input[this.pos] : undefined;
  }

  private advance(n: number): void {
    this.pos += n;
  }

  private startsWith(s: string): boolean {
    return this.input.startsWith(s, this.pos);
  }

  private eatChar(ch: string): boolean {
    if (this.peekChar() === ch) {
      this.advance(1);
      return true;
    }
    return false;
  }

  private expectChar(ch: string): void {
    if (!this.eatChar(ch)) {
      throw this.errorPoint(`Expected '${ch}'`);
    }
  }

  private position(): Position {
    const consumed = this.input.substring(0, this.pos);
    const line = (consumed.match(/\n/g) || []).length;
    const lastNewline = consumed.lastIndexOf("\n");
    const column = this.pos - (lastNewline === -1 ? 0 : lastNewline + 1);
    return { line, column, offset: this.pos };
  }

  private errorPoint(message: string): MOTLYError {
    const pos = this.position();
    return {
      code: "tag-parse-syntax-error",
      message,
      begin: pos,
      end: pos,
    };
  }

  private errorSpan(message: string, begin: Position): MOTLYError {
    return {
      code: "tag-parse-syntax-error",
      message,
      begin,
      end: this.position(),
    };
  }

  // ── Whitespace & Comments ───────────────────────────────────────

  private skipWs(): void {
    for (;;) {
      while (this.pos < this.input.length) {
        const ch = this.input[this.pos];
        if (ch === " " || ch === "\t" || ch === "\r" || ch === "\n") {
          this.pos++;
        } else {
          break;
        }
      }
      if (this.peekChar() === "#") {
        while (this.pos < this.input.length) {
          const ch = this.input[this.pos];
          if (ch === "\r" || ch === "\n") break;
          this.pos++;
        }
      } else {
        break;
      }
    }
  }

  /**
   * Like `skipWs`, but also eats commas. Used in statement-list
   * contexts (top-level document and properties blocks) so commas
   * can serve as optional separators between statements.
   */
  private skipWsAndCommas(): void {
    this.skipWs();
    while (this.peekChar() === ",") {
      this.advance(1);
      this.skipWs();
    }
  }

  // ── Statement Dispatch ──────────────────────────────────────────

  parseStatements(): Statement[] {
    const statements: Statement[] = [];
    this.skipWsAndCommas();
    while (this.pos < this.input.length) {
      statements.push(this.parseStatement());
      this.skipWsAndCommas();
    }
    return statements;
  }

  private parseStatement(): Statement {
    // -... (clearAll)
    if (this.startsWith("-...")) {
      this.advance(4);
      return { kind: "clearAll" };
    }

    // -name (define deleted)
    if (this.peekChar() === "-") {
      this.advance(1);
      const path = this.parsePropName();
      return { kind: "define", path, deleted: true };
    }

    // Parse the property path
    const path = this.parsePropName();
    this.skipWs();

    const ch = this.peekChar();

    // Check := first (MUST check before : alone)
    if (ch === ":" && this.startsWith(":=")) {
      this.advance(2);
      this.skipWs();
      const value = this.parseEqValue();
      this.skipWs();
      if (this.peekChar() === "{") {
        const props = this.parsePropertiesBlock();
        return { kind: "assignBoth", path, value, properties: props };
      }
      return { kind: "assignBoth", path, value, properties: null };
    }

    if (ch === "=") {
      this.advance(1);
      this.skipWs();

      // = { is now a parse error (= requires a value)
      if (this.peekChar() === "{") {
        throw this.errorPoint(
          "'=' requires a value; use ': { ... }' to replace properties"
        );
      }

      // = value
      const value = this.parseEqValue();
      this.skipWs();

      // Optional { props } block (MERGE semantics)
      if (this.peekChar() === "{") {
        const props = this.parsePropertiesBlock();
        return { kind: "setEq", path, value, properties: props };
      }

      return { kind: "setEq", path, value, properties: null };
    }

    if (ch === ":") {
      this.advance(1);
      this.skipWs();
      const props = this.parsePropertiesBlock();
      return { kind: "replaceProperties", path, properties: props };
    }

    if (ch === "{") {
      const props = this.parsePropertiesBlock();
      return { kind: "updateProperties", path, properties: props };
    }

    return { kind: "define", path, deleted: false };
  }

  // ── Property Name (dotted path) ─────────────────────────────────

  private parsePropName(): string[] {
    const first = this.parseIdentifier();
    const path = [first];
    while (this.peekChar() === ".") {
      this.advance(1);
      path.push(this.parseIdentifier());
    }
    return path;
  }

  private parseIdentifier(): string {
    if (this.peekChar() === "`") {
      return this.parseBacktickString();
    }
    return this.parseBareString();
  }

  // ── Values ──────────────────────────────────────────────────────

  private parseEqValue(allowArrays = true): TagValue {
    const ch = this.peekChar();
    if (allowArrays && ch === "[") return { kind: "array", elements: this.parseArray() };
    if (this.startsWith("<<<")) {
      return {
        kind: "scalar",
        value: { kind: "string", value: this.parseHeredoc() },
      };
    }
    if (ch === "@")
      return { kind: "scalar", value: this.parseAtValue() };
    if (ch === "$")
      return { kind: "scalar", value: this.parseReference() };
    if (ch === '"') {
      if (this.startsWith('"""')) {
        return {
          kind: "scalar",
          value: { kind: "string", value: this.parseTripleString() },
        };
      }
      return {
        kind: "scalar",
        value: { kind: "string", value: this.parseDoubleQuotedString() },
      };
    }
    if (ch === "'") {
      if (this.startsWith("'''")) {
        return {
          kind: "scalar",
          value: {
            kind: "string",
            value: this.parseTripleSingleQuotedString(),
          },
        };
      }
      return {
        kind: "scalar",
        value: { kind: "string", value: this.parseSingleQuotedString() },
      };
    }
    if (
      ch !== undefined &&
      (ch === "-" || (ch >= "0" && ch <= "9") || ch === ".")
    ) {
      return this.parseNumberOrString();
    }
    if (ch !== undefined && isBareChar(ch)) {
      return {
        kind: "scalar",
        value: { kind: "string", value: this.parseBareString() },
      };
    }
    throw this.errorPoint("Expected a value");
  }

  /** Parse `@true`, `@false`, `@none`, `@env.NAME`, or `@date` */
  private parseAtValue(): ScalarValue {
    const begin = this.position();
    this.expectChar("@");
    if (this.startsWith("true") && !this.isBareCharAt(4)) {
      this.advance(4);
      return { kind: "boolean", value: true };
    }
    if (this.startsWith("false") && !this.isBareCharAt(5)) {
      this.advance(5);
      return { kind: "boolean", value: false };
    }
    if (this.startsWith("none") && !this.isBareCharAt(4)) {
      this.advance(4);
      return { kind: "none" };
    }
    if (this.startsWith("env.")) {
      this.advance(4);
      const name = this.parseBareString();
      return { kind: "env", name };
    }
    const ch = this.peekChar();
    if (ch !== undefined && ch >= "0" && ch <= "9") {
      return this.parseDate(begin);
    }
    // Consume the bad token for a better span
    const tokenStart = this.pos;
    while (this.pos < this.input.length && isBareChar(this.input[this.pos])) {
      this.pos++;
    }
    const token =
      this.pos > tokenStart ? this.input.substring(tokenStart, this.pos) : "";
    throw this.errorSpan(
      `Illegal constant @${token}; expected @true, @false, @none, @env.NAME, or @date`,
      begin
    );
  }

  private isBareCharAt(offset: number): boolean {
    const absPos = this.pos + offset;
    return absPos < this.input.length && isBareChar(this.input[absPos]);
  }

  private parseDate(begin: Position): ScalarValue {
    const start = this.pos;
    // YYYY-MM-DD
    this.consumeDigits(4, begin);
    this.expectChar("-");
    this.consumeDigits(2, begin);
    this.expectChar("-");
    this.consumeDigits(2, begin);

    // Optional time part: T HH:MM
    if (this.peekChar() === "T") {
      this.advance(1);
      this.consumeDigits(2, begin);
      this.expectChar(":");
      this.consumeDigits(2, begin);

      // Optional :SS
      if (this.peekChar() === ":") {
        this.advance(1);
        this.consumeDigits(2, begin);

        // Optional .fractional
        if (this.peekChar() === ".") {
          this.advance(1);
          const fracStart = this.pos;
          while (
            this.pos < this.input.length &&
            this.input[this.pos] >= "0" &&
            this.input[this.pos] <= "9"
          ) {
            this.pos++;
          }
          if (this.pos === fracStart) {
            throw this.errorSpan(
              "Expected fractional digits in date",
              begin
            );
          }
        }
      }

      // Optional timezone: Z or +/-HH:MM or +/-HHMM
      const tzCh = this.peekChar();
      if (tzCh === "Z") {
        this.advance(1);
      } else if (tzCh === "+" || tzCh === "-") {
        this.advance(1);
        this.consumeDigits(2, begin);
        if (this.peekChar() === ":") {
          this.advance(1);
        }
        this.consumeDigits(2, begin);
      }
    }

    const dateStr = this.input.substring(start, this.pos);
    return { kind: "date", value: dateStr };
  }

  private consumeDigits(count: number, begin: Position): void {
    for (let i = 0; i < count; i++) {
      const ch = this.peekChar();
      if (ch === undefined || ch < "0" || ch > "9") {
        throw this.errorSpan("Expected digit", begin);
      }
      this.advance(1);
    }
  }

  // ── Numbers ─────────────────────────────────────────────────────

  private parseNumberOrString(): TagValue {
    const start = this.pos;
    const begin = this.position();

    const hasMinus = this.peekChar() === "-";
    if (hasMinus) this.advance(1);

    let hasIntDigits = false;
    let hasDot = false;

    // Integer part
    while (
      this.pos < this.input.length &&
      this.input[this.pos] >= "0" &&
      this.input[this.pos] <= "9"
    ) {
      hasIntDigits = true;
      this.advance(1);
    }

    // Decimal point
    if (this.peekChar() === ".") {
      hasDot = true;
      this.advance(1);
      const fracStart = this.pos;
      while (
        this.pos < this.input.length &&
        this.input[this.pos] >= "0" &&
        this.input[this.pos] <= "9"
      ) {
        this.advance(1);
      }
      if (this.pos === fracStart) {
        this.pos = start;
        return this.parseIntegerOrBare(start, hasMinus);
      }
    }

    if (!hasIntDigits && !hasDot) {
      this.pos = start;
      if (hasMinus) {
        throw this.errorPoint("Expected a value");
      }
      return {
        kind: "scalar",
        value: { kind: "string", value: this.parseBareString() },
      };
    }

    // Exponent part
    const expCh = this.peekChar();
    if (expCh === "e" || expCh === "E") {
      this.advance(1);
      const signCh = this.peekChar();
      if (signCh === "+" || signCh === "-") this.advance(1);
      const expStart = this.pos;
      while (
        this.pos < this.input.length &&
        this.input[this.pos] >= "0" &&
        this.input[this.pos] <= "9"
      ) {
        this.advance(1);
      }
      if (this.pos === expStart) {
        throw this.errorSpan("Expected exponent digits", begin);
      }
    }

    // Make sure the number isn't followed by bare-string characters
    const nextCh = this.peekChar();
    if (
      nextCh !== undefined &&
      isBareChar(nextCh) &&
      !(nextCh >= "0" && nextCh <= "9")
    ) {
      this.pos = start;
      if (hasMinus) {
        throw this.errorPoint("Expected a value");
      }
      return {
        kind: "scalar",
        value: { kind: "string", value: this.parseBareString() },
      };
    }

    const fullStr = this.input.substring(start, this.pos);
    const n = parseFloat(fullStr);
    if (isNaN(n)) {
      throw this.errorSpan(`Invalid number: ${fullStr}`, begin);
    }

    return { kind: "scalar", value: { kind: "number", value: n } };
  }

  private parseIntegerOrBare(start: number, hasMinus: boolean): TagValue {
    this.pos = start;
    const begin = this.position();
    if (hasMinus) this.advance(1);

    const digitStart = this.pos;
    while (
      this.pos < this.input.length &&
      this.input[this.pos] >= "0" &&
      this.input[this.pos] <= "9"
    ) {
      this.advance(1);
    }

    if (this.pos === digitStart) {
      this.pos = start;
      if (hasMinus) {
        throw this.errorPoint("Expected a value");
      }
      return {
        kind: "scalar",
        value: { kind: "string", value: this.parseBareString() },
      };
    }

    // Check if followed by bare chars
    if (!hasMinus) {
      const ch = this.peekChar();
      if (
        ch !== undefined &&
        isBareChar(ch) &&
        !(ch >= "0" && ch <= "9")
      ) {
        this.pos = start;
        return {
          kind: "scalar",
          value: { kind: "string", value: this.parseBareString() },
        };
      }
    }

    // Check for exponent
    const expCh = this.peekChar();
    if (expCh === "e" || expCh === "E") {
      this.advance(1);
      const signCh = this.peekChar();
      if (signCh === "+" || signCh === "-") this.advance(1);
      const expStart = this.pos;
      while (
        this.pos < this.input.length &&
        this.input[this.pos] >= "0" &&
        this.input[this.pos] <= "9"
      ) {
        this.advance(1);
      }
      if (this.pos === expStart) {
        throw this.errorSpan("Expected exponent digits", begin);
      }
    }

    const fullStr = this.input.substring(start, this.pos);
    const n = parseFloat(fullStr);
    if (isNaN(n)) {
      throw this.errorSpan(`Invalid number: ${fullStr}`, begin);
    }

    return { kind: "scalar", value: { kind: "number", value: n } };
  }

  // ── Strings ─────────────────────────────────────────────────────

  private parseBareString(): string {
    const start = this.pos;
    while (this.pos < this.input.length && isBareChar(this.input[this.pos])) {
      this.pos++;
    }
    if (this.pos === start) {
      throw this.errorPoint("Expected an identifier");
    }
    return this.input.substring(start, this.pos);
  }

  private parseDoubleQuotedString(): string {
    const begin = this.position();
    this.expectChar('"');
    let result = "";
    for (;;) {
      const ch = this.peekChar();
      if (ch === undefined || ch === "\r" || ch === "\n") {
        throw this.errorSpan("Unterminated string", begin);
      }
      if (ch === '"') {
        this.advance(1);
        return result;
      }
      if (ch === "\\") {
        this.advance(1);
        result += this.parseEscapeChar();
      } else {
        this.advance(1);
        result += ch;
      }
    }
  }

  private parseSingleQuotedString(): string {
    const begin = this.position();
    this.expectChar("'");
    let result = "";
    for (;;) {
      const ch = this.peekChar();
      if (ch === undefined || ch === "\r" || ch === "\n") {
        throw this.errorSpan("Unterminated string", begin);
      }
      if (ch === "'") {
        this.advance(1);
        return result;
      }
      if (ch === "\\") {
        this.advance(1);
        result += "\\";
        const next = this.peekChar();
        if (next === undefined || next === "\r" || next === "\n") {
          throw this.errorSpan("Unterminated string", begin);
        }
        this.advance(1);
        result += next;
      } else {
        this.advance(1);
        result += ch;
      }
    }
  }

  private parseTripleSingleQuotedString(): string {
    const begin = this.position();
    if (!this.startsWith("'''")) {
      throw this.errorPoint("Expected triple-single-quoted string");
    }
    this.advance(3);

    let result = "";
    for (;;) {
      if (this.startsWith("'''")) {
        this.advance(3);
        return result;
      }
      const ch = this.peekChar();
      if (ch === undefined) {
        throw this.errorSpan(
          "Unterminated triple-single-quoted string",
          begin
        );
      }
      if (ch === "\\") {
        this.advance(1);
        result += "\\";
        const next = this.peekChar();
        if (next === undefined) {
          throw this.errorSpan(
            "Unterminated triple-single-quoted string",
            begin
          );
        }
        this.advance(1);
        result += next;
      } else {
        this.advance(1);
        result += ch;
      }
    }
  }

  private parseBacktickString(): string {
    const begin = this.position();
    this.expectChar("`");
    let result = "";
    for (;;) {
      const ch = this.peekChar();
      if (ch === undefined || ch === "\r" || ch === "\n") {
        throw this.errorSpan("Unterminated backtick string", begin);
      }
      if (ch === "`") {
        this.advance(1);
        return result;
      }
      if (ch === "\\") {
        this.advance(1);
        result += this.parseEscapeChar();
      } else {
        this.advance(1);
        result += ch;
      }
    }
  }

  private parseTripleString(): string {
    const begin = this.position();
    if (!this.startsWith('"""')) {
      throw this.errorPoint("Expected triple-quoted string");
    }
    this.advance(3);

    let result = "";
    for (;;) {
      if (this.startsWith('"""')) {
        this.advance(3);
        return result;
      }
      const ch = this.peekChar();
      if (ch === undefined) {
        throw this.errorSpan("Unterminated triple-quoted string", begin);
      }
      if (ch === "\\") {
        this.advance(1);
        result += this.parseEscapeChar();
      } else {
        this.advance(1);
        result += ch;
      }
    }
  }

  private parseEscapeChar(): string {
    const ch = this.peekChar();
    if (ch === undefined) throw this.errorPoint("Unterminated escape sequence");
    switch (ch) {
      case "b":
        this.advance(1);
        return "\b";
      case "f":
        this.advance(1);
        return "\f";
      case "n":
        this.advance(1);
        return "\n";
      case "r":
        this.advance(1);
        return "\r";
      case "t":
        this.advance(1);
        return "\t";
      case "u": {
        const begin = this.position();
        this.advance(1);
        const start = this.pos;
        for (let i = 0; i < 4; i++) {
          const hch = this.peekChar();
          if (hch === undefined || !isHexDigit(hch)) {
            throw this.errorSpan("Expected 4 hex digits in \\uXXXX", begin);
          }
          this.advance(1);
        }
        const hex = this.input.substring(start, this.pos);
        const codePoint = parseInt(hex, 16);
        return String.fromCharCode(codePoint);
      }
      default:
        this.advance(1);
        return ch;
    }
  }

  // ── Heredoc ─────────────────────────────────────────────────────

  private parseHeredoc(): string {
    const begin = this.position();
    this.advance(3); // past <<<

    // Skip spaces/tabs on the same line
    while (this.pos < this.input.length) {
      const ch = this.input[this.pos];
      if (ch === " " || ch === "\t") {
        this.pos++;
      } else {
        break;
      }
    }

    // Allow \r before \n
    if (this.pos < this.input.length && this.input[this.pos] === "\r") {
      this.advance(1);
    }

    // Expect newline
    if (this.pos >= this.input.length || this.input[this.pos] !== "\n") {
      throw this.errorSpan("Expected newline after <<<", begin);
    }
    this.advance(1);

    // Collect lines until we find >>> on its own line
    const lines: string[] = [];
    let foundClose = false;

    while (this.pos < this.input.length) {
      // Read a line (break only on \n)
      const lineStart = this.pos;
      while (this.pos < this.input.length && this.input[this.pos] !== "\n") {
        this.pos++;
      }
      // Strip trailing \r for CRLF compatibility
      let lineContent = this.input.substring(lineStart, this.pos);
      if (lineContent.endsWith("\r")) {
        lineContent = lineContent.substring(0, lineContent.length - 1);
      }

      // Consume the \n
      if (this.pos < this.input.length && this.input[this.pos] === "\n") {
        this.advance(1);
      }

      // Check if this is the closing >>> line
      if (lineContent.trim() === ">>>") {
        foundClose = true;
        break;
      }

      lines.push(lineContent);
    }

    if (!foundClose) {
      throw this.errorSpan("Unterminated heredoc (expected >>>)", begin);
    }

    if (lines.length === 0) {
      return "";
    }

    // Determine strip amount from first line containing a non-space character
    let strip = 0;
    for (const line of lines) {
      const trimmed = line.trimStart();
      if (trimmed.length > 0) {
        strip = line.length - trimmed.length;
        break;
      }
    }

    // Strip indentation and join; whitespace-only lines become empty
    const stripped = lines.map((line) => {
      if (line.trimStart().length === 0) return "";
      if (strip <= line.length) return line.substring(strip);
      return line;
    });

    return stripped.join("\n") + "\n";
  }

  // ── Arrays ──────────────────────────────────────────────────────

  private parseArray(): ArrayElement[] {
    const begin = this.position();
    this.expectChar("[");
    this.skipWs();

    if (this.eatChar("]")) return [];

    const elements: ArrayElement[] = [];
    elements.push(this.parseArrayElement());

    for (;;) {
      this.skipWs();
      if (this.eatChar("]")) return elements;
      if (this.eatChar(",")) {
        this.skipWs();
        if (this.peekChar() === "]") {
          this.advance(1);
          return elements;
        }
        elements.push(this.parseArrayElement());
      } else if (this.pos >= this.input.length) {
        throw this.errorSpan("Unclosed '['", begin);
      } else {
        throw this.errorPoint("Expected ',' or ']' in array");
      }
    }
  }

  private parseArrayElement(): ArrayElement {
    this.skipWs();
    const ch = this.peekChar();

    if (ch === "{") {
      const props = this.parsePropertiesBlock();
      return { value: null, properties: props };
    }

    if (ch === "[") {
      const elements = this.parseArray();
      return { value: { kind: "array", elements }, properties: null };
    }

    const value = this.parseEqValue(false);
    this.skipWs();
    if (this.peekChar() === "{") {
      const props = this.parsePropertiesBlock();
      return { value, properties: props };
    }
    return { value, properties: null };
  }

  // ── Properties Block ────────────────────────────────────────────

  private parsePropertiesBlock(): Statement[] {
    const begin = this.position();
    this.expectChar("{");

    const stmts: Statement[] = [];
    for (;;) {
      this.skipWsAndCommas();
      if (this.eatChar("}")) return stmts;
      if (this.pos >= this.input.length) {
        throw this.errorSpan("Unclosed '{'", begin);
      }
      stmts.push(this.parseStatement());
    }
  }

  // ── References ──────────────────────────────────────────────────

  private parseReference(): ScalarValue {
    this.expectChar("$");

    let ups = 0;
    while (this.peekChar() === "^") {
      this.advance(1);
      ups++;
    }

    const path: RefPathSegment[] = [];
    const firstName = this.parseIdentifier();
    path.push({ kind: "name", name: firstName });

    if (this.peekChar() === "[") {
      this.advance(1);
      this.skipWs();
      const idx = this.parseRefIndex();
      path.push({ kind: "index", index: idx });
      this.skipWs();
      this.expectChar("]");
    }

    while (this.peekChar() === ".") {
      this.advance(1);
      const name = this.parseIdentifier();
      path.push({ kind: "name", name });

      if (this.peekChar() === "[") {
        this.advance(1);
        this.skipWs();
        const idx = this.parseRefIndex();
        path.push({ kind: "index", index: idx });
        this.skipWs();
        this.expectChar("]");
      }
    }

    return { kind: "reference", ups, path };
  }

  private parseRefIndex(): number {
    const begin = this.position();
    const start = this.pos;
    while (
      this.pos < this.input.length &&
      this.input[this.pos] >= "0" &&
      this.input[this.pos] <= "9"
    ) {
      this.pos++;
    }
    if (this.pos === start) {
      throw this.errorPoint("Expected array index");
    }
    const idx = parseInt(this.input.substring(start, this.pos), 10);
    if (isNaN(idx)) {
      throw this.errorSpan("Invalid array index", begin);
    }
    return idx;
  }
}

/** Check if a character is valid in a bare string / identifier. */
function isBareChar(ch: string): boolean {
  const code = ch.charCodeAt(0);
  return (
    (code >= 0x30 && code <= 0x39) || // 0-9
    (code >= 0x41 && code <= 0x5a) || // A-Z
    (code >= 0x61 && code <= 0x7a) || // a-z
    code === 0x5f || // _
    (code >= 0x00c0 && code <= 0x024f) || // Latin Extended
    (code >= 0x1e00 && code <= 0x1eff) // Latin Extended Additional
  );
}

function isHexDigit(ch: string): boolean {
  const code = ch.charCodeAt(0);
  return (
    (code >= 0x30 && code <= 0x39) || // 0-9
    (code >= 0x41 && code <= 0x46) || // A-F
    (code >= 0x61 && code <= 0x66) // a-f
  );
}

/** Parse a MOTLY input string into a list of statements. */
export function parse(input: string): Statement[] {
  const parser = new Parser(input);
  return parser.parseStatements();
}
