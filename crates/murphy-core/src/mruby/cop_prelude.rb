# Murphy::Cop mruby SDK prelude (Phase 3 Task 4).
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
# AST mutation). In Phase 3 the recorded fix is captured-stored-only by the
# host and never applied/serialized (Scope Fence 1, soft-(a)).

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

  # Captured-only fix recorder (Scope Fence 1, soft-(a)). A cop's `do |fix|`
  # block calls `replace`/`insert`/`remove`; the edits are collected here and
  # handed to the host, which in Phase 3 STORES them in-memory only and never
  # applies or serializes them. Forward-compatible: cop authors write
  # Phase-4-ready cops today.
  class Fix
    attr_reader :edits

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
    # whose edits are CAPTURED-ONLY (soft-(a) — never applied/serialized in
    # Phase 3). Crosses to the host via the `Murphy.__emit_offense` native;
    # the host builds the Rust `Offense` and the captured-fix is dropped after
    # the run.
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
        # Captured-only: serialized as a compact string purely so the host
        # can prove "fix was recorded" internally. NOT the autocorrect
        # contract (Phase 4 owns that); the host drops it after the run.
        fix ? fix.edits.length : 0
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
