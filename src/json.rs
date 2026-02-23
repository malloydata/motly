use crate::error::MOTLYError;
use crate::tree::*;

/// JSON formatting style.
#[derive(Clone, Copy)]
pub enum JsonStyle {
    /// Compact: no whitespace between tokens.
    Compact,
    /// Pretty: 2-space indented, one entry per line.
    Pretty,
}

struct JsonWriter {
    buf: String,
    style: JsonStyle,
    depth: usize,
    /// When true, dates are wrapped as `{"$date": "..."}` instead of plain strings.
    wire: bool,
}

impl JsonWriter {
    fn new(style: JsonStyle) -> Self {
        JsonWriter {
            buf: String::new(),
            style,
            depth: 0,
            wire: false,
        }
    }

    fn is_pretty(&self) -> bool {
        matches!(self.style, JsonStyle::Pretty)
    }

    fn newline(&mut self) {
        if self.is_pretty() {
            self.buf.push('\n');
            for _ in 0..self.depth {
                self.buf.push_str("  ");
            }
        }
    }

    fn space(&mut self) {
        if self.is_pretty() {
            self.buf.push(' ');
        }
    }

    fn write_node(&mut self, node: &MOTLYNode) {
        self.buf.push('{');
        self.depth += 1;

        let mut first = true;

        // "deleted": true
        if node.deleted {
            self.entry_sep(&mut first);
            self.write_key("deleted");
            self.buf.push_str("true");
        }

        // "eq": ...
        if let Some(ref eq) = node.eq {
            self.entry_sep(&mut first);
            self.write_key("eq");
            self.write_eq(eq);
        }

        // "properties": { ... }
        if let Some(ref props) = node.properties {
            self.entry_sep(&mut first);
            self.write_key("properties");
            self.write_properties(props);
        }

        self.depth -= 1;
        self.newline();
        self.buf.push('}');
    }

    fn write_property_value(&mut self, pv: &MOTLYPropertyValue) {
        match pv {
            MOTLYPropertyValue::Node(n) => self.write_node(n),
            MOTLYPropertyValue::Ref(link_to) => {
                // Serialize as {"linkTo": "..."}
                self.buf.push('{');
                self.depth += 1;
                let mut first = true;
                self.entry_sep(&mut first);
                self.write_key("linkTo");
                self.write_string_value(link_to);
                self.depth -= 1;
                self.newline();
                self.buf.push('}');
            }
        }
    }

    fn write_eq(&mut self, eq: &EqValue) {
        match eq {
            EqValue::Scalar(scalar) => self.write_scalar(scalar),
            EqValue::Array(arr) => self.write_array(arr),
            EqValue::EnvRef(name) => {
                // Serialize as {"env": "..."}
                self.buf.push('{');
                self.depth += 1;
                let mut first = true;
                self.entry_sep(&mut first);
                self.write_key("env");
                self.write_string_value(name);
                self.depth -= 1;
                self.newline();
                self.buf.push('}');
            }
        }
    }

    fn write_scalar(&mut self, scalar: &Scalar) {
        match scalar {
            Scalar::String(s) => self.write_string_value(s),
            Scalar::Number(n) => self.write_number(*n),
            Scalar::Boolean(b) => self.buf.push_str(if *b { "true" } else { "false" }),
            Scalar::Date(d) => {
                if self.wire {
                    self.buf.push_str("{\"$date\":");
                    self.write_string_value(d);
                    self.buf.push('}');
                } else {
                    self.write_string_value(d);
                }
            }
        }
    }

    fn write_number(&mut self, n: f64) {
        // Format integers without decimal point.
        // Guard: must be finite, integral, and within the range where f64
        // can represent every integer exactly (2^53).
        if n.is_finite() && n.fract() == 0.0 && n.abs() < (1u64 << 53) as f64 {
            write!(&mut self.buf, "{}", n as i64).unwrap();
        } else {
            // Use Rust's default float formatting
            let s = format!("{}", n);
            self.buf.push_str(&s);
        }
    }

    fn write_array(&mut self, arr: &[MOTLYPropertyValue]) {
        self.buf.push('[');
        self.depth += 1;

        for (i, item) in arr.iter().enumerate() {
            if i > 0 {
                self.buf.push(',');
            }
            self.newline();
            self.write_property_value(item);
        }

        self.depth -= 1;
        if !arr.is_empty() {
            self.newline();
        }
        self.buf.push(']');
    }

    fn write_properties(&mut self, props: &std::collections::BTreeMap<String, MOTLYPropertyValue>) {
        self.buf.push('{');
        self.depth += 1;

        let mut first = true;
        for (key, value) in props {
            self.entry_sep(&mut first);
            self.write_key(key);
            self.write_property_value(value);
        }

        self.depth -= 1;
        self.newline();
        self.buf.push('}');
    }

    fn entry_sep(&mut self, first: &mut bool) {
        if *first {
            *first = false;
        } else {
            self.buf.push(',');
        }
        self.newline();
    }

