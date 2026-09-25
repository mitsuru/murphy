# frozen_string_literal: true

require_relative "lib/murphy/version"

Gem::Specification.new do |s|
  s.name = "murphy"
  s.version = Murphy::VERSION
  s.summary = "High-speed Ruby linter/formatter (Rust core)"
  s.description = "Murphy is a high-speed Ruby linter/formatter with a Rust core. "                   "This gem ships precompiled native binaries and execs the "                   "matching one for your platform."
  s.authors = ["Mitsuru Hayasaka"]
  s.homepage = "https://github.com/mitsuru/murphy"
  s.license = "MIT"

  s.required_ruby_version = ">= 3.0"

  s.bindir = "exe"
  s.executables = ["murphy"]
  s.require_paths = ["lib"]

  # Release CI builds one platform gem per matrix entry by exporting
  # `MURPHY_GEM_PLATFORM=x86_64-linux` (etc.) before `gem build`: the
  # platform string doubles as the `libexec/murphy-<platform>` suffix.
  # A plain `gem build` (no env) produces the generic `ruby` gem with
  # whatever `libexec/` payload is staged (possibly none — the `exe/murphy`
  # wrapper then reports the missing binary with install guidance).
  s.platform = ENV.fetch("MURPHY_GEM_PLATFORM", Gem::Platform::RUBY)

  s.files = Dir["lib/**/*.rb", "exe/*", "libexec/*", "README.md", "docs/guides/gem-distribution.md"]
end
