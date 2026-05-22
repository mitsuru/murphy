//! Plugin-facing offense severity and its ABI wire encoding.

/// How serious an offense is, as declared by a cop.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// A non-fatal style / correctness concern.
    Warning = 0,
    /// A serious problem.
    Error = 1,
}

/// Wire byte for "severity not specified" — the host keeps its default.
pub const SEVERITY_UNSET: u8 = 255;
/// Wire byte for "enablement not specified".
pub const TRISTATE_UNSET: u8 = 255;

impl Severity {
    /// Encode an optional severity to its ABI wire byte.
    pub const fn to_wire(value: Option<Severity>) -> u8 {
        match value {
            Some(Severity::Warning) => 0,
            Some(Severity::Error) => 1,
            None => SEVERITY_UNSET,
        }
    }

    /// Decode an ABI wire byte. `SEVERITY_UNSET` and any unknown byte → `None`.
    pub const fn from_wire(byte: u8) -> Option<Severity> {
        match byte {
            0 => Some(Severity::Warning),
            1 => Some(Severity::Error),
            _ => None,
        }
    }
}

/// Encode an optional bool (a cop's default-enabled) to its wire byte.
pub const fn tristate_to_wire(value: Option<bool>) -> u8 {
    match value {
        Some(false) => 0,
        Some(true) => 1,
        None => TRISTATE_UNSET,
    }
}

/// Decode a tristate wire byte. `TRISTATE_UNSET`/unknown → `None`.
pub const fn tristate_from_wire(byte: u8) -> Option<bool> {
    match byte {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_wire_round_trips_each_variant() {
        for value in [None, Some(Severity::Warning), Some(Severity::Error)] {
            assert_eq!(Severity::from_wire(Severity::to_wire(value)), value);
        }
        assert_eq!(Severity::to_wire(None), SEVERITY_UNSET);
        assert_eq!(Severity::from_wire(SEVERITY_UNSET), None);
    }

    #[test]
    fn tristate_wire_round_trips_each_variant() {
        for value in [None, Some(false), Some(true)] {
            assert_eq!(tristate_from_wire(tristate_to_wire(value)), value);
        }
        assert_eq!(tristate_to_wire(None), TRISTATE_UNSET);
    }

    #[test]
    fn unknown_wire_bytes_decode_to_none() {
        assert_eq!(Severity::from_wire(7), None);
        assert_eq!(tristate_from_wire(7), None);
    }
}
