//! Native style cops.
//!
//! This group is reserved for shipped style cops as they are ported.

pub mod frozen_string_literal_comment;
pub mod string_literals;
pub mod symbol_array;
pub mod word_array;

pub use frozen_string_literal_comment::FrozenStringLiteralComment;
pub use string_literals::StringLiterals;
pub use symbol_array::SymbolArray;
pub use word_array::WordArray;
