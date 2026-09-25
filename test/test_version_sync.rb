# frozen_string_literal: true

require "minitest/autorun"

class TestVersionSync < Minitest::Test
  REPO_ROOT = File.expand_path("..", __dir__)

  def test_gem_version_matches_cargo_version
    require_relative "../lib/murphy/version"
    cargo = File.read(File.join(REPO_ROOT, "crates", "murphy-cli", "Cargo.toml"))
    m = cargo.match(/^version\s*=\s*"([^"]+)"/)
    assert m, "could not find version in crates/murphy-cli/Cargo.toml"
    assert_equal m[1], Murphy::VERSION,
                 "lib/murphy/version.rb (#{Murphy::VERSION}) must match "                  "crates/murphy-cli/Cargo.toml (#{m[1]})"
  end
end
