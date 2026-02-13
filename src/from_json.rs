use crate::tree::*;
use std::collections::BTreeMap;

/// Deserialize a JSON string into a MOTLYValue.
/// This is the inverse of `json::to_json`.
pub fn from_json(input: &str) -> Result<MOTLYValue, String> {
    let mut p = JsonParser::new(input);
    let value = p.parse_node()?;
    p.skip_ws();
    if p.pos < p.input.len() {
        return Err(format!("Trailing content at position {}", p.pos));
    }
    Ok(value)
}

/// Deserialize a wire-format JSON string into a MOTLYValue.
///
/// Wire format is the internal JSON dialect used between the Rust WASM
/// module and the TypeScript wrapper. The only difference from standard
/// JSON is that `{"$date": "..."}` in scalar positions is recognized as
/// `Scalar::Date` rather than being treated as an unknown object.
/// See `json::to_wire` for the serialization counterpart.
pub fn from_wire(input: &str) -> Result<MOTLYValue, String> {
    let mut p = JsonParser::new(input);
    p.wire = true;
    let value = p.parse_node()?;
    p.skip_ws();
    if p.pos < p.input.len() {
        return Err(format!("Trailing content at position {}", p.pos));
    }
    Ok(value)
}

struct JsonParser<'a> {
    input: &'a [u8],
    pos: usize,
    /// When true, `{"$date": "..."}` in eq position is parsed as `Scalar::Date`.
    wire: bool,
}

