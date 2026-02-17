use crate::ast::*;
use crate::error::{MOTLYError, Position};

/// Parser state: tracks position in the input string.
struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

/// Parse a MOTLY input string into a list of statements.
pub fn parse(input: &str) -> Result<Vec<Statement>, MOTLYError> {
    let mut parser = Parser { input, pos: 0 };
    let mut statements = Vec::new();

    parser.skip_ws_and_commas();
    while parser.pos < parser.input.len() {
        let stmt = parser.parse_statement()?;
        statements.push(stmt);
        parser.skip_ws_and_commas();
    }

    Ok(statements)
}

impl<'a> Parser<'a> {
    // ── Helpers ──────────────────────────────────────────────────────

    fn remaining(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn peek_char(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn advance(&mut self, n: usize) {
        self.pos += n;
    }

    fn starts_with(&self, s: &str) -> bool {
        self.remaining().starts_with(s)
    }

    fn eat_char(&mut self, ch: char) -> bool {
        if self.peek_char() == Some(ch) {
            self.advance(ch.len_utf8());
            true
        } else {
            false
        }
    }

    fn expect_char(&mut self, ch: char) -> Result<(), MOTLYError> {
        if self.eat_char(ch) {
            Ok(())
        } else {
            Err(self.error_point(format!("Expected '{}'", ch)))
        }
    }

    /// Current position in the source.
    fn position(&self) -> Position {
        let consumed = &self.input[..self.pos];
        let line = consumed.matches('\n').count();
        let last_newline = consumed.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let column = self.pos - last_newline;
        Position {
            line,
            column,
            offset: self.pos,
        }
    }

    /// Create an error at a single point (current position).
    fn error_point(&self, message: String) -> MOTLYError {
        let pos = self.position();
        MOTLYError::syntax_error(message, pos, pos)
    }

    /// Create an error spanning from `begin` to the current position.
    fn error_span(&self, message: String, begin: Position) -> MOTLYError {
        MOTLYError::syntax_error(message, begin, self.position())
    }

    // ── Whitespace & Comments ───────────────────────────────────────

    fn skip_ws(&mut self) {
        loop {
            // Skip whitespace characters
            while let Some(ch) = self.peek_char() {
                if ch == ' ' || ch == '\t' || ch == '\r' || ch == '\n' {
                    self.advance(ch.len_utf8());
                } else {
                    break;
                }
            }
            // Skip line comments: # to end of line
            if self.peek_char() == Some('#') {
                while let Some(ch) = self.peek_char() {
                    if ch == '\r' || ch == '\n' {
                        break;
                    }
                    self.advance(ch.len_utf8());
                }
            } else {
                break;
            }
        }
    }

    /// Like `skip_ws`, but also eats commas. Used in statement-list
    /// contexts (top-level document and properties blocks) so commas
    /// can serve as optional separators between statements.
    fn skip_ws_and_commas(&mut self) {
        self.skip_ws();
        while self.peek_char() == Some(',') {
            self.advance(1);
            self.skip_ws();
        }
    }

    // ── Statement Dispatch ──────────────────────────────────────────

    fn parse_statement(&mut self) -> Result<Statement, MOTLYError> {
        // -... (clearAll)
        if self.starts_with("-...") {
            self.advance(4);
            return Ok(Statement::ClearAll);
        }

        // -name (define deleted)
        if self.peek_char() == Some('-') {
            self.advance(1);
            let path = self.parse_prop_name()?;
            return Ok(Statement::Define {
                path,
                deleted: true,
            });
        }

        // Parse the property path
        let path = self.parse_prop_name()?;
        self.skip_ws();

        match self.peek_char() {
            Some('=') => {
                self.advance(1);
                self.skip_ws();

                // Check for `= ... {` (replaceProperties with preserveValue)
                if self.starts_with("...") {
                    let saved = self.pos;
                    self.advance(3);
                    self.skip_ws();
                    if self.peek_char() == Some('{') {
                        let props = self.parse_properties_block()?;
                        return Ok(Statement::ReplaceProperties {
                            path,
                            properties: props,
                            preserve_value: true,
                        });
                    }
                    self.pos = saved;
                }

                // Check for `= {` (replaceProperties without preserveValue)
                if self.peek_char() == Some('{') {
                    let props = self.parse_properties_block()?;
                    return Ok(Statement::ReplaceProperties {
                        path,
                        properties: props,
                        preserve_value: false,
                    });
                }

                // `= value` (setEq)
                let value = self.parse_eq_value()?;
                self.skip_ws();

                // Optionally followed by `{ ... }` or `{ statements }`
                if self.peek_char() == Some('{') {
                    let saved = self.pos;
                    self.advance(1);
                    self.skip_ws();

                    if self.starts_with("...") {
                        let saved2 = self.pos;
                        self.advance(3);
                        self.skip_ws();
                        if self.peek_char() == Some('}') {
                            self.advance(1);
                            return Ok(Statement::SetEq {
                                path,
                                value,
                                properties: None,
                                preserve_properties: true,
                            });
                        }
                        self.pos = saved2;
                    }

                    self.pos = saved;
                    let props = self.parse_properties_block()?;
                    return Ok(Statement::SetEq {
                        path,
                        value,
                        properties: Some(props),
                        preserve_properties: false,
                    });
                }

                Ok(Statement::SetEq {
                    path,
                    value,
                    properties: None,
                    preserve_properties: false,
                })
            }
            Some(':') => {
                self.advance(1);
                self.skip_ws();
                let props = self.parse_properties_block()?;
                Ok(Statement::ReplaceProperties {
                    path,
                    properties: props,
                    preserve_value: false,
                })
            }
            Some('{') => {
                let props = self.parse_properties_block()?;
                Ok(Statement::UpdateProperties {
                    path,
                    properties: props,
                })
            }
            _ => Ok(Statement::Define {
                path,
                deleted: false,
            }),
        }
    }

    // ── Property Name (dotted path) ─────────────────────────────────

    fn parse_prop_name(&mut self) -> Result<Vec<String>, MOTLYError> {
        let first = self.parse_identifier()?;
        let mut path = vec![first];
        while self.peek_char() == Some('.') {
            self.advance(1);
            let next = self.parse_identifier()?;
            path.push(next);
        }
        Ok(path)
    }

    fn parse_identifier(&mut self) -> Result<String, MOTLYError> {
        if self.peek_char() == Some('`') {
            self.parse_backtick_string()
        } else {
            self.parse_bare_string()
        }
    }

    // ── Values ──────────────────────────────────────────────────────

    fn parse_eq_value(&mut self) -> Result<TagValue, MOTLYError> {
        match self.peek_char() {
            Some('[') => self.parse_array().map(TagValue::Array),
            Some('@') => self.parse_at_value().map(TagValue::Scalar),
            Some('$') => self.parse_reference().map(TagValue::Scalar),
            Some('"') => {
                if self.starts_with("\"\"\"") {
                    self.parse_triple_string()
                        .map(|s| TagValue::Scalar(ScalarValue::String(s)))
                } else {
                    self.parse_double_quoted_string()
                        .map(|s| TagValue::Scalar(ScalarValue::String(s)))
                }
            }
            Some('\'') => {
                if self.starts_with("'''") {
                    self.parse_triple_single_quoted_string()
                        .map(|s| TagValue::Scalar(ScalarValue::String(s)))
                } else {
                    self.parse_single_quoted_string()
                        .map(|s| TagValue::Scalar(ScalarValue::String(s)))
                }
            }
            Some(ch) if ch == '-' || ch.is_ascii_digit() || ch == '.' => {
                self.parse_number_or_string()
            }
            Some(ch) if is_bare_char(ch) => self
                .parse_bare_string()
                .map(|s| TagValue::Scalar(ScalarValue::String(s))),
            _ => Err(self.error_point("Expected a value".to_string())),
        }
    }

    fn parse_scalar_value(&mut self) -> Result<TagValue, MOTLYError> {
        match self.peek_char() {
            Some('@') => self.parse_at_value().map(TagValue::Scalar),
            Some('$') => self.parse_reference().map(TagValue::Scalar),
            Some('"') => {
                if self.starts_with("\"\"\"") {
                    self.parse_triple_string()
                        .map(|s| TagValue::Scalar(ScalarValue::String(s)))
                } else {
                    self.parse_double_quoted_string()
                        .map(|s| TagValue::Scalar(ScalarValue::String(s)))
                }
            }
            Some('\'') => {
                if self.starts_with("'''") {
                    self.parse_triple_single_quoted_string()
                        .map(|s| TagValue::Scalar(ScalarValue::String(s)))
                } else {
                    self.parse_single_quoted_string()
                        .map(|s| TagValue::Scalar(ScalarValue::String(s)))
                }
            }
            Some(ch) if ch.is_ascii_digit() || ch == '.' || ch == '-' => {
                self.parse_number_or_string()
            }
            Some(ch) if is_bare_char(ch) => self
                .parse_bare_string()
                .map(|s| TagValue::Scalar(ScalarValue::String(s))),
            _ => Err(self.error_point("Expected a value".to_string())),
        }
    }

    /// Parse `@true`, `@false`, or `@date`
    fn parse_at_value(&mut self) -> Result<ScalarValue, MOTLYError> {
        let begin = self.position();
        self.expect_char('@')?;
        if self.starts_with("true") && !self.is_bare_char_at(4) {
            self.advance(4);
            return Ok(ScalarValue::Boolean(true));
        }
        if self.starts_with("false") && !self.is_bare_char_at(5) {
            self.advance(5);
            return Ok(ScalarValue::Boolean(false));
        }
        // Must start with a digit to be a date
        match self.peek_char() {
            Some(ch) if ch.is_ascii_digit() => self.parse_date(begin),
            _ => {
                // Consume the bad token for a better span
                let token_start = self.pos;
                while let Some(ch) = self.peek_char() {
                    if is_bare_char(ch) {
                        self.advance(ch.len_utf8());
                    } else {
                        break;
                    }
                }
                let token = if self.pos > token_start {
                    &self.input[token_start..self.pos]
                } else {
                    ""
                };
                Err(self.error_span(
                    format!(
                        "Illegal constant @{}; expected @true, @false, or @date",
                        token
                    ),
                    begin,
                ))
            }
        }
    }

    fn is_bare_char_at(&self, offset: usize) -> bool {
        self.remaining()
            .chars()
            .nth(offset)
            .map_or(false, is_bare_char)
    }

    fn parse_date(&mut self, begin: Position) -> Result<ScalarValue, MOTLYError> {
        let start = self.pos;
        // YYYY-MM-DD
        self.consume_digits(4, begin)?;
        self.expect_char('-')?;
        self.consume_digits(2, begin)?;
        self.expect_char('-')?;
        self.consume_digits(2, begin)?;

        // Optional time part: T HH:MM
        if self.peek_char() == Some('T') {
            self.advance(1);
            self.consume_digits(2, begin)?;
            self.expect_char(':')?;
            self.consume_digits(2, begin)?;

            // Optional :SS
            if self.peek_char() == Some(':') {
                self.advance(1);
                self.consume_digits(2, begin)?;

                // Optional .fractional
                if self.peek_char() == Some('.') {
                    self.advance(1);
                    let frac_start = self.pos;
                    while let Some(ch) = self.peek_char() {
                        if ch.is_ascii_digit() {
                            self.advance(1);
                        } else {
                            break;
                        }
                    }
                    if self.pos == frac_start {
                        return Err(self
                            .error_span("Expected fractional digits in date".to_string(), begin));
                    }
                }
            }

            // Optional timezone: Z or +/-HH:MM or +/-HHMM
            match self.peek_char() {
                Some('Z') => {
                    self.advance(1);
                }
                Some('+') | Some('-') => {
                    self.advance(1);
                    self.consume_digits(2, begin)?;
                    if self.peek_char() == Some(':') {
                        self.advance(1);
                    }
                    self.consume_digits(2, begin)?;
                }
                _ => {}
            }
        }

        let date_str = &self.input[start..self.pos];
        Ok(ScalarValue::Date(date_str.to_string()))
    }

    fn consume_digits(&mut self, count: usize, begin: Position) -> Result<(), MOTLYError> {
        for _ in 0..count {
            match self.peek_char() {
                Some(ch) if ch.is_ascii_digit() => self.advance(1),
                _ => return Err(self.error_span("Expected digit".to_string(), begin)),
            }
        }
        Ok(())
    }

    // ── Numbers ─────────────────────────────────────────────────────

    fn parse_number_or_string(&mut self) -> Result<TagValue, MOTLYError> {
        let start = self.pos;
        let begin = self.position();

        // Optional minus sign
        let has_minus = self.peek_char() == Some('-');
        if has_minus {
            self.advance(1);
        }

        let digit_start = self.pos;
        let mut has_int_digits = false;
        let mut has_dot = false;

        // Integer part
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                has_int_digits = true;
                self.advance(1);
            } else {
                break;
            }
        }

        // Decimal point
        if self.peek_char() == Some('.') {
            has_dot = true;
            self.advance(1);
            let frac_start = self.pos;
            while let Some(ch) = self.peek_char() {
                if ch.is_ascii_digit() {
                    self.advance(1);
                } else {
                    break;
                }
            }
            if self.pos == frac_start {
                self.pos = start;
                return self.parse_integer_or_bare(start, has_minus);
            }
        }

        if !has_int_digits && !has_dot {
            self.pos = start;
            if has_minus {
                return Err(self.error_point("Expected a value".to_string()));
            }
            return self
                .parse_bare_string()
                .map(|s| TagValue::Scalar(ScalarValue::String(s)));
        }

        // Exponent part
        if let Some('e' | 'E') = self.peek_char() {
            self.advance(1);
            if let Some('+' | '-') = self.peek_char() {
                self.advance(1);
            }
            let exp_start = self.pos;
            while let Some(ch) = self.peek_char() {
                if ch.is_ascii_digit() {
                    self.advance(1);
                } else {
                    break;
                }
            }
            if self.pos == exp_start {
                return Err(self.error_span("Expected exponent digits".to_string(), begin));
            }
        }

        // Make sure the number isn't followed by bare-string characters
        if let Some(ch) = self.peek_char() {
            if is_bare_char(ch) && !ch.is_ascii_digit() {
                self.pos = start;
                if has_minus {
                    return Err(self.error_point("Expected a value".to_string()));
                }
                return self
                    .parse_bare_string()
                    .map(|s| TagValue::Scalar(ScalarValue::String(s)));
            }
        }

        let num_str = &self.input[digit_start..self.pos];
        let full_str = &self.input[start..self.pos];
        let n: f64 = full_str
            .parse()
            .map_err(|_| self.error_span(format!("Invalid number: {}", num_str), begin))?;

        Ok(TagValue::Scalar(ScalarValue::Number(n)))
    }

