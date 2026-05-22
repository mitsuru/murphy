# A small, natural Ruby program exercising the constructs murphy-translate
# handles: classes, modules, methods with every parameter flavor, blocks,
# conditionals, loops, exception handling, collections, string interpolation,
# op-assign, and multiple assignment.

module Inventory
  DEFAULT_LIMIT = 100

  class OutOfStockError < StandardError
    def initialize(sku)
      super("out of stock: #{sku}")
      @sku = sku
    end

    attr_reader :sku
  end

  class Warehouse
    attr_accessor :name

    def initialize(name, capacity: DEFAULT_LIMIT)
      @name = name
      @capacity = capacity
      @items = {}
      @log = []
    end

    def add(sku, quantity = 1, *tags, note: nil, **meta)
      quantity = 1 if quantity < 1
      current = @items[sku] || 0
      total = current + quantity
      if total > @capacity
        raise "capacity exceeded for #{@name}"
      end
      @items[sku] = total
      record(:add, sku, quantity, tags, meta)
      note
    end

    def remove(sku, quantity = 1)
      have = @items.fetch(sku, 0)
      unless have >= quantity
        raise OutOfStockError, sku
      end
      remaining = have - quantity
      if remaining.zero?
        @items.delete(sku)
      else
        @items[sku] = remaining
      end
      remaining
    end

    def count(sku)
      @items[sku] || 0
    end

    def total_units
      sum = 0
      @items.each_value do |n|
        sum += n
      end
      sum
    end

    def each_item(&block)
      @items.each(&block)
    end

    def report
      lines = @items.map do |sku, qty|
        "#{sku}: #{qty}"
      end
      lines.sort.join("\n")
    end

    def self.merge(first, second)
      combined = new("#{first.name}+#{second.name}")
      [first, second].each do |source|
        source.each_item do |sku, qty|
          combined.add(sku, qty)
        end
      end
      combined
    end

    private

    def record(action, sku, quantity, tags, meta)
      entry = { action: action, sku: sku, quantity: quantity }
      entry[:tags] = tags unless tags.empty?
      entry[:meta] = meta unless meta.empty?
      @log << entry
    end
  end

  module Reporting
    SEPARATOR = "-" * 20

    def self.summarize(warehouse)
      header = "Warehouse: #{warehouse.name}"
      body = warehouse.report
      [header, SEPARATOR, body].join("\n")
    end
  end
end

def restock(warehouse, orders)
  applied = 0
  failed = 0
  orders.each do |sku, quantity|
    begin
      warehouse.add(sku, quantity)
      applied += 1
    rescue => error
      warn("could not restock #{sku}: #{error.message}")
      failed += 1
    ensure
      warehouse
    end
  end
  status = failed.zero? ? :ok : :partial
  [status, applied, failed]
end

def drain(warehouse, sku)
  removed = 0
  while warehouse.count(sku) > 0
    warehouse.remove(sku, 1)
    removed += 1
    break if removed > 1_000
  end
  removed
end

def classify(quantity)
  case quantity
  when 0
    :empty
  when 1..9
    :low
  when 10..99
    :normal
  else
    :high
  end
end

main = Inventory::Warehouse.new("main", capacity: 500)
backup = Inventory::Warehouse.new("backup")

orders = { "apple" => 12, "pear" => 4, "plum" => 30 }
state, ok, bad = restock(main, orders)
backup.add("apple", 3, "fruit", note: "spare")

puts Inventory::Reporting.summarize(main)
puts "state=#{state} ok=#{ok} bad=#{bad}"
puts "apples classified: #{classify(main.count("apple"))}"

merged = Inventory::Warehouse.merge(main, backup)
puts "merged total: #{merged.total_units}"
