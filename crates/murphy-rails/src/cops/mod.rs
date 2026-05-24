//! Per-namespace cop modules. As individual cops are migrated from the
//! lib.rs stub macro to real arena dispatch (murphy-au8 child tasks),
//! they move into `cops::<namespace>::<cop_name>` files following the
//! same layout as `murphy-rspec` / `murphy-std`.

pub mod rails;
