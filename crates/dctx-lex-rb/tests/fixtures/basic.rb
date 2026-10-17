# File demo is a sample.
require "json"
require_relative "helper"
load "extra.rb"

module Demo
  MAX = 3

  # A boxed value.
  class Box < Shape
    attr_accessor :width
    attr_reader :height
    attr_writer :title

    # Measures the box.
    # @param scale [Integer] factor
    def area(scale = 1)
      helper(scale)
      self.width
    end

    def self.build(name)
      Box.new(name)
    end

    class << self
      def register(key)
        store.add(key)
      end
    end

    alias measure area
    alias_method :size, :height

    private

    def hidden
      secret()
    end

    define_method(:nope) { hidden_meta() }
    const_set(:NOPE, 1)
    class_eval do
      def injected
        meta_call()
      end
    end
  end
end

def top(a, b)
  a.plus(b)
end

require dynamic
