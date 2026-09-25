# frozen_string_literal: true

require_relative 'murphy/version'
require_relative 'murphy/native'

# Top-level Murphy namespace: version plus native binary resolution.
# The lint engine itself is the native Rust binary; this wrapper only
# exposes version and binary lookup so tooling can probe the install
# without spawning a process.
module Murphy
  # Entry point version for `require 'murphy'`.
  def self.version
    VERSION
  end
end
