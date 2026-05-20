# ADR 0019 - Phase 6 native primitive IDL

- Date: 2026-05-20
- Status: Accepted
- Issue: `murphy-7rg.2`
- Parent: `murphy-7rg` (Phase 6 - v1 standard cop scope + perf-regression CI)
- Gated by: ADR 0018 (Phase 6 v1 standard cop scope)

## Context

Murphy's mruby cop prelude is intentionally thin Ruby glue over native Rust
primitives. Before Phase 6, the message-location primitive crossed the native
boundary as a string shaped like `"start,end"`, and `Node#message_loc` split and
parsed that string back into integers.

That stringly transport was acceptable for the early live-resolution spike, but
it is now part of the user-cop boundary. Phase 6 expands the standard cop suite,
so the primitive IDL needs to be explicit, typed, and resistant to accidental
Ruby parsing drift.

## Decision

The mruby/native read-only primitive IDL is:

| Primitive | Return | Contract |
|---|---|---|
| `Murphy.node_count` | `Integer` | Count of live prism `CallNode` handles; valid handles are `0...node_count`. |
| `Murphy.node_name(handle)` | `String` or `nil` | Method/call name for a valid handle, or `nil` for an invalid handle. |
| `Murphy.node_receiver_nil?(handle)` | `true` or `false` | `true` when the call has no explicit receiver; invalid handles also return `true`. |
| `Murphy.node_msg_start(handle)` | `Integer` | Start byte offset of the call message token, or `-1` when absent/invalid. |
| `Murphy.node_msg_end(handle)` | `Integer` | Exclusive end byte offset of the call message token, or `-1` when absent/invalid. |
| `Murphy.source_slice(start, end)` | `String` or `nil` | Source bytes in `source[start...end]`, or `nil` for negative, inverted, or out-of-range spans. |

All offsets are byte offsets into the original source buffer, not character
indices. The Rust narrowing site for prism locations remains
`Range::from_prism_location(&loc)`, so the ADR 0001 `usize -> u32` byte-offset
contract stays centralized and audited.

## Sentinel Behavior

`node_msg_start` and `node_msg_end` use `-1` as their only missing-value
sentinel. The sentinel covers all absent cases:

- Negative handle.
- Handle greater than or equal to `node_count`.
- A valid node with no `message_loc`.

Ruby glue owns the conversion from typed primitive offsets to the user-facing
object:

```ruby
start_offset = Murphy.node_msg_start(@handle)
end_offset = Murphy.node_msg_end(@handle)
return nil if start_offset < 0 || end_offset < 0

Murphy::Range.new(start_offset, end_offset)
```

This keeps `Node#message_loc` typed as `Murphy::Range` or `nil` without putting
string parsing on the native boundary.

## Rationale

Typed integer primitives make the native IDL match the data model: message
locations are ranges of byte offsets. They avoid allocating and parsing a
temporary string, remove ambiguity around malformed string sentinels, and make
future primitive tests assert integer behavior directly.

`Murphy::Range` remains a Ruby value object because cop authors should work with
ranges, not with pairs of native calls. The native layer supplies typed offsets;
the prelude assembles the public Ruby object.

## Rejected

- Keep `Murphy.node_msg_range(handle) -> "start,end"`: rejected because it makes
  a range look like an arbitrary string transport and requires `split(',')` /
  `to_i` in the prelude.
- Return Ruby `nil` directly from start/end primitives: rejected because the IDL
  is simpler when both primitives always return integers and the prelude owns
  the user-facing `nil` conversion.

## Consequences

`Murphy.node_msg_range` is no longer registered as a native primitive. Existing
in-repo mruby cops continue to call `node.message_loc`; they now receive the
same `Murphy::Range` object through typed offset primitives.
