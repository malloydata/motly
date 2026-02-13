use std::fmt;

/// A 0-based position in the source text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// 0-based line number
    pub line: usize,
    /// 0-based column (character offset within the line)
    pub column: usize,
    /// 0-based absolute byte offset from the start of input
    pub offset: usize,
}

/// A parse error with span information (begin..end).
#[derive(Debug, Clone, PartialEq)]
pub struct MOTLYError {
    pub code: String,
    pub message: String,
    /// Start of the offending region
    pub begin: Position,
    /// End of the offending region (exclusive)
    pub end: Position,
}

impl MOTLYError {
    pub fn syntax_error(message: String, begin: Position, end: Position) -> Self {
        MOTLYError {
            code: "tag-parse-syntax-error".to_string(),
            message,
            begin,
            end,
        }
    }

}

impl fmt::Display for MOTLYError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.begin == self.end {
            write!(
                f,
                "{}:{}: {} ({})",
                self.begin.line, self.begin.column, self.message, self.code
            )
        } else {
            write!(
                f,
                "{}:{}-{}:{}: {} ({})",
                self.begin.line,
                self.begin.column,
                self.end.line,
                self.end.column,
                self.message,
                self.code
            )
        }
    }
}
