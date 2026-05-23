//! Built-in cops registered through the same single-surface plugin ABI
//! (`PluginCopV1`, ADR 0038) as externally-loaded `.so` plugin packs.
//!
//! v1 (murphy-9cr.22) ships `Murphy/NoReceiverPuts` only. The 14 Phase-6
//! standard cops (Layout / Lint / Style) that lived on the legacy `Cop`
//! trait do not survive this reboot; their migration to the new surface
//! is staged in follow-up issues (murphy-9cr.23 onward, design §1 table
//! row "組込みビルトイン cop").

pub mod no_receiver_puts;

use murphy_plugin_api::PluginCopV1;

/// The host's built-in cop table. The dispatch host (`crate::dispatch`)
/// consumes any `&[&PluginCopV1]`, so this is just the static start of
/// the cop list — `.so`-loaded cops append to a runtime `Vec` over it.
pub static BUILTINS: &[&PluginCopV1] = &[&no_receiver_puts::COP];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_table_contains_no_receiver_puts() {
        let names: Vec<&[u8]> = BUILTINS
            .iter()
            .map(|c| unsafe { c.name.as_bytes() })
            .collect();
        assert!(
            names.contains(&(b"Murphy/NoReceiverPuts".as_slice())),
            "BUILTINS should advertise Murphy/NoReceiverPuts; got {names:?}"
        );
    }

    #[test]
    fn every_builtin_carries_a_struct_size_check() {
        // Loader (murphy-9cr.4 acceptance criterion) rejects a `PluginCopV1`
        // whose `size` is not `size_of::<PluginCopV1>`. Built-ins must also
        // satisfy that — defense in depth against a forgotten initializer.
        for cop in BUILTINS {
            assert_eq!(cop.size, std::mem::size_of::<PluginCopV1>());
        }
    }
}
