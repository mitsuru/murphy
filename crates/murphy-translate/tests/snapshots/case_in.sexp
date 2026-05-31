(case_match
  (send :http_status
    nil)
  (in_pattern
    (const :Integer
      nil)
    nil
    (sym :matched))
  (in_pattern
    (match_var :y)
    (if_guard
      (send :>
        (lvar y)
        (int 0)))
    (sym :positive))
  (sym :other))
