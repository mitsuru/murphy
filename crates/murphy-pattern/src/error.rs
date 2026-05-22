//! Parse errors with a byte-offset span into the pattern source string.

/// A half-open byte range into the pattern source string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatSpan {
    pub start: u32,
    pub end: u32,
}

impl PatSpan {
    pub fn new(start: usize, end: usize) -> PatSpan {
        PatSpan {
            start: start as u32,
            end: end as u32,
        }
    }
}

/// A pattern parse error: a human-readable message plus the offending span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub span: PatSpan,
}

impl ParseError {
    pub fn new(message: impl Into<String>, span: PatSpan) -> ParseError {
        ParseError {
            message: message.into(),
            span,
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (at {}..{})",
            self.message, self.span.start, self.span.end
        )
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_carries_message_and_span() {
        let e = ParseError::new("unknown node type `sned`", PatSpan::new(1, 5));
        assert_eq!(e.span, PatSpan { start: 1, end: 5 });
        assert!(e.to_string().contains("sned"));
        assert!(e.to_string().contains("1..5"));
    }
}
