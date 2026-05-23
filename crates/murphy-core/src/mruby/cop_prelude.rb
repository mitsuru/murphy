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
# `add_offense` passes fix.edits to the host as a single blob String built by
# Murphy::Fix#to_blob; the host decodes it with the `s` (ptr+len) mrb_get_args
# format. The length prefix makes the *transport* byte-exact so NUL / newline /
# comma inside legitimate multi-byte source text survive intact — it does NOT
# widen the contract to arbitrary binary. `Edit.replacement` is a Rust `String`
# serialised as a JSON string, so a replacement MUST be valid UTF-8 (it is
# replacement *source text*); the host drops any edit whose replacement bytes
# are not valid UTF-8 rather than corrupting them.
#
# Blob = zero or more concatenated edit records:
#   "<start_decimal> <end_decimal> <replen_decimal> " + exactly replen bytes
# Fields are non-negative decimal ASCII integers followed by a single space.
# Replacement is exactly replen bytes (UTF-8 source text) after that space.
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
    attr_reader :id

    def initialize(id)
      @id = id
      @handle = id
    end

    def kind
      Murphy.node_kind(@id)
    end

    def parent
      pid = Murphy.node_parent(@id)
      pid && Murphy::Node.new(pid)
    end

    def children
      Murphy.node_children(@id).map { |node_id| Murphy::Node.new(node_id) }
    end

    def ancestors
      Murphy.node_ancestors(@id).map { |node_id| Murphy::Node.new(node_id) }
    end

    def descendants
      Murphy.node_descendants(@id).map { |node_id| Murphy::Node.new(node_id) }
    end

    def range
      start_offset, end_offset = Murphy.node_range(@id)
      Murphy::Range.new(start_offset, end_offset)
    end

    def field(name)
      wrap_field(Murphy.node_field(@id, name))
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
      start_offset = Murphy.node_msg_start(@handle)
      end_offset = Murphy.node_msg_end(@handle)
      return nil if start_offset < 0 || end_offset < 0

      Murphy::Range.new(start_offset, end_offset)
    end

    private

    def wrap_field(value)
      if value.is_a?(Integer)
        Murphy::Node.new(value)
      elsif value.is_a?(Array)
        value.map { |item| item.is_a?(Integer) ? Murphy::Node.new(item) : item }
      else
        value
      end
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

    # Encode all edits as a single blob (see format spec at top of file).
    # Each edit: "<start> <end> <replen> " + exactly replen bytes of UTF-8
    # replacement source text. String#<< concatenation is byte-exact (no pack
    # available) so the length-prefixed transport stays lossless; the host
    # drops any edit whose replacement is not valid UTF-8.
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

    def self.__hook_kinds
      own_methods = instance_methods - Murphy::Cop.instance_methods
      own_methods.map do |m|
        s = m.to_s
        s[0, 3] == "on_" && s.length > 3 ? s[3, s.length - 3] : nil
      end.compact
    end

    def self.def_node_matcher(name, pattern)
      ir = Murphy.compile_pattern(pattern.to_s)
      define_method(name) do |node|
        Murphy::Node.wrap_match(Murphy.match(ir, node.id))
      end
    end

    def self.def_node_search(name, pattern)
      ir = Murphy.compile_pattern(pattern.to_s)
      define_method(name) do |root|
        return enum_for(name, root) unless block_given?
        Murphy.node_descendants(root.id).each do |node_id|
          captures = Murphy::Node.wrap_match(Murphy.match(ir, node_id))
          yield captures if captures
        end
      end
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

    # Host entry point. Walks the arena AST and dispatches `on_<kind>` hooks.
    # The legacy `on_call_node` loop remains during the bridge migration so
    # existing spike-era cops keep running until the CLI fixtures are ported.
    def __run
      ([Murphy.ast_root] + Murphy.node_descendants(Murphy.ast_root)).each do |node_id|
        node = Murphy::Node.new(node_id)
        kind = node.kind
        next unless kind

        hook = ("on_" + kind.to_s).to_sym
        send(hook, node) if respond_to?(hook)
      end

      Murphy.node_count.times do |h|
        on_call_node(Murphy::Node.new(h))
      end
    end
  end

  class Node
    def self.wrap_match(value)
      if value.is_a?(Integer)
        Murphy::Node.new(value)
      elsif value.is_a?(Array)
        value.map { |item| wrap_match(item) }
      else
        value
      end
    end
  end
end
