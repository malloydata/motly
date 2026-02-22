/** A scalar or reference value. */
export type ScalarValue =
  | { kind: "string"; value: string }
  | { kind: "number"; value: number }
  | { kind: "boolean"; value: boolean }
  | { kind: "date"; value: string }
  | { kind: "reference"; ups: number; path: RefPathSegment[] }
  | { kind: "none" }
  | { kind: "env"; name: string };

/** A segment in a reference path: either a named property or an array index. */
export type RefPathSegment =
  | { kind: "name"; name: string }
  | { kind: "index"; index: number };

/** A value that can be assigned with `=`. */
export type TagValue =
  | { kind: "scalar"; value: ScalarValue }
  | { kind: "array"; elements: ArrayElement[] };

/** An element in an array literal. */
export interface ArrayElement {
  value: TagValue | null;
  properties: Statement[] | null;
}

/** A parsed statement (the IR between the parser and interpreter). */
export type Statement =
  | {
      kind: "setEq";
      path: string[];
      value: TagValue;
      properties: Statement[] | null;
    }
  | {
      kind: "assignBoth";
      path: string[];
      value: TagValue;
      properties: Statement[] | null;
    }
  | {
      kind: "replaceProperties";
      path: string[];
      properties: Statement[];
    }
  | {
      kind: "updateProperties";
      path: string[];
      properties: Statement[];
    }
  | {
      kind: "define";
      path: string[];
      deleted: boolean;
    }
  | { kind: "clearAll" };
