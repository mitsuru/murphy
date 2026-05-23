# Arena-shaped Murphy::Cop mruby SDK prelude (murphy-9cr.24.1).
#
# Loaded into a fresh per-cop `MrubyState` AFTER the arena native
# primitives have been registered on the `Murphy` class. Defines the
# Ruby surface that a user `.rb` cop subclasses (`Murphy::Cop`) plus
# the small helper value types (`Range`, `Node`, `Fix`).
#
# # Edit blob wire format (kept in sync with `arena_emit_offense` in
# proxy.rs)
#
# Zero or more concatenated edit records:
#   "<start_decimal> <end_decimal> <replen_decimal> " + exactly replen bytes
#
# All numeric fields are non-negative decimal ASCII integers followed by
# a single space. Replacement is exactly replen bytes (UTF-8 source text)
# after the trailing space. Empty blob -> no autocorrect.
#
# # def_node_matcher / def_node_search
#
# Stubbed in this slice with `raise NotImplementedError` so the prelude
# loads cleanly without requiring `Murphy.compile_pattern` to exist.
# Real implementation lands in murphy-9cr.24.3 / 24.4.

class Murphy
  class Range
    attr_reader :start_offset, :end_offset

    def initialize(start_offset, end_offset)
      @start_offset = start_offset
      @end_offset = end_offset
    end
  end

  class Node
    attr_reader :id

    def initialize(id)
      @id = id
    end

    # Returns a `Murphy::Range` over this node's source span.
    def range
      pair = Murphy.node_range(@id)
      return nil unless pair
      Murphy::Range.new(pair[0], pair[1])
    end

    # Returns the node's kind as a Symbol (e.g. :send).
    def kind
      Murphy.node_kind(@id)
    end
  end

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

    def to_blob
      blob = ""
      @edits.each do |(start, stop, rep)|
        rep_bytes = rep.to_s
        blob << start.to_s << " " << stop.to_s << " " << rep_bytes.bytesize.to_s << " " << rep_bytes
      end
      blob
    end
  end

  # Base class for every arena-shaped user cop.
  class Cop
    def self.inherited(subclass)
      (@@subclasses ||= []) << subclass
    end

    def self.__registered
      @@subclasses ||= []
    end

    # Method-name-derived hook kinds: every instance method matching
    # `on_<snake_kind>` is reported as a Symbol "<snake_kind>". The host
    # resolves these to NodeKindTag. Regexp-free: this mruby build does
    # NOT include the Regexp gem.
    def self.__hook_kinds
      result = []
      instance_methods(false).each do |m|
        s = m.to_s
        next unless s.length > 3 && s[0, 3] == "on_"
        kind_str = s[3, s.length - 3]
        next if kind_str.length == 0
        result << kind_str.to_sym
      end
      result
    end

    # Pattern matcher stubs (provided by murphy-9cr.24.3 / 24.4).
    def self.def_node_matcher(_name, _pattern)
      raise NotImplementedError, "def_node_matcher is provided by murphy-9cr.24.3"
    end

    def self.def_node_search(_name, _pattern)
      raise NotImplementedError, "def_node_search is provided by murphy-9cr.24.3"
    end

    # Report an offense; an optional block receives a `Murphy::Fix` whose
    # recorded edits are marshalled to the host as a single blob.
    def add_offense(range, message:, severity: :warning)
      fix = nil
      if block_given?
        fix = Murphy::Fix.new
        yield fix
      end
      Murphy.emit_offense(
        range.start_offset,
        range.end_offset,
        message.to_s,
        severity.to_s,
        fix ? fix.to_blob : ""
      )
    end
  end
end
