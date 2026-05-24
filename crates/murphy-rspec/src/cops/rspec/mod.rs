//! `RSpec/*` cops.
//!
//! Each cop type is re-exported from this module so `lib.rs` can list
//! them in `register_cops!` with one short `use cops::rspec::*` line.

pub mod describe_class;
pub mod example_length;
pub mod multiple_expectations;

mod helpers;

pub use describe_class::DescribeClass;
pub use example_length::ExampleLength;
pub use multiple_expectations::MultipleExpectations;
