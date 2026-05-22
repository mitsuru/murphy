# Conditionals, loops, logical operators, and ranges.
grade =
  if score >= 90
    :a
  elsif score >= 70
    :b
  else
    :c
  end

unless ready
  prepare
end

case grade
when :a
  celebrate
when :b, :c
  retry_later
else
  give_up
end

i = 0
while i < 10
  i += 1
end

until done
  step
end

ok = first && second
any = left || right
span = 1..10
tail = 5...
