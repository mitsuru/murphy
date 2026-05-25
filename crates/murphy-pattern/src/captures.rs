//! Capture container for the C-backend runtime matcher.
//!
//! The B-backend (`node_pattern!` proc macro) returns captures as a typed
//! tuple resolved at compile time; the C-backend cannot, because the IR is
//! consumed at runtime by mruby user cops (see design §4). [`Captures`] is
//! the runtime-typed equivalent: slot-indexed, one entry per `$` capture in
//! source order. Slot numbering matches `PatternIr::captures` exactly.

use murphy_ast::NodeId;

/// One captured value. The variant mirrors the pattern's [`CaptureKind`]:
/// `$_` / `$name` / `$(...)` bind a single node; `$...` / `$pat+` / `$pat*`
/// bind a slice of sibling nodes; `$pat?` binds an optional single node.
///
/// [`CaptureKind`]: crate::CaptureKind
#[derive(Debug, Clone, PartialEq)]
pub enum CaptureValue {
    /// A single captured node. Slot kind is [`CaptureKind::Node`].
    ///
    /// [`CaptureKind::Node`]: crate::CaptureKind::Node
    Node(NodeId),
    /// A captured slice of consecutive sibling nodes (the `$...` span, or
    /// the iterations of a `$pat+` / `$pat*` quantifier). Slot kind is
    /// [`CaptureKind::Seq`].
    ///
    /// [`CaptureKind::Seq`]: crate::CaptureKind::Seq
    Seq(Vec<NodeId>),
    /// An optional single captured node (the `$pat?` quantifier —
    /// murphy-ycx). `Some(id)` when the `?` matched one node; `None` when
    /// it matched zero. Slot kind is [`CaptureKind::OptNode`].
    ///
    /// Distinct from an unwritten slot: `OptNode(None)` is a
    /// *successfully written* slot whose value is the absence of a match,
    /// so [`CaptureBuf::finish`] keeps the `Captures` record and does NOT
    /// degrade the match to "incomplete capture".
    ///
    /// [`CaptureKind::OptNode`]: crate::CaptureKind::OptNode
    OptNode(Option<NodeId>),
}

/// The result of a successful match: one [`CaptureValue`] per `$` capture in
/// the pattern, indexed by slot. Empty for pattern matches with no captures.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Captures {
    values: Vec<CaptureValue>,
}

impl Captures {
    /// Empty captures (no `$` in the pattern).
    pub fn empty() -> Captures {
        Captures { values: Vec::new() }
    }

    /// Number of capture slots.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// `true` iff there are no captures.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Access the capture at `slot`.
    pub fn get(&self, slot: usize) -> Option<&CaptureValue> {
        self.values.get(slot)
    }

    /// Slot-ordered slice of all captures.
    pub fn as_slice(&self) -> &[CaptureValue] {
        &self.values
    }
}

/// In-progress capture buffer used by the matcher. Slot-sized at start; each
/// slot is `None` until a successful pattern arm writes it. On a top-level
/// match success the matcher unwraps every slot into a [`Captures`].
#[derive(Debug, Clone)]
pub(crate) struct CaptureBuf {
    slots: Vec<Option<CaptureValue>>,
}

impl CaptureBuf {
    /// Allocate `n` empty slots. The matcher overwrites a slot when its
    /// pattern arm succeeds, and clones the buffer before exploring an
    /// alternative whose failure must not pollute already-written slots.
    pub(crate) fn new(n: usize) -> CaptureBuf {
        CaptureBuf {
            slots: vec![None; n],
        }
    }

    /// Write `value` into `slot`. The parser numbers each `$` in source
    /// order, and the matcher visits patterns in match order, so on the
    /// successful arm every slot is written exactly once.
    pub(crate) fn set(&mut self, slot: u16, value: CaptureValue) {
        self.slots[slot as usize] = Some(value);
    }

    /// Finish: unwrap every slot into the public [`Captures`]. Returns
    /// `None` if any slot is unwritten — defense in depth against an
    /// IR shape the parser's `validate_capture_position` should have
    /// rejected (e.g. a capture inside `{}` / `!` / `` ` ``). The matcher
    /// surfaces that `None` as a failed match rather than a panic.
    pub(crate) fn finish(self) -> Option<Captures> {
        let mut values = Vec::with_capacity(self.slots.len());
        for slot in self.slots {
            values.push(slot?);
        }
        Some(Captures { values })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_captures_is_empty() {
        let c = Captures::empty();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
        assert!(c.as_slice().is_empty());
        assert!(c.get(0).is_none());
    }

    #[test]
    fn capture_buf_round_trips_values_to_captures() {
        let mut buf = CaptureBuf::new(2);
        buf.set(0, CaptureValue::Node(NodeId(7)));
        buf.set(1, CaptureValue::Seq(vec![NodeId(1), NodeId(2)]));
        let c = buf.finish().expect("all slots written");
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(0), Some(&CaptureValue::Node(NodeId(7))));
        assert_eq!(
            c.get(1),
            Some(&CaptureValue::Seq(vec![NodeId(1), NodeId(2)]))
        );
    }

    #[test]
    fn capture_buf_finish_returns_none_on_unwritten_slot() {
        // The parser's `validate_capture_position` already prevents the
        // patterns that would leave a hole (`{$a $b}`, `!$_`, ` `$_`).
        // `finish` returning `None` is a defense-in-depth net so a
        // hand-built PatternAst that bypasses validation degrades to a
        // failed match instead of a panic.
        assert!(CaptureBuf::new(1).finish().is_none());
        let mut buf = CaptureBuf::new(2);
        buf.set(0, CaptureValue::Node(NodeId(0)));
        assert!(buf.finish().is_none());
    }

    // --- murphy-ycx: OptNode is a written value, NOT an empty slot ---------

    #[test]
    fn capture_buf_round_trips_optnode_some() {
        // `$pat?` with one match -> `OptNode(Some(id))`. Round-trips
        // through CaptureBuf::set/finish identically to `Node` / `Seq`.
        let mut buf = CaptureBuf::new(1);
        buf.set(0, CaptureValue::OptNode(Some(NodeId(7))));
        let c = buf.finish().expect("OptNode(Some) is a written value");
        assert_eq!(c.len(), 1);
        assert_eq!(c.get(0), Some(&CaptureValue::OptNode(Some(NodeId(7)))));
    }

    #[test]
    fn capture_buf_round_trips_optnode_none() {
        // `$pat?` with zero matches -> `OptNode(None)`. The slot is still
        // *written* — `finish` returns `Some(_)`. This is the contract
        // that lets `?` produce a `Captures` for every successful match,
        // even when the optional pattern didn't fire.
        let mut buf = CaptureBuf::new(1);
        buf.set(0, CaptureValue::OptNode(None));
        let c = buf
            .finish()
            .expect("OptNode(None) is a written value, not an empty slot");
        assert_eq!(c.len(), 1);
        assert_eq!(c.get(0), Some(&CaptureValue::OptNode(None)));
    }
}