    fn write_key(&mut self, key: &str) {
        self.write_string_value(key);
        self.buf.push(':');
        self.space();
    }

    fn write_string_value(&mut self, s: &str) {
        self.buf.push('"');
        for ch in s.chars() {
            match ch {
                '"' => self.buf.push_str("\\\""),
                '\\' => self.buf.push_str("\\\\"),
                '\n' => self.buf.push_str("\\n"),
                '\r' => self.buf.push_str("\\r"),
                '\t' => self.buf.push_str("\\t"),
                '\u{0008}' => self.buf.push_str("\\b"),
                '\u{000C}' => self.buf.push_str("\\f"),
                c if c < '\u{0020}' => {
                    write!(&mut self.buf, "\\u{:04x}", c as u32).unwrap();
                }
                c => self.buf.push(c),
            }
        }
        self.buf.push('"');
    }
}

use std::fmt::Write;

/// Serialize a MOTLYNode to a compact JSON string (no whitespace).
pub fn to_json(node: &MOTLYNode) -> String {
    let mut w = JsonWriter::new(JsonStyle::Compact);
    w.write_node(node);
    w.buf
}

/// Serialize a MOTLYNode to a pretty-printed JSON string (2-space indent).
pub fn to_json_pretty(node: &MOTLYNode) -> String {
    let mut w = JsonWriter::new(JsonStyle::Pretty);
    w.write_node(node);
    w.buf
}

/// Serialize a MOTLYNode to the internal wire format.
///
/// "Wire format" is the JSON dialect used to transfer data between the
/// Rust WASM module and the TypeScript wrapper layer. It is *not* the
/// public JSON representation — consumers never see it.
///
/// The only difference from standard JSON (`to_json`) is that MOTLY dates
/// are wrapped as `{"$date": "2024-01-15"}` instead of bare strings.
/// This lets the TypeScript layer distinguish dates from strings and
/// construct JS `Date` objects, which plain JSON cannot represent.
pub fn to_wire(node: &MOTLYNode) -> String {
    let mut w = JsonWriter::new(JsonStyle::Compact);
    w.wire = true;
    w.write_node(node);
    w.buf
}

/// Serialize a list of parse errors to a JSON array string.
pub fn errors_to_json(errors: &[MOTLYError]) -> String {
    let mut w = JsonWriter::new(JsonStyle::Compact);
    w.buf.push('[');
    for (i, err) in errors.iter().enumerate() {
        if i > 0 {
            w.buf.push(',');
        }
        w.buf.push('{');
        w.write_key("code");
        w.write_string_value(&err.code);
        w.buf.push(',');
        w.write_key("message");
        w.write_string_value(&err.message);
        w.buf.push(',');
        w.write_key("begin");
        write_position(&mut w, &err.begin);
        w.buf.push(',');
        w.write_key("end");
        write_position(&mut w, &err.end);
        w.buf.push('}');
    }
    w.buf.push(']');
    w.buf
}

fn write_position(w: &mut JsonWriter, pos: &crate::error::Position) {
    write!(
        &mut w.buf,
        "{{\"line\":{},\"column\":{},\"offset\":{}}}",
        pos.line, pos.column, pos.offset
    )
    .unwrap();
}

/// Serialize schema validation errors to a JSON array string.
pub fn schema_errors_to_json(errors: &[crate::validate::SchemaError]) -> String {
    let mut w = JsonWriter::new(JsonStyle::Compact);
    w.buf.push('[');
    for (i, err) in errors.iter().enumerate() {
        if i > 0 {
            w.buf.push(',');
        }
        w.buf.push('{');
        w.write_key("code");
        w.write_string_value(err.code);
        w.buf.push(',');
        w.write_key("message");
        w.write_string_value(&err.message);
        w.buf.push(',');
        w.write_key("path");
        write_string_array(&mut w, &err.path);
        w.buf.push('}');
    }
    w.buf.push(']');
    w.buf
}

/// Serialize reference validation errors to a JSON array string.
pub fn validation_errors_to_json(errors: &[crate::validate::ValidationError]) -> String {
    let mut w = JsonWriter::new(JsonStyle::Compact);
    w.buf.push('[');
    for (i, err) in errors.iter().enumerate() {
        if i > 0 {
            w.buf.push(',');
        }
        w.buf.push('{');
        w.write_key("code");
        w.write_string_value(err.code);
        w.buf.push(',');
        w.write_key("message");
        w.write_string_value(&err.message);
        w.buf.push(',');
        w.write_key("path");
        write_string_array(&mut w, &err.path);
        w.buf.push('}');
    }
    w.buf.push(']');
    w.buf
}

fn write_string_array(w: &mut JsonWriter, arr: &[String]) {
    w.buf.push('[');
    for (i, s) in arr.iter().enumerate() {
        if i > 0 {
            w.buf.push(',');
        }
        w.write_string_value(s);
    }
    w.buf.push(']');
}
