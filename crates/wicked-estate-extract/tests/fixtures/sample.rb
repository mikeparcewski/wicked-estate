require 'json'
require 'digest'

MAX_RECORDS = 1000
DEFAULT_ENCODING = 'utf-8'

module Processing
  class DataStore
    def initialize
      @records = []
    end

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
