use crate::tree::*;
#[allow(unused_imports)]
use crate::tree::MOTLYLocation;
use std::collections::BTreeMap;

/// Deserialize a JSON string into a MOTLYNode.
/// This is the inverse of `json::to_json`.
pub fn from_json(input: &str) -> Result<MOTLYNode, String> {
    let mut p = JsonParser::new(input);
    let value = p.parse_node()?;
    p.skip_ws();
    if p.pos < p.input.len() {
        return Err(format!("Trailing content at position {}", p.pos));
    }
    Ok(value)
}

/// Deserialize a wire-format JSON string into a MOTLYNode.
///
/// Wire format is the internal JSON dialect used between the Rust WASM
/// module and the TypeScript wrapper. The only difference from standard
/// JSON is that `{"$date": "..."}` in scalar positions is recognized as
/// `Scalar::Date` rather than being treated as an unknown object.
/// See `json::to_wire` for the serialization counterpart.
pub fn from_wire(input: &str) -> Result<MOTLYNode, String> {
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

    fn parse_usize(&mut self) -> Result<usize, String> {
        let n = self.parse_number()?;
        if n < 0.0 || n.fract() != 0.0 {
            return Err(format!("Expected non-negative integer, got {}", n));
        }
        Ok(n as usize)
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

    /// Parse a JSON object that represents a MOTLYNode.
    /// Nodes have optional "deleted", "eq", "properties", and "location" keys.
    fn parse_node(&mut self) -> Result<MOTLYNode, String> {
        self.expect(b'{')?;

        let mut eq: Option<EqValue> = None;
        let mut properties: Option<BTreeMap<String, MOTLYPropertyValue>> = None;
        let mut deleted = false;
        let mut location: Option<MOTLYLocation> = None;

        if self.peek() != Some(b'}') {
            loop {
                let key = self.parse_string()?;
                self.expect(b':')?;

                match key.as_str() {
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
                    "location" if self.wire => {
                        location = Some(self.parse_location()?);
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

        Ok(MOTLYNode {
            eq,
            properties,
            deleted,
            location,
        })
    }

    /// Parse a location object: {"parseId":N,"begin":{...},"end":{...}}
    fn parse_location(&mut self) -> Result<MOTLYLocation, String> {
        self.expect(b'{')?;
        let mut parse_id: u32 = 0;
        let mut begin = crate::error::Position { line: 0, column: 0, offset: 0 };
        let mut end = crate::error::Position { line: 0, column: 0, offset: 0 };

        if self.peek() != Some(b'}') {
            loop {
                let key = self.parse_string()?;
                self.expect(b':')?;
                match key.as_str() {
                    "parseId" => parse_id = self.parse_usize()? as u32,
                    "begin" => begin = self.parse_position_obj()?,
                    "end" => end = self.parse_position_obj()?,
                    _ => { self.skip_json_value()?; }
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
        Ok(MOTLYLocation { parse_id, begin, end })
    }

    /// Parse a position object: {"line":N,"column":N,"offset":N}
    fn parse_position_obj(&mut self) -> Result<crate::error::Position, String> {
        self.expect(b'{')?;
        let mut line: usize = 0;
        let mut column: usize = 0;
        let mut offset: usize = 0;

        if self.peek() != Some(b'}') {
            loop {
                let key = self.parse_string()?;
                self.expect(b':')?;
                match key.as_str() {
                    "line" => line = self.parse_usize()?,
                    "column" => column = self.parse_usize()?,
                    "offset" => offset = self.parse_usize()?,
                    _ => { self.skip_json_value()?; }
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
        Ok(crate::error::Position { line, column, offset })
    }

    /// Parse a property value: either a node or a link reference.
    /// Peeks at the JSON object to determine which variant.
    fn parse_property_value(&mut self) -> Result<MOTLYPropertyValue, String> {
        self.skip_ws();
        // Must be an object — peek inside to check for "linkTo"/"linkUps"
        let saved_pos = self.pos;
        self.expect(b'{')?;

        // Peek at the first key
        if self.peek() == Some(b'"') {
            let key = self.parse_string()?;

            if key == "linkTo" || key == "linkUps" {
                // This is a Ref — parse both keys in either order
                self.expect(b':')?;
                let mut link_to: Option<Vec<RefSegment>> = None;
                let mut link_ups: Option<usize> = None;

                if key == "linkTo" {
                    link_to = Some(self.parse_ref_segments()?);
                } else {
                    link_ups = Some(self.parse_usize()?);
                }

                // Check for second key
                self.skip_ws();
                if self.pos < self.input.len() && self.input[self.pos] == b',' {
                    self.pos += 1;
                    let key2 = self.parse_string()?;
                    self.expect(b':')?;
                    if key2 == "linkTo" && link_to.is_none() {
                        link_to = Some(self.parse_ref_segments()?);
                    } else if key2 == "linkUps" && link_ups.is_none() {
                        link_ups = Some(self.parse_usize()?);
                    } else {
                        // Unknown or duplicate key — skip its value
                        self.skip_json_value()?;
                    }
                }

                self.expect(b'}')?;
                let link_to = link_to.ok_or_else(|| "Ref object missing \"linkTo\" key".to_string())?;
                return Ok(MOTLYPropertyValue::Ref {
                    link_to,
                    link_ups: link_ups.unwrap_or(0),
                });
            }

            // Not a linkTo/linkUps — restore position and parse as a full node
            self.pos = saved_pos;
            let node = self.parse_node()?;
            return Ok(MOTLYPropertyValue::Node(node));
        }

        // Empty object {} or starts with non-string — restore and parse as node
        self.pos = saved_pos;
        let node = self.parse_node()?;
        Ok(MOTLYPropertyValue::Node(node))
    }

    /// Parse a JSON array of ref segments: strings become Name, numbers become Index.
    fn parse_ref_segments(&mut self) -> Result<Vec<RefSegment>, String> {
        self.expect(b'[')?;
        let mut segments = Vec::new();

        if self.peek() != Some(b']') {
            loop {
                match self.peek() {
                    Some(b'"') => {
                        let name = self.parse_string()?;
                        segments.push(RefSegment::Name(name));
                    }
                    Some(ch) if ch == b'-' || ch.is_ascii_digit() => {
                        let n = self.parse_usize()?;
                        segments.push(RefSegment::Index(n));
                    }
                    _ => return Err(format!("Expected string or number in linkTo array at position {}", self.pos)),
                }

                self.skip_ws();
                if self.pos < self.input.len() && self.input[self.pos] == b',' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }

        self.expect(b']')?;
        Ok(segments)
    }

    /// Parse an eq value: a scalar, an array, or a special object
    /// (`{"$date": "..."}` in wire mode, or `{"env": "..."}`).
    /// References are NOT in eq anymore — they are at the property value level.
    fn parse_eq(&mut self) -> Result<EqValue, String> {
        match self.peek() {
            Some(b'[') => {
                let arr = self.parse_array()?;
                Ok(EqValue::Array(arr))
            }
            Some(b'{') => {
                // Could be: {"$date": "..."} (wire mode) or {"env": "..."}
                let saved_pos = self.pos;
                self.expect(b'{')?;
                let key = self.parse_string()?;
                self.expect(b':')?;

                match key.as_str() {
                    "$date" if self.wire => {
                        let value = self.parse_string()?;
                        self.expect(b'}')?;
                        Ok(EqValue::Scalar(Scalar::Date(value)))
                    }
                    "env" => {
                        let value = self.parse_string()?;
                        self.expect(b'}')?;
                        Ok(EqValue::EnvRef(value))
                    }
                    _ => {
                        // Unknown object in eq position — restore and treat as error
                        self.pos = saved_pos;
                        Err(format!(
                            "Unexpected object in eq position with key \"{}\" at position {}",
                            key, saved_pos
                        ))
                    }
                }
            }
            _ => {
                let scalar = self.parse_scalar()?;
                Ok(EqValue::Scalar(scalar))
            }
        }
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

    /// Parse a JSON array of MOTLYPropertyValue values.
    fn parse_array(&mut self) -> Result<Vec<MOTLYPropertyValue>, String> {
        self.expect(b'[')?;
        let mut arr = Vec::new();

        if self.peek() != Some(b']') {
            loop {
                let pv = self.parse_property_value()?;
                arr.push(pv);

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

    /// Parse a JSON object as a BTreeMap<String, MOTLYPropertyValue>.
    fn parse_properties(&mut self) -> Result<BTreeMap<String, MOTLYPropertyValue>, String> {
        self.expect(b'{')?;
        let mut map = BTreeMap::new();

        if self.peek() != Some(b'}') {
            loop {
                let key = self.parse_string()?;
                self.expect(b':')?;
                let value = self.parse_property_value()?;
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
        let v = MOTLYNode::new();
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_scalar_string() {
        let v = MOTLYNode::with_eq(EqValue::Scalar(Scalar::String("hello".to_string())));
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_scalar_number() {
        let v = MOTLYNode::with_eq(EqValue::Scalar(Scalar::Number(42.0)));
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_scalar_boolean() {
        let v = MOTLYNode::with_eq(EqValue::Scalar(Scalar::Boolean(true)));
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_deleted() {
        let v = MOTLYNode::deleted();
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_with_properties() {
        let mut v = MOTLYNode::new();
        let mut props = BTreeMap::new();
        props.insert(
            "name".to_string(),
            MOTLYPropertyValue::Node(MOTLYNode::with_eq(EqValue::Scalar(Scalar::String("test".to_string())))),
        );
        props.insert(
            "count".to_string(),
            MOTLYPropertyValue::Node(MOTLYNode::with_eq(EqValue::Scalar(Scalar::Number(5.0)))),
        );
        v.properties = Some(props);
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_with_ref() {
        let mut v = MOTLYNode::new();
        let mut props = BTreeMap::new();
        props.insert(
            "link".to_string(),
            MOTLYPropertyValue::Ref {
                link_to: vec![RefSegment::Name("parent".to_string()), RefSegment::Name("name".to_string())],
                link_ups: 1,
            },
        );
        v.properties = Some(props);
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_with_env_ref() {
        let mut v = MOTLYNode::new();
        let mut props = BTreeMap::new();
        props.insert(
            "path".to_string(),
            MOTLYPropertyValue::Node(MOTLYNode::with_eq(EqValue::EnvRef("HOME".to_string()))),
        );
        v.properties = Some(props);
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_array() {
        let arr = vec![
            MOTLYPropertyValue::Node(MOTLYNode::with_eq(EqValue::Scalar(Scalar::String("a".to_string())))),
            MOTLYPropertyValue::Node(MOTLYNode::with_eq(EqValue::Scalar(Scalar::Number(2.0)))),
            MOTLYPropertyValue::Ref {
                link_to: vec![RefSegment::Name("root".to_string())],
                link_ups: 0,
            },
        ];
        let v = MOTLYNode::with_eq(EqValue::Array(arr));
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_pretty() {
        let mut v = MOTLYNode::new();
        let mut props = BTreeMap::new();
        props.insert(
            "x".to_string(),
            MOTLYPropertyValue::Node(MOTLYNode::with_eq(EqValue::Scalar(Scalar::Boolean(false)))),
        );
        v.properties = Some(props);
        let json = v.to_json_pretty();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_escaped_string() {
        let v = MOTLYNode::with_eq(EqValue::Scalar(Scalar::String(
            "line1\nline2\ttab\"quote\\back".to_string(),
        )));
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_negative_number() {
        let v = MOTLYNode::with_eq(EqValue::Scalar(Scalar::Number(-3.14)));
        let json = v.to_json();
        let v2 = from_json(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn parse_from_external_json() {
        // Simulate JSON that a TS consumer might send
        let json = r#"{"eq":"hello","properties":{"sub":{"eq":42}}}"#;
        let v = from_json(json).unwrap();
        assert_eq!(
            v.eq,
            Some(EqValue::Scalar(Scalar::String("hello".to_string())))
        );
        let props = v.properties.unwrap();
        let sub = match props.get("sub").unwrap() {
            MOTLYPropertyValue::Node(n) => n,
            _ => panic!("Expected Node"),
        };
        assert_eq!(sub.eq, Some(EqValue::Scalar(Scalar::Number(42.0))));
    }
}