impl<'a> JsonParser<'a> {
    fn new(input: &'a str) -> Self {
        JsonParser {
            input: input.as_bytes(),
            pos: 0,
            wire: false,
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.input.len() {
            match self.input[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        if self.pos < self.input.len() {
            Some(self.input[self.pos])
        } else {
            None
        }
    }

    fn expect(&mut self, ch: u8) -> Result<(), String> {
        self.skip_ws();
        if self.pos < self.input.len() && self.input[self.pos] == ch {
            self.pos += 1;
            Ok(())
        } else {
            let found = if self.pos < self.input.len() {
                format!("'{}'", self.input[self.pos] as char)
            } else {
                "EOF".to_string()
            };
            Err(format!(
                "Expected '{}' at position {}, found {}",
                ch as char, self.pos, found
            ))
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut s = String::new();
        while self.pos < self.input.len() {
            let ch = self.input[self.pos];
            if ch == b'"' {
                self.pos += 1;
                return Ok(s);
            }
            if ch == b'\\' {
                self.pos += 1;
                if self.pos >= self.input.len() {
                    return Err("Unexpected end of input in string escape".to_string());
                }
                match self.input[self.pos] {
                    b'"' => s.push('"'),
                    b'\\' => s.push('\\'),
                    b'/' => s.push('/'),
                    b'n' => s.push('\n'),
                    b'r' => s.push('\r'),
                    b't' => s.push('\t'),
                    b'b' => s.push('\u{0008}'),
                    b'f' => s.push('\u{000C}'),
                    b'u' => {
                        self.pos += 1;
                        let cp = self.parse_hex4()?;
                        // Handle surrogate pairs
                        if (0xD800..=0xDBFF).contains(&cp) {
                            // High surrogate — expect \uXXXX low surrogate
                            if self.pos + 1 < self.input.len()
                                && self.input[self.pos] == b'\\'
                                && self.input[self.pos + 1] == b'u'
                            {
                                self.pos += 2;
                                let low = self.parse_hex4()?;
                                if (0xDC00..=0xDFFF).contains(&low) {
                                    let cp = 0x10000
                                        + ((cp as u32 - 0xD800) << 10)
                                        + (low as u32 - 0xDC00);
                                    if let Some(c) = char::from_u32(cp) {
                                        s.push(c);
                                    }
                                } else {
                                    s.push(char::REPLACEMENT_CHARACTER);
                                }
                            } else {
                                s.push(char::REPLACEMENT_CHARACTER);
                            }
                        } else if let Some(c) = char::from_u32(cp as u32) {
                            s.push(c);
                        } else {
                            s.push(char::REPLACEMENT_CHARACTER);
                        }
                        continue; // parse_hex4 already advanced pos
                    }
                    other => {
                        return Err(format!("Unknown escape '\\{}'", other as char));
                    }
                }
                self.pos += 1;
            } else {
                // Regular UTF-8 byte — decode properly
                let start = self.pos;
                // Figure out how many bytes this UTF-8 char is
                let width = utf8_char_width(ch);
                if self.pos + width > self.input.len() {
                    return Err("Invalid UTF-8 in JSON string".to_string());
                }
                let slice = &self.input[start..start + width];
                match std::str::from_utf8(slice) {
                    Ok(cs) => {
                        s.push_str(cs);
                        self.pos += width;
                    }
                    Err(_) => {
                        return Err("Invalid UTF-8 in JSON string".to_string());
                    }
                }
            }
        }
        Err("Unterminated string".to_string())
    }

    fn parse_hex4(&mut self) -> Result<u16, String> {
        if self.pos + 4 > self.input.len() {
            return Err("Unexpected end of input in \\u escape".to_string());
        }
        let hex = &self.input[self.pos..self.pos + 4];
        let hex_str = std::str::from_utf8(hex).map_err(|_| "Invalid hex in \\u escape")?;
        let val = u16::from_str_radix(hex_str, 16)
            .map_err(|_| format!("Invalid hex in \\u escape: {}", hex_str))?;
        self.pos += 4;
        Ok(val)
    }

    fn parse_number(&mut self) -> Result<f64, String> {
        self.skip_ws();
        let start = self.pos;
        // Consume: optional minus, digits, optional .digits, optional e/E[+-]digits
        if self.pos < self.input.len() && self.input[self.pos] == b'-' {
            self.pos += 1;
        }
        self.consume_digits();
        if self.pos < self.input.len() && self.input[self.pos] == b'.' {
            self.pos += 1;
            self.consume_digits();
        }
        if self.pos < self.input.len()
            && (self.input[self.pos] == b'e' || self.input[self.pos] == b'E')
        {
            self.pos += 1;
            if self.pos < self.input.len()
                && (self.input[self.pos] == b'+' || self.input[self.pos] == b'-')
            {
                self.pos += 1;
            }
            self.consume_digits();
        }
        let num_str = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|_| "Invalid number encoding")?;
        num_str
            .parse::<f64>()
            .map_err(|e| format!("Invalid number \"{}\": {}", num_str, e))
    }

    fn consume_digits(&mut self) {
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
    }

    fn parse_literal(&mut self, expected: &[u8]) -> Result<(), String> {
        if self.pos + expected.len() > self.input.len() {
            return Err(format!("Unexpected end of input"));
        }
        if &self.input[self.pos..self.pos + expected.len()] == expected {
            self.pos += expected.len();
            Ok(())
        } else {
            Err(format!("Unexpected token at position {}", self.pos))
        }
    }

    /// Parse a JSON object that represents a MOTLYNode (either a Value or a Ref).
    fn parse_value(&mut self) -> Result<MOTLYNode, String> {
        self.expect(b'{')?;

        let mut eq: Option<EqValue> = None;
        let mut properties: Option<BTreeMap<String, MOTLYNode>> = None;
        let mut deleted = false;
        let mut link_to: Option<String> = None;

        if self.peek() != Some(b'}') {
            loop {
                let key = self.parse_string()?;
                self.expect(b':')?;

                match key.as_str() {
                    "linkTo" => {
                        link_to = Some(self.parse_string()?);
                    }
                    "deleted" => {
                        self.skip_ws();
                        self.parse_literal(b"true")?;
                        deleted = true;
                    }
                    "eq" => {
                        eq = Some(self.parse_eq()?);
                    }
                    "properties" => {
                        properties = Some(self.parse_properties()?);
                    }
                    _ => {
                        // Skip unknown keys
                        self.skip_json_value()?;
                    }
                }

                self.skip_ws();
                if self.pos < self.input.len() && self.input[self.pos] == b',' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }

        self.expect(b'}')?;

        if let Some(target) = link_to {
            Ok(MOTLYNode::Ref(MOTLYRef { link_to: target }))
        } else {
            Ok(MOTLYNode::Value(MOTLYValue {
                eq,
                properties,
                deleted,
            }))
        }
    }

    /// Parse the top-level node (must be a MOTLYValue, not a Ref).
    fn parse_node(&mut self) -> Result<MOTLYValue, String> {
        let node = self.parse_value()?;
        match node {
            MOTLYNode::Value(v) => Ok(v),
            MOTLYNode::Ref(_) => Err("Expected a MOTLYValue at top level, got a Ref".to_string()),
        }
    }

    /// Parse an eq value: a scalar, an array, or (in wire mode) a `{"$date": "..."}`.
    fn parse_eq(&mut self) -> Result<EqValue, String> {
        match self.peek() {
            Some(b'[') => {
                let arr = self.parse_array()?;
                Ok(EqValue::Array(arr))
            }
            Some(b'{') if self.wire => {
                // Wire format: {"$date": "..."} → Scalar::Date
                let scalar = self.parse_date_wrapper()?;
                Ok(EqValue::Scalar(scalar))
            }
            _ => {
                let scalar = self.parse_scalar()?;
                Ok(EqValue::Scalar(scalar))
            }
        }
    }

    /// Parse a `{"$date": "..."}` wrapper into `Scalar::Date`.
    fn parse_date_wrapper(&mut self) -> Result<Scalar, String> {
        self.expect(b'{')?;
        let key = self.parse_string()?;
        if key != "$date" {
            return Err(format!("Expected \"$date\" key, got \"{}\"", key));
        }
        self.expect(b':')?;
        let value = self.parse_string()?;
        self.expect(b'}')?;
        Ok(Scalar::Date(value))
    }

    /// Parse a JSON scalar value into a Scalar.
    fn parse_scalar(&mut self) -> Result<Scalar, String> {
        match self.peek() {
            Some(b'"') => {
                let s = self.parse_string()?;
                Ok(Scalar::String(s))
            }
            Some(b't') => {
                self.parse_literal(b"true")?;
                Ok(Scalar::Boolean(true))
            }
            Some(b'f') => {
                self.parse_literal(b"false")?;
                Ok(Scalar::Boolean(false))
            }
            Some(ch) if ch == b'-' || ch.is_ascii_digit() => {
                let n = self.parse_number()?;
                Ok(Scalar::Number(n))
            }
            Some(ch) => Err(format!(
                "Unexpected character '{}' at position {} when parsing scalar",
                ch as char, self.pos
            )),
            None => Err("Unexpected end of input when parsing scalar".to_string()),
        }
    }

    /// Parse a JSON array of MOTLYNode values.
    fn parse_array(&mut self) -> Result<Vec<MOTLYNode>, String> {
        self.expect(b'[')?;
        let mut arr = Vec::new();

        if self.peek() != Some(b']') {
            loop {
                let node = self.parse_value()?;
                arr.push(node);

                self.skip_ws();
                if self.pos < self.input.len() && self.input[self.pos] == b',' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }

        self.expect(b']')?;
        Ok(arr)
    }

    /// Parse a JSON object as a BTreeMap<String, MOTLYNode>.
    fn parse_properties(&mut self) -> Result<BTreeMap<String, MOTLYNode>, String> {
        self.expect(b'{')?;
        let mut map = BTreeMap::new();

        if self.peek() != Some(b'}') {
            loop {
                let key = self.parse_string()?;
                self.expect(b':')?;
                let value = self.parse_value()?;
                map.insert(key, value);

                self.skip_ws();
                if self.pos < self.input.len() && self.input[self.pos] == b',' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }

        self.expect(b'}')?;
        Ok(map)
    }

    /// Skip over an arbitrary JSON value (for unknown keys).
    fn skip_json_value(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(b'"') => {
                self.parse_string()?;
            }
            Some(b'{') => {
                self.expect(b'{')?;
                if self.peek() != Some(b'}') {
                    loop {
                        self.parse_string()?; // key
                        self.expect(b':')?;
                        self.skip_json_value()?;
                        self.skip_ws();
                        if self.pos < self.input.len() && self.input[self.pos] == b',' {
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                }
                self.expect(b'}')?;
            }
            Some(b'[') => {
                self.expect(b'[')?;
                if self.peek() != Some(b']') {
                    loop {
                        self.skip_json_value()?;
                        self.skip_ws();
                        if self.pos < self.input.len() && self.input[self.pos] == b',' {
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                }
                self.expect(b']')?;
            }
            Some(b't') => self.parse_literal(b"true")?,
            Some(b'f') => self.parse_literal(b"false")?,
            Some(b'n') => self.parse_literal(b"null")?,
            Some(ch) if ch == b'-' || ch.is_ascii_digit() => {
                self.parse_number()?;
            }
            Some(ch) => {
                return Err(format!(
                    "Unexpected character '{}' at position {}",
                    ch as char, self.pos
                ));
            }
            None => {
                return Err("Unexpected end of input".to_string());
            }
        }
        Ok(())
    }
}

fn utf8_char_width(first_byte: u8) -> usize {
    match first_byte {
        0..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1, // invalid leading byte, consume 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_empty() {
        let v = MOTLYValue::new();
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_scalar_string() {
        let v = MOTLYValue::with_eq(EqValue::Scalar(Scalar::String("hello".to_string())));
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_scalar_number() {
        let v = MOTLYValue::with_eq(EqValue::Scalar(Scalar::Number(42.0)));
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_scalar_boolean() {
        let v = MOTLYValue::with_eq(EqValue::Scalar(Scalar::Boolean(true)));
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_deleted() {
        let v = MOTLYValue::deleted();
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_with_properties() {
        let mut v = MOTLYValue::new();
        let mut props = BTreeMap::new();
        props.insert(
            "name".to_string(),
            MOTLYNode::Value(MOTLYValue::with_eq(EqValue::Scalar(Scalar::String(
                "test".to_string(),
            )))),
        );
        props.insert(
            "count".to_string(),
            MOTLYNode::Value(MOTLYValue::with_eq(EqValue::Scalar(Scalar::Number(5.0)))),
        );
        v.properties = Some(props);
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_with_ref() {
        let mut v = MOTLYValue::new();
        let mut props = BTreeMap::new();
        props.insert(
            "link".to_string(),
            MOTLYNode::Ref(MOTLYRef {
                link_to: "$^parent.name".to_string(),
            }),
        );
        v.properties = Some(props);
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_array() {
        let arr = vec![
            MOTLYNode::Value(MOTLYValue::with_eq(EqValue::Scalar(Scalar::String(
                "a".to_string(),
            )))),
            MOTLYNode::Value(MOTLYValue::with_eq(EqValue::Scalar(Scalar::Number(2.0)))),
            MOTLYNode::Ref(MOTLYRef {
                link_to: "$root".to_string(),
            }),
        ];
        let v = MOTLYValue::with_eq(EqValue::Array(arr));
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_pretty() {
        let mut v = MOTLYValue::new();
        let mut props = BTreeMap::new();
        props.insert(
            "x".to_string(),
            MOTLYNode::Value(MOTLYValue::with_eq(EqValue::Scalar(Scalar::Boolean(false)))),
        );
        v.properties = Some(props);
        let json = v.to_json_pretty();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_escaped_string() {
        let v = MOTLYValue::with_eq(EqValue::Scalar(Scalar::String(
            "line1\nline2\ttab\"quote\\back".to_string(),
        )));
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_negative_number() {
        let v = MOTLYValue::with_eq(EqValue::Scalar(Scalar::Number(-3.14)));
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn parse_from_external_json() {
        // Simulate JSON that a TS consumer might send
        let json = r#"{"eq":"hello","properties":{"sub":{"eq":42}}}"#;
        let v = from_json(json).unwrap();
        assert_eq!(v.eq, Some(EqValue::Scalar(Scalar::String("hello".to_string()))));
        let props = v.properties.unwrap();
        let sub = match props.get("sub").unwrap() {
            MOTLYNode::Value(n) => n,
            _ => panic!("expected Value"),
        };
        assert_eq!(sub.eq, Some(EqValue::Scalar(Scalar::Number(42.0))));
    }
}
