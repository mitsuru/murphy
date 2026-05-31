# PM-B: array/hash patterns — array_pattern, array_pattern_with_tail,
# hash_pattern, match_rest, match_nil_pattern.

# array_pattern with named rest
case request
in [first, *rest]
  first
end

# array_pattern_with_tail (trailing comma)
case items
in [a, b,]
  a
end

# bare match_rest
case coords
in [*]
  :any
end

# hash_pattern with match_nil_pattern (**nil)
case opts
in {host:, **nil}
  host
end

# hash_pattern with match_rest (**rest)
case opts
in {host:, **rest}
  rest
end
