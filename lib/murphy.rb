# frozen_string_literal: true

require_relative "murphy/version"
require_relative "murphy/native"

module Murphy
  # Entry point for `require "murphy"`. The lint engine itself is the
  # native Rust binary (see `Murphy.native_binary`); this file only
  # exposes version + binary resolution so `bundle exec murphy` and
  # tooling can probe the install without spawning a process.
  def self.version
    VERSION
  end
end
