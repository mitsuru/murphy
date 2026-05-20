//! Native layout cops.
//!
//! This group is reserved for shipped layout cops as they are ported.

pub mod empty_lines;
pub mod space_inside_parens;
pub mod trailing_whitespace;

pub use empty_lines::EmptyLines;
pub use space_inside_parens::SpaceInsideParens;
pub use trailing_whitespace::TrailingWhitespace;
