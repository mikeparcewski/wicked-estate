require 'json'
require 'digest'

MAX_RECORDS = 1000
DEFAULT_ENCODING = 'utf-8'

module Processing
  class DataStore
    attr_reader :balance
    attr_accessor :label, :notes

    def initialize
      @records = []
    end

    def name=(v)
      @name = v
    end

    def [](k)
      @records[k]
    end

    def <=>(other)
      0
    end

    def ==(other)
      false
    end

    def original
      @records.length
    end
    alias new_name original
    alias_method :other_name, :original

    def add(record)
      @records << normalize(record)
    end

    def all
      @records.dup
    end

    def to_json
      JSON.generate(@records)
    end

    private

    def normalize(record)
      record.to_s.strip
    end
  end

  def self.checksum(data)
    Digest::SHA256.hexdigest(data)
  end
end

def run_store
  store = Processing::DataStore.new
  store.add("alpha")
  store.add("beta")
  json = store.to_json
  checksum = Processing.checksum(json)
  puts checksum
end
