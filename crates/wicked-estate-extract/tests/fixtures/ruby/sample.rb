require 'set'
require 'base64'

MAX_ENTRIES = 256
DEFAULT_SEPARATOR = ','

module Codec
  class Encoder
    attr_reader :sep

    def initialize(sep = DEFAULT_SEPARATOR)
      @sep = sep
      @seen = Set.new
    end

    def encode(value)
      return nil if @seen.include?(value)
      @seen.add(value)
      transform(value)
    end

    def encode_all(values)
      values.map { |v| encode(v) }.compact.join(@sep)
    end

    def reset
      @seen.clear
    end
    alias clear reset
    alias_method :wipe, :reset

    def size=(n)
      @size = n
    end

    def [](key)
      @seen.include?(key)
    end

    private

    def transform(value)
      Base64.strict_encode64(value.to_s)
    end
  end

  def self.decode(token)
    Base64.strict_decode64(token)
  end

  def self.run_encode(items)
    enc = Encoder.new
    result = enc.encode_all(items)
    puts decode(result.split(DEFAULT_SEPARATOR).first || '')
    result
  end
end
