//! `Style/IpAddresses` — flags hardcoded IP addresses in string literals.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/IpAddresses
//! upstream_version_checked: 1.69.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Murphy checks plain `str` nodes for IPv4 and IPv6 literal addresses
//!   using `std::net::{Ipv4Addr,Ipv6Addr}::from_str`. IPv4 parity with
//!   Resolv::IPv4::Regex is high; IPv6 edge-case divergence is possible
//!   (Resolv accepts some forms that `from_str` may not and vice versa).
//!   `AllowedAddresses` config is supported (default: ["::"], compared
//!   case-insensitively). When `AllowedAddresses` is empty in config,
//!   Murphy uses RuboCop's default of ["::"].
//!   Strings inside interpolated strings (`dstr` segments) are visited
//!   by `#[on_node(kind = "str")]` — RuboCop skips them via StringHelp.
//!   This is a known gap; top-level IPs in double-quoted strings are still
//!   flagged correctly.
//!   Heredoc bodies produce a `str` with trailing `\n`; the content check
//!   trims trailing whitespace before testing, so heredoc IPs are flagged.
//!   No autocorrect (same as RuboCop). Disabled by default.
//! ```
//!
//! ## Matched shapes
//!
//! Plain `str` nodes whose decoded content:
//! - is 45 bytes or fewer (IPv4-mapped IPv6 maximum)
//! - starts with a hex digit (`0-9`, `a-f`, `A-F`) or `:` (IPv6 prefix)
//! - is not in `AllowedAddresses` (case-insensitive)
//! - parses as a valid IPv4 or IPv6 address
//!
//! ## No autocorrect
//!
//! There is no mechanical fix — addresses must be moved to configuration.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};
use std::net::{Ipv4Addr, Ipv6Addr};

const MSG: &str = "Do not hardcode IP addresses.";

/// Maximum byte length of an IP address string.
/// IPv4-mapped IPv6 (`::ffff:192.168.1.1`) is the longest at 45 chars.
const IPV6_MAX_SIZE: usize = 45;

#[derive(Default)]
pub struct IpAddresses;

#[derive(CopOptions)]
pub struct IpAddressesOptions {
    #[option(
        name = "AllowedAddresses",
        default = [],
        description = "List of IP addresses that are allowed in code."
    )]
    pub allowed_addresses: Vec<String>,
}

#[cop(
    name = "Style/IpAddresses",
    description = "Don't include literal IP addresses in code.",
    default_severity = "warning",
    default_enabled = false,
    options = IpAddressesOptions,
)]
impl IpAddresses {
    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Str(string_id) = *cx.kind(node) else {
        return;
    };

    let contents = cx.string_str(string_id);

    // Trim trailing newline that heredoc bodies include.
    let contents = contents.trim_end_matches('\n');

    if contents.is_empty() {
        return;
    }

    // Fast reject: too long to be an IP address.
    if contents.len() > IPV6_MAX_SIZE {
        return;
    }

    // Fast reject: first char must be a hex digit or `:`.
    if !starts_with_hex_or_colon(contents) {
        return;
    }

    // Check against allowed addresses (case-insensitive).
    let opts = cx.options_or_default::<IpAddressesOptions>();

    // When AllowedAddresses is empty (default from derive), use RuboCop's
    // default of ["::"].
    let is_allowed = if opts.allowed_addresses.is_empty() {
        contents.eq_ignore_ascii_case("::")
    } else {
        opts.allowed_addresses
            .iter()
            .any(|a| a.eq_ignore_ascii_case(contents))
    };
    if is_allowed {
        return;
    }

    // Try to parse as IPv4 or IPv6.
    if !is_ip_address(contents) {
        return;
    }

    cx.emit_offense(cx.range(node), MSG, None);
}

fn starts_with_hex_or_colon(s: &str) -> bool {
    s.as_bytes()
        .first()
        .is_some_and(|&b| b == b':' || b.is_ascii_hexdigit())
}

fn is_ip_address(s: &str) -> bool {
    s.parse::<Ipv4Addr>().is_ok() || s.parse::<Ipv6Addr>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::{IpAddresses, IpAddressesOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- IPv4 -----

    #[test]
    fn flags_ipv4_address() {
        test::<IpAddresses>().expect_offense(indoc! {"
            ip = '127.59.241.29'
                 ^^^^^^^^^^^^^^^ Do not hardcode IP addresses.
        "});
    }

    #[test]
    fn flags_ipv4_loopback() {
        test::<IpAddresses>().expect_offense(indoc! {"
            ip = '127.0.0.1'
                 ^^^^^^^^^^^ Do not hardcode IP addresses.
        "});
    }

    #[test]
    fn flags_ipv4_zeros() {
        test::<IpAddresses>().expect_offense(indoc! {"
            ip = '0.0.0.0'
                 ^^^^^^^^^ Do not hardcode IP addresses.
        "});
    }

    // ----- IPv6 -----

    #[test]
    fn flags_ipv6_loopback() {
        test::<IpAddresses>().expect_offense(indoc! {"
            ip = '::1'
                 ^^^^^ Do not hardcode IP addresses.
        "});
    }

    #[test]
    fn flags_ipv6_full() {
        test::<IpAddresses>().expect_offense(indoc! {"
            ip = '2001:db8::1'
                 ^^^^^^^^^^^^^ Do not hardcode IP addresses.
        "});
    }

    // ----- Allowed addresses -----

    #[test]
    fn allows_double_colon_by_default() {
        // `::` is the default allowed address (RuboCop default).
        test::<IpAddresses>().expect_no_offenses("ip = '::'\n");
    }

    #[test]
    fn allows_configured_address() {
        test::<IpAddresses>()
            .with_options(&IpAddressesOptions {
                allowed_addresses: vec!["127.0.0.1".to_string()],
            })
            .expect_no_offenses("ip = '127.0.0.1'\n");
    }

    #[test]
    fn allows_configured_address_case_insensitive() {
        test::<IpAddresses>()
            .with_options(&IpAddressesOptions {
                allowed_addresses: vec!["::1".to_string()],
            })
            .expect_no_offenses("ip = '::1'\n");
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_non_ip_string() {
        test::<IpAddresses>().expect_no_offenses("ip = 'not_an_ip'\n");
    }

    #[test]
    fn accepts_empty_string() {
        test::<IpAddresses>().expect_no_offenses("ip = ''\n");
    }

    #[test]
    fn accepts_version_number() {
        // "1.0.0" — not a valid IPv4 (only 3 octets)
        test::<IpAddresses>().expect_no_offenses("v = '1.0.0'\n");
    }

    #[test]
    fn accepts_long_string() {
        // Too long to be an IP
        test::<IpAddresses>()
            .expect_no_offenses("s = '1234567890123456789012345678901234567890123456'\n");
    }

    #[test]
    fn accepts_string_starting_with_non_hex() {
        test::<IpAddresses>().expect_no_offenses("s = 'hello world'\n");
    }
}

murphy_plugin_api::submit_cop!(IpAddresses);
