# Method and class/module definitions with every parameter flavor.
module Greeter
  class Base
    def initialize(name)
      @name = name
    end

    def greet(prefix, suffix = "!", *extras, tone:, volume: 1, **opts, &block)
      message = "#{prefix} #{@name}#{suffix}"
      yield message if block
      message
    end

    def self.default
      new("world")
    end
  end
end

def add(a, b)
  a + b
end

class << self
  def helper
    42
  end
end
