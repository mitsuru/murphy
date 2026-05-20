//! Native style cops.
//!
//! This group is reserved for shipped style cops as they are ported.

pub mod and_or;
pub mod frozen_string_literal_comment;
pub mod nil_comparison;
pub mod redundant_return;
pub mod string_literals;
pub mod symbol_array;
pub mod word_array;

pub use and_or::AndOr;
pub use frozen_string_literal_comment::FrozenStringLiteralComment;
pub use nil_comparison::NilComparison;
pub use redundant_return::RedundantReturn;
pub use string_literals::StringLiterals;
pub use symbol_array::SymbolArray;
pub use word_array::WordArray;
