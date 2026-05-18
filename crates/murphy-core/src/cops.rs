//! Native cop implementations.
//!
//! Each cop is a self-contained module implementing the [`Cop`](crate::Cop)
//! trait. Phase 1 ships exactly one (YAGNI); more are added per the design's
//! standard-cop catalogue.

pub mod no_receiver_puts;
