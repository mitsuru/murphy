# Murphy::Cop mruby SDK prelude (Phase 4 Task 2).
#
# This is the THIN Ruby glue ("fast core, scripted glue", design §2/§4) over
# the Task-3 native primitives. The heavy AST work stays native; this file is
# only the visitor surface a cop author writes against:
#
#   class MyCop < Murphy::Cop
#     def on_call_node(node)
#       return unless node.name == :puts && node.receiver_nil?
#       add_offense(node.message_loc, message: "...") do |fix|
#         fix.replace(node.message_loc, "logger.info")
#       end
#     end
#   end
#
# `include_str!`-embedded into `sdk.rs`; loaded into a fresh isolated
# `mrb_state` (Task 2) BEFORE the cop `.rb`. The native `Murphy.node_*`
# primitives (Task 3) and `Murphy.__emit_offense` (Task 4) are already
# registered on the `Murphy` class by the time this runs.
#
# Cops are READ-ONLY: a `fix` only RECORDS suggested text edits (design §4 — no
# AST mutation). Phase 4 (ADR 0013): the recorded fix is marshalled to Rust as
# a binary blob and attached to the Offense as autocorrect:{edits:[...]}.
#
# ## Edit blob wire format (Phase 4 Task 2 — kept in sync with native_emit_offense)
#
# `add_offense` passes fix.edits to the host as a single binary blob String
# built by Murphy::Fix#to_blob. The blob is NUL-safe (arbitrary replacement
# bytes); the host decodes it with the `s` (ptr+len) mrb_get_args format.
#
# Blob = zero or more concatenated edit records:
#   "<start_decimal> <end_decimal> <replen_decimal> " + exactly replen raw bytes
# Fields are non-negative decimal ASCII integers followed by a single space.
# Replacement is exactly replen raw bytes immediately after the trailing space.
# Empty blob (no edits) → zero Edit records → no autocorrect attached.
#
# Example: fix.replace(Range.new(0,4), "hi") encodes as "0 4 2 hi"
# Example: fix.remove(Range.new(5,9)) encodes as "5 9 0 " (0 bytes of replacement)
# Example: two edits "0 4 2 hi" + "5 9 0 " back-to-back in one blob.
#
# MUST stay in sync with the decoder in native_emit_offense (sdk.rs). Both
# files carry this format spec to prevent encoder/decoder drift.

# `Murphy` is defined as a CLASS by the Task-3 native `primitives::register`
# (`mrb_define_class(mrb, "Murphy", Object)`) before this prelude is eval'd.
# We REOPEN it as a class (a `module Murphy` here would raise `TypeError:
# Murphy is not a module`); its `node_*` / `__emit_offense` natives are
# already defined as module functions on it.
class Murphy
  # A byte-offset source span (ADR 0001). `add_offense`/`fix.replace` take one.
  # Produced by `Node#message_loc`. Plain value object; no behavior.
  class Range
    attr_reader :start_offset, :end_offset

    def initialize(start_offset, end_offset)
      @start_offset = start_offset
      @end_offset = end_offset
    end
  end

  # Thin wrapper over an opaque integer handle into the LIVE prism tree
  # (Task-3 ADR 0008). Every accessor RE-RESOLVES through a native primitive;
  # nothing is cached Ruby-side. `name` is coerced to a Symbol so a cop reads
  # `node.name == :puts` exactly like design §4.
  class Node
    def initialize(handle)
      @handle = handle
    end

    # Returns a Symbol (design §4: `node.name == :puts`), or nil if the
    # primitive reports nil (out-of-range — never happens for a handle the
    # SDK itself produced from `node_count`).
    def name
      n = Murphy.node_name(@handle)
      n && n.to_sym
    end

    # True when the call has no explicit receiver (bare `puts` vs `obj.puts`).
    def receiver_nil?
      Murphy.node_receiver_nil?(@handle)
    end

    # The message/selector token span as a Murphy::Range (byte offsets), or
    # nil when the node has no message_loc.
    def message_loc
      r = Murphy.node_msg_range(@handle)
      return nil unless r
      a, b = r.split(",")
      Murphy::Range.new(a.to_i, b.to_i)
    end
  end

  # Fix recorder (Phase 4 Task 2, ADR 0013). A cop's `do |fix|` block calls
  # `replace`/`insert`/`remove`; the edits are collected here and marshalled
  # to the host as a binary blob (see blob format spec at top of this file).
  # The host attaches the decoded Edit records to the Offense as autocorrect.
  # Cop authors write the same API; the marshalling is invisible to them.
  class Fix
    def initialize
      @edits = []
    end

    def replace(range, replacement)
      @edits << [range.start_offset, range.end_offset, replacement.to_s]
    end

    def insert(range, text)
      @edits << [range.start_offset, range.start_offset, text.to_s]
    end

    def remove(range)
      @edits << [range.start_offset, range.end_offset, ""]
    end

    # Encode all edits as a single binary blob (see format spec at top of file).
    # Each edit: "<start> <end> <replen> " + exactly replen raw bytes.
    # Uses String#<< for NUL-safe binary concatenation (no pack available).
    # Returns a binary String (arbitrary bytes).
    def to_blob
      blob = ""
      @edits.each do |(start, stop, rep)|
        rep_bytes = rep.to_s
        blob << start.to_s << " " << stop.to_s << " " << rep_bytes.bytesize.to_s << " " << rep_bytes
      end
      blob
    end
  end

  # The base class every user cop subclasses (design §4).
  #
  # The host calls `__run` after loading the cop `.rb`: it walks every call
  # node handle and dispatches to `on_call_node` (the only visitor hook in
  # Phase 3 — more `on_<type>` hooks are added when a cop needs them, YAGNI).
  class Cop
    # Every subclass registers itself so the host can find and run the
    # author's cop without knowing its class name (the `.rb` names the class,
    # not the host). One `.rb` == one cop in Phase 3; if a file defines
    # several, all run (their offenses merge — same as multiple native cops).
    def self.inherited(subclass)
      (@subclasses ||= []) << subclass
    end

    def self.__registered
      @subclasses ||= []
    end

    # Default visitor: a no-op. A cop overrides the hooks it cares about.
    def on_call_node(node); end

    # Report an offense. `range` is a Murphy::Range (byte offsets). `severity`
    # defaults to :warning; an optional block receives a Murphy::Fix recorder
    # whose edits are marshalled to the host as a binary blob (Phase 4, ADR 0013)
    # and attached to the Offense as autocorrect:{edits:[...]}. See the blob
    # format spec at the top of this file and in native_emit_offense (sdk.rs).
    # Crosses to the host via the `Murphy.__emit_offense` native.
    def add_offense(range, message:, severity: :warning)
      fix = nil
      if block_given?
        fix = Murphy::Fix.new
        yield fix
      end
      Murphy.__emit_offense(
        range.start_offset,
        range.end_offset,
        message.to_s,
        severity.to_s,
        # Phase 4 (ADR 0013): pass the edit blob (empty string when no fix block).
        # The host decodes it into Vec<Edit> and attaches autocorrect if non-empty.
        # Blob format: "<start> <end> <replen> " + replen bytes, per edit, concatenated.
        fix ? fix.to_blob : ""
      )
    end

    # Host entry point. Walks the LIVE call-node handle space `0...node_count`
    # (Task-3 ADR 0008 walk-order index) and dispatches each to the cop.
    # Read-only traversal (design §4).
    def __run
      Murphy.node_count.times do |h|
        on_call_node(Murphy::Node.new(h))
      end
    end
  end
end