    fn parse_integer_or_bare(
        &mut self,
        start: usize,
        has_minus: bool,
    ) -> Result<TagValue, MOTLYError> {
        self.pos = start;
        let begin = self.position();
        if has_minus {
            self.advance(1);
        }
        let digit_start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                self.advance(1);
            } else {
                break;
            }
        }
        if self.pos == digit_start {
            self.pos = start;
            if has_minus {
                return Err(self.error_point("Expected a value".to_string()));
            }
            return self
                .parse_bare_string()
                .map(|s| TagValue::Scalar(ScalarValue::String(s)));
        }

        // Check if followed by bare chars (making it a bare string)
        if !has_minus {
            if let Some(ch) = self.peek_char() {
                if is_bare_char(ch) && !ch.is_ascii_digit() {
                    self.pos = start;
                    return self
                        .parse_bare_string()
                        .map(|s| TagValue::Scalar(ScalarValue::String(s)));
                }
            }
        }

        // Check for exponent
        if let Some('e' | 'E') = self.peek_char() {
            self.advance(1);
            if let Some('+' | '-') = self.peek_char() {
                self.advance(1);
            }
            let exp_start = self.pos;
            while let Some(ch) = self.peek_char() {
                if ch.is_ascii_digit() {
                    self.advance(1);
                } else {
                    break;
                }
            }
            if self.pos == exp_start {
                return Err(self.error_span("Expected exponent digits".to_string(), begin));
            }
        }

        let full_str = &self.input[start..self.pos];
        let n: f64 = full_str
            .parse()
            .map_err(|_| self.error_span(format!("Invalid number: {}", full_str), begin))?;

        Ok(TagValue::Scalar(ScalarValue::Number(n)))
    }

    // ── Strings ─────────────────────────────────────────────────────

    fn parse_bare_string(&mut self) -> Result<String, MOTLYError> {
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if is_bare_char(ch) {
                self.advance(ch.len_utf8());
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(self.error_point("Expected an identifier".to_string()));
        }
        Ok(self.input[start..self.pos].to_string())
    }

    fn parse_double_quoted_string(&mut self) -> Result<String, MOTLYError> {
        let begin = self.position();
        self.expect_char('"')?;
        let mut result = String::new();
        loop {
            match self.peek_char() {
                None | Some('\r') | Some('\n') => {
                    return Err(self.error_span("Unterminated string".to_string(), begin));
                }
                Some('"') => {
                    self.advance(1);
                    return Ok(result);
                }
                Some('\\') => {
                    self.advance(1);
                    let esc = self.parse_escape_char()?;
                    result.push_str(&esc);
                }
                Some(ch) => {
                    self.advance(ch.len_utf8());
                    result.push(ch);
                }
            }
        }
    }

    /// Parse a raw single-quoted string.
    /// Backslash is literal in the output but pairs with the next character
    /// for delimiter purposes (so `\'` does not end the string).
    /// A raw string cannot end with an odd number of backslashes.
    fn parse_single_quoted_string(&mut self) -> Result<String, MOTLYError> {
        let begin = self.position();
        self.expect_char('\'')?;
        let mut result = String::new();
        loop {
            match self.peek_char() {
                None | Some('\r') | Some('\n') => {
                    return Err(self.error_span("Unterminated string".to_string(), begin));
                }
                Some('\'') => {
                    self.advance(1);
                    return Ok(result);
                }
                Some('\\') => {
                    self.advance(1); // consume backslash
                    result.push('\\');
                    // Pair with the next character (kept literally)
                    match self.peek_char() {
                        None | Some('\r') | Some('\n') => {
                            return Err(self.error_span("Unterminated string".to_string(), begin));
                        }
                        Some(ch) => {
                            self.advance(ch.len_utf8());
                            result.push(ch);
                        }
                    }
                }
                Some(ch) => {
                    self.advance(ch.len_utf8());
                    result.push(ch);
                }
            }
        }
    }

    /// Parse a raw triple-single-quoted string `'''...'''`.
    /// Same raw semantics as single-quoted but allows newlines and
    /// single/double `'` characters. Only `'''` closes the string.
    fn parse_triple_single_quoted_string(&mut self) -> Result<String, MOTLYError> {
        let begin = self.position();
        if !self.starts_with("'''") {
            return Err(self.error_point("Expected triple-single-quoted string".to_string()));
        }
        self.advance(3);

        let mut result = String::new();
        loop {
            if self.starts_with("'''") {
                self.advance(3);
                return Ok(result);
            }
            match self.peek_char() {
                None => {
                    return Err(self.error_span(
                        "Unterminated triple-single-quoted string".to_string(),
                        begin,
                    ));
                }
                Some('\\') => {
                    self.advance(1);
                    result.push('\\');
                    // Pair with next character
                    match self.peek_char() {
                        None => {
                            return Err(self.error_span(
                                "Unterminated triple-single-quoted string".to_string(),
                                begin,
                            ));
                        }
                        Some(ch) => {
                            self.advance(ch.len_utf8());
                            result.push(ch);
                        }
                    }
                }
                Some(ch) => {
                    self.advance(ch.len_utf8());
                    result.push(ch);
                }
            }
        }
    }

    fn parse_backtick_string(&mut self) -> Result<String, MOTLYError> {
        let begin = self.position();
        self.expect_char('`')?;
        let mut result = String::new();
        loop {
            match self.peek_char() {
                None | Some('\r') | Some('\n') => {
                    return Err(self.error_span("Unterminated backtick string".to_string(), begin));
                }
                Some('`') => {
                    self.advance(1);
                    return Ok(result);
                }
                Some('\\') => {
                    self.advance(1);
                    let esc = self.parse_escape_char()?;
                    result.push_str(&esc);
                }
                Some(ch) => {
                    self.advance(ch.len_utf8());
                    result.push(ch);
                }
            }
        }
    }

    fn parse_triple_string(&mut self) -> Result<String, MOTLYError> {
        let begin = self.position();
        if !self.starts_with("\"\"\"") {
            return Err(self.error_point("Expected triple-quoted string".to_string()));
        }
        self.advance(3);

        let mut result = String::new();
        loop {
            if self.starts_with("\"\"\"") {
                self.advance(3);
                return Ok(result);
            }
            match self.peek_char() {
                None => {
                    return Err(
                        self.error_span("Unterminated triple-quoted string".to_string(), begin)
                    );
                }
                Some('\\') => {
                    self.advance(1);
                    let esc = self.parse_escape_char()?;
                    result.push_str(&esc);
                }
                Some(ch) => {
                    self.advance(ch.len_utf8());
                    result.push(ch);
                }
            }
        }
    }

    fn parse_escape_char(&mut self) -> Result<String, MOTLYError> {
        match self.peek_char() {
            None => Err(self.error_point("Unterminated escape sequence".to_string())),
            Some('b') => {
                self.advance(1);
                Ok("\u{0008}".to_string())
            }
            Some('f') => {
                self.advance(1);
                Ok("\u{000C}".to_string())
            }
            Some('n') => {
                self.advance(1);
                Ok("\n".to_string())
            }
            Some('r') => {
                self.advance(1);
                Ok("\r".to_string())
            }
            Some('t') => {
                self.advance(1);
                Ok("\t".to_string())
            }
            Some('u') => {
                let begin = self.position();
                self.advance(1);
                let start = self.pos;
                for _ in 0..4 {
                    match self.peek_char() {
                        Some(ch) if ch.is_ascii_hexdigit() => self.advance(1),
                        _ => {
                            return Err(self
                                .error_span("Expected 4 hex digits in \\uXXXX".to_string(), begin))
                        }
                    }
                }
                let hex = &self.input[start..self.pos];
                let code_point = u32::from_str_radix(hex, 16).map_err(|_| {
                    self.error_span(format!("Invalid hex in \\u escape: {}", hex), begin)
                })?;
                match char::from_u32(code_point) {
                    Some(ch) => Ok(ch.to_string()),
                    None => {
                        Err(self
                            .error_span(format!("Invalid unicode code point: \\u{}", hex), begin))
                    }
                }
            }
            Some(ch) => {
                // Passthrough: \x -> x
                self.advance(ch.len_utf8());
                Ok(ch.to_string())
            }
        }
    }

    // ── Arrays ──────────────────────────────────────────────────────

    fn parse_array(&mut self) -> Result<Vec<ArrayElement>, MOTLYError> {
        let begin = self.position();
        self.expect_char('[')?;
        self.skip_ws();

        if self.eat_char(']') {
            return Ok(Vec::new());
        }

        let mut elements = Vec::new();
        let first = self.parse_array_element()?;
        elements.push(first);

        loop {
            self.skip_ws();
            if self.eat_char(']') {
                return Ok(elements);
            }
            if self.eat_char(',') {
                self.skip_ws();
                // Allow trailing comma
                if self.peek_char() == Some(']') {
                    self.advance(1);
                    return Ok(elements);
                }
                let el = self.parse_array_element()?;
                elements.push(el);
            } else if self.pos >= self.input.len() {
                return Err(self.error_span("Unclosed '['".to_string(), begin));
            } else {
                return Err(self.error_point("Expected ',' or ']' in array".to_string()));
            }
        }
    }

    fn parse_array_element(&mut self) -> Result<ArrayElement, MOTLYError> {
        self.skip_ws();

        match self.peek_char() {
            Some('{') => {
                let props = self.parse_properties_block()?;
                Ok(ArrayElement {
                    value: None,
                    properties: Some(props),
                })
            }
            Some('[') => {
                let elements = self.parse_array()?;
                Ok(ArrayElement {
                    value: Some(TagValue::Array(elements)),
                    properties: None,
                })
            }
            _ => {
                let value = self.parse_scalar_value()?;
                self.skip_ws();
                if self.peek_char() == Some('{') {
                    let props = self.parse_properties_block()?;
                    Ok(ArrayElement {
                        value: Some(value),
                        properties: Some(props),
                    })
                } else {
                    Ok(ArrayElement {
                        value: Some(value),
                        properties: None,
                    })
                }
            }
        }
    }

    // ── Properties Block ────────────────────────────────────────────

    fn parse_properties_block(&mut self) -> Result<Vec<Statement>, MOTLYError> {
        let begin = self.position();
        self.expect_char('{')?;
        self.skip_ws();

        if self.starts_with("...") {
            let saved = self.pos;
            self.advance(3);
            self.skip_ws();
            if self.peek_char() == Some('}') {
                self.advance(1);
                return Ok(Vec::new());
            }
            self.pos = saved;
        }

        let mut stmts = Vec::new();
        loop {
            self.skip_ws_and_commas();
            if self.eat_char('}') {
                return Ok(stmts);
            }
            if self.pos >= self.input.len() {
                return Err(self.error_span("Unclosed '{'".to_string(), begin));
            }
            let stmt = self.parse_statement()?;
            stmts.push(stmt);
        }
    }

    // ── References ──────────────────────────────────────────────────

    fn parse_reference(&mut self) -> Result<ScalarValue, MOTLYError> {
        self.expect_char('$')?;

        let mut ups = 0;
        while self.peek_char() == Some('^') {
            self.advance(1);
            ups += 1;
        }

        let mut path = Vec::new();
        let first_name = self.parse_identifier()?;
        path.push(RefPathSegment::Name(first_name));

        if self.peek_char() == Some('[') {
            self.advance(1);
            self.skip_ws();
            let idx = self.parse_ref_index()?;
            path.push(RefPathSegment::Index(idx));
            self.skip_ws();
            self.expect_char(']')?;
        }

        while self.peek_char() == Some('.') {
            self.advance(1);
            let name = self.parse_identifier()?;
            path.push(RefPathSegment::Name(name));

            if self.peek_char() == Some('[') {
                self.advance(1);
                self.skip_ws();
                let idx = self.parse_ref_index()?;
                path.push(RefPathSegment::Index(idx));
                self.skip_ws();
                self.expect_char(']')?;
            }
        }

        Ok(ScalarValue::Reference { ups, path })
    }

    fn parse_ref_index(&mut self) -> Result<usize, MOTLYError> {
        let begin = self.position();
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                self.advance(1);
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(self.error_point("Expected array index".to_string()));
        }
        let idx_str = &self.input[start..self.pos];
        idx_str
            .parse::<usize>()
            .map_err(|_| self.error_span("Invalid array index".to_string(), begin))
    }
}

/// Check if a character is valid in a bare string / identifier.
fn is_bare_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
        || ch == '_'
        || ('\u{00C0}'..='\u{024F}').contains(&ch)
        || ('\u{1E00}'..='\u{1EFF}').contains(&ch)
}
