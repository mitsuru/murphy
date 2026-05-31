(begin
  (case_match
    (send :request
      nil)
    (in_pattern
      (array_pattern
        (match_var :first)
        (match_rest
          (match_var :rest)))
      nil
      (lvar first))
    nil)
  (case_match
    (send :items
      nil)
    (in_pattern
      (array_pattern_with_tail
        (match_var :a)
        (match_var :b))
      nil
      (lvar a))
    nil)
  (case_match
    (send :coords
      nil)
    (in_pattern
      (array_pattern
        (match_rest))
      nil
      (sym :any))
    nil)
  (case_match
    (send :opts
      nil)
    (in_pattern
      (hash_pattern
        (pair
          (sym :host)
          (match_var :host))
        (match_nil_pattern))
      nil
      (lvar host))
    nil)
  (case_match
    (send :opts
      nil)
    (in_pattern
      (hash_pattern
        (pair
          (sym :host)
          (match_var :host))
        (match_rest
          (match_var :rest)))
      nil
      (lvar rest))
    nil))
