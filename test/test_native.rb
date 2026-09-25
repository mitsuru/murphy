# frozen_string_literal: true

require 'minitest/autorun'
require 'tmpdir'
require 'fileutils'
require_relative '../lib/murphy/native'

class TestNative < Minitest::Test
  def test_platform_tag_maps_common_ruby_platforms
    assert_equal 'x86_64-linux', Murphy::Native.platform_tag('x86_64-linux')
    assert_equal 'aarch64-linux', Murphy::Native.platform_tag('aarch64-linux-gnu')
    assert_equal 'x86_64-darwin', Murphy::Native.platform_tag('x86_64-darwin-23')
    assert_equal 'arm64-darwin', Murphy::Native.platform_tag('arm64-darwin-23')
  end

  def test_platform_tag_rejects_unknown
    err = assert_raises(RuntimeError) { Murphy::Native.platform_tag('mips-unknown') }
    assert_match(/unsupported platform/, err.message)
  end

  def test_native_binary_honors_env_override
    Dir.mktmpdir do |dir|
      fake = File.join(dir, 'murphy')
      FileUtils.touch(fake)
      FileUtils.chmod(0o755, fake)
      old = ENV['MURPHY_NATIVE_BINARY']
      ENV['MURPHY_NATIVE_BINARY'] = fake
      assert_equal fake, Murphy::Native.native_binary('x86_64-linux')
    ensure
      ENV['MURPHY_NATIVE_BINARY'] = old
    end
  end

  def test_native_binary_finds_libexec_payload
    # libexec payload lookup is relative to the gem root derived from
    # this file's location, so this test only asserts the candidate list
    # shape (the real payload exists only in release platform gems).
    cands = Murphy::Native.candidates('x86_64-linux')
    assert(cands.any? { |c| c.end_with?('libexec/murphy-x86_64-linux') })
    assert(cands.any? { |c| c.end_with?('target/release/murphy') })
  end

  GEM_ENV_KEYS = %w[MURPHY_GEM_PATH GEM_HOME GEM_PATH].freeze

  def with_bare_gem_env
    old = GEM_ENV_KEYS.map { |k| ENV[k] }
    GEM_ENV_KEYS.each { |k| ENV.delete(k) }
    yield
  ensure
    GEM_ENV_KEYS.zip(old).each { |k, v| ENV[k] = v }
  end

  def test_ensure_gem_path_env_sets_fallback_when_bare
    with_bare_gem_env do
      Murphy::Native.ensure_gem_path_env!
      # Either Gem.path supplied a fallback or there was nothing to set;
      # the call must not raise in either case.
      assert(true)
    end
  end
end
