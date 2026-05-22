(begin
  (lvasgn grade
    (if
      (send :>=
        (send :score
          nil)
        (int 90))
      (sym :a)
      (if
        (send :>=
          (send :score
            nil)
          (int 70))
        (sym :b)
        (sym :c))))
  (if
    (send :ready
      nil)
    nil
    (send :prepare
      nil))
  (case
    (lvar grade)
    (when
      (sym :a)
      (send :celebrate
        nil))
    (when
      (sym :b)
      (sym :c)
      (send :retry_later
        nil))
    (send :give_up
      nil))
  (lvasgn i
    (int 0))
  (while post=false
    (send :<
      (lvar i)
      (int 10))
    (op-asgn :+
      (lvasgn i
        nil)
      (int 1)))
  (until post=false
    (send :done
      nil)
    (send :step
      nil))
  (lvasgn ok
    (and
      (send :first
        nil)
      (send :second
        nil)))
  (lvasgn any
    (or
      (send :left
        nil)
      (send :right
        nil)))
  (lvasgn span
    (range exclusive=false
      (int 1)
      (int 10)))
  (lvasgn tail
    (range exclusive=true
      (int 5)
      nil)))
