//! `#[repr(C)]` types that cross the plugin ABI boundary (ADR 0038).
//!
//! Every struct here has a frozen layout: the `#[cfg(test)]` `offset_of!`
//! assertions are the freeze guard. New fields append at the end only.

/// The ABI's borrowed-slice primitive: a `#[repr(C)]` pointer+length pair.
///
/// `len == 0` is valid with any `ptr` (including null); accessors check
/// `len` before dereferencing.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RawSlice {
    /// Start pointer. Meaningful only when `len > 0`.
    pub ptr: *const u8,
    /// Byte length.
    pub len: usize,
}

// Safety: a RawSlice is an immutable, non-owning view. The pointee's
// validity and thread-safety are the host's responsibility under the
// ADR 0038 safety contract (the arena is immutable during dispatch).
unsafe impl Sync for RawSlice {}
unsafe impl Send for RawSlice {}

impl RawSlice {
    /// The empty slice.
    pub const EMPTY: RawSlice = RawSlice {
        ptr: std::ptr::null(),
        len: 0,
    };

    /// Borrow a `&'static str`.
    pub const fn from_str(s: &'static str) -> RawSlice {
        RawSlice {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }

    /// Reconstruct the byte slice.
    ///
    /// # Safety
    /// When `len > 0`, `ptr` must point to `len` initialized bytes valid
    /// for `'a`.
    pub unsafe fn as_bytes<'a>(self) -> &'a [u8] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
        }
    }
}

/// `#[repr(C)]` schema entry for one cop option. Re-implements the
/// option-metadata struct (murphy-9cr.2 concept) for the single-surface
/// ABI. The validation gate (murphy-9cr.9) reads `CopOptions::SCHEMA`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OptionSpec {
    /// Option key in `[cops.rules."Name"]`.
    pub name: RawSlice,
    /// Wire type: `"bool"` / `"int"` / `"string"` / `"string_list"`.
    pub ty: RawSlice,
    /// Default value, JSON-encoded. `EMPTY` when the option is required.
    pub default_json: RawSlice,
    /// One-line human description.
    pub description: RawSlice,
    /// Allowed values for an enum `string` (JSON array); `EMPTY` if free.
    pub enum_values_json: RawSlice,
    /// Suggested replacement when this option is deprecated.
    pub replacement: RawSlice,
    /// Why the option exists / its deprecation reason.
    pub reason: RawSlice,
}

// Safety: OptionSpec is an immutable aggregate of non-owning RawSlice
// views; it lives only in &'static schemas. Sharing across threads is
// sound for the same reason RawSlice is Sync. Not Send: never moved
// across threads, so the stronger bound is left off deliberately.
unsafe impl Sync for OptionSpec {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_slice_from_str_round_trips() {
        let s = RawSlice::from_str("send");
        assert_eq!(unsafe { s.as_bytes() }, b"send");
        assert_eq!(unsafe { RawSlice::EMPTY.as_bytes() }, b"");
    }

    #[test]
    fn raw_slice_field_offsets_are_frozen() {
        use std::mem::offset_of;
        assert_eq!(offset_of!(RawSlice, ptr), 0);
        assert_eq!(offset_of!(RawSlice, len), std::mem::size_of::<usize>());
    }

    #[test]
    fn option_spec_is_repr_c_seven_slices() {
        use std::mem::{offset_of, size_of};
        assert_eq!(size_of::<OptionSpec>(), 7 * size_of::<RawSlice>());
        assert_eq!(offset_of!(OptionSpec, name), 0);
        assert_eq!(offset_of!(OptionSpec, ty), size_of::<RawSlice>());
        assert_eq!(
            offset_of!(OptionSpec, default_json),
            2 * size_of::<RawSlice>()
        );
        assert_eq!(
            offset_of!(OptionSpec, description),
            3 * size_of::<RawSlice>()
        );
        assert_eq!(
            offset_of!(OptionSpec, enum_values_json),
            4 * size_of::<RawSlice>()
        );
        assert_eq!(
            offset_of!(OptionSpec, replacement),
            5 * size_of::<RawSlice>()
        );
        assert_eq!(offset_of!(OptionSpec, reason), 6 * size_of::<RawSlice>());
    }
}
