# A broad mix: calls, blocks, collections, interpolation, op-assign,
# multiple assignment, exceptions, jumps.
numbers = [1, 2, 3, 4]
total = 0
numbers.each do |n|
  total += n
end

config = { mode: :fast, retries: 3, **defaults }
label = "result: #{total} (#{numbers.size} items)"

a, b, *rest = numbers
x = y = 0
count ||= 10
flag &&= true

doubled = numbers.map { |n| n * 2 }
safe = obj&.value

begin
  risky_call
rescue StandardError => e
  log(e)
ensure
  cleanup
end

def find(list, target)
  list.each do |item|
    return item if item == target
  end
  nil
end

result = find(numbers, 3)
puts label
words = `echo hello`
pattern = /a#{b}c/i
