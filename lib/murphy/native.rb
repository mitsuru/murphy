# frozen_string_literal: true

require_relative 'version'

# Top-level Murphy namespace: gem wrapper around the native Rust binary.
module Murphy
  # Locates the precompiled native `murphy` binary shipped inside the gem
  # (C2 Gem distribution; ADR 0048).
  module Native
    # Platform tags used both as `libexec/` binary suffixes and as
    # RubyGems platform strings (`MURPHY_GEM_PLATFORM` in the gemspec).
    PLATFORMS = [
      'x86_64-linux',
      'aarch64-linux',
      'x86_64-darwin',
      'arm64-darwin'
    ].freeze

    def self.cpu_for(plat)
      return 'arm64' if plat.include?('darwin') && plat.include?('arm')
      return 'aarch64' if plat.include?('aarch64')
      return 'arm64' if plat.include?('arm64')
      return 'x86_64' if plat.include?('x86_64') || plat.include?('x64')

      nil
    end

    def self.os_for(plat)
      return 'darwin' if plat.include?('darwin') || plat.include?('macos')
      return 'linux' if plat.include?('linux')

      nil
    end

    def self.unsupported_message(ruby_platform)
      expected = PLATFORMS.join(', ')
      [
        "murphy: unsupported platform #{ruby_platform.inspect}",
        "(expected one of #{expected}).",
        'See docs/guides/gem-distribution.md for supported platforms.'
      ].join("\n")
    end

    # Normalize `RUBY_PLATFORM` / `Gem::Platform.local` to one of
    # PLATFORMS. Raises on unknown platforms with an actionable message.
    def self.platform_tag(ruby_platform = RUBY_PLATFORM)
      plat = ruby_platform.to_s.downcase
      tag = "#{cpu_for(plat)}-#{os_for(plat)}"
      return tag if PLATFORMS.include?(tag)

      raise unsupported_message(ruby_platform)
    end

    # Absolute path of the gem root that contains this file
    # (`<root>/lib/murphy/native.rb` -> `<root>`).
    def self.gem_root
      File.expand_path('../..', __dir__)
    end

    # Candidate native binaries in priority order:
    # 1. `$MURPHY_NATIVE_BINARY` (test / manual override).
    # 2. `<gem>/libexec/murphy-<platform>` (platform gem payload).
    # 3. `<repo>/target/release/murphy`, `<repo>/target/debug/murphy`
    #    (dev checkout running from source).
    def self.candidates(tag = platform_tag)
      cands = []
      cands << ENV['MURPHY_NATIVE_BINARY'] if ENV['MURPHY_NATIVE_BINARY']
      cands << File.join(gem_root, 'libexec', "murphy-#{tag}")
      cands << File.join(gem_root, 'target', 'release', 'murphy')
      cands << File.join(gem_root, 'target', 'debug', 'murphy')
      cands
    end

    def self.usable_binary(path)
      return false if path.nil? || path.empty?
      return false unless File.file?(path)

      File.executable?(path) || path.end_with?('murphy')
    end

    def self.missing_message(tag, cands)
      searched = cands.map { |c| "  - #{c}" }.join("\n")
      lines = [
        "murphy: native binary not found for platform #{tag}.",
        'Searched:',
        searched,
        "Install the matching platform gem (`gem install murphy --platform #{tag}`)",
        'or build from source (`cargo build --release -p murphy-cli`).'
      ]
      lines.join("\n")
    end

    # First existing executable candidate, or raise with install guidance.
    def self.native_binary(tag = platform_tag)
      cands = candidates(tag)
      found = cands.find { |path| usable_binary(path) }
      return found if found

      raise missing_message(tag, cands)
    end

    def self.gem_paths
      Gem.path
    rescue StandardError
      []
    end

    # Gem search dirs for the native resolver (`MURPHY_GEM_PATH` fallback).
    # Under `bundle exec` Bundler already sets `GEM_HOME`/`GEM_PATH`, so
    # this is only needed for plain `gem install` use outside Bundler.
    def self.ensure_gem_path_env!
      return if ENV['MURPHY_GEM_PATH'] || ENV['GEM_HOME'] || ENV['GEM_PATH']

      paths = gem_paths
      return if paths.empty?

      ENV['MURPHY_GEM_PATH'] = paths.join(File::PATH_SEPARATOR)
    end
  end

  def self.native_binary
    Native.native_binary
  end
end
