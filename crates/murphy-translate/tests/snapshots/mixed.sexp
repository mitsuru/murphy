(begin
  (lvasgn numbers
    (array
      (int 1)
      (int 2)
      (int 3)
      (int 4)))
  (lvasgn total
    (int 0))
  (block
    (send :each
      (lvar numbers))
    (args
      (arg n))
    (op-asgn :+
      (lvasgn total
        nil)
      (lvar n)))
  (lvasgn config
    (hash
      (pair
        (sym :mode)
        (sym :fast))
      (pair
        (sym :retries)
        (int 3))
      (kwsplat
        (send :defaults
          nil))))
  (lvasgn label
    (dstr
      (str "result: ")
      (begin
        (lvar total))
      (str " (")
      (begin
        (send :size
          (lvar numbers)))
      (str " items)")))
  (masgn
    (mlhs
      (lvasgn a
        nil)
      (lvasgn b
        nil)
      (splat
        (lvasgn rest
          nil)))
    (lvar numbers))
  (lvasgn x
    (lvasgn y
      (int 0)))
  (or-asgn
    (lvasgn count
      nil)
    (int 10))
  (and-asgn
    (lvasgn flag
      nil)
    (true))
  (lvasgn doubled
    (block
      (send :map
        (lvar numbers))
      (args
        (arg n))
      (send :*
        (lvar n)
        (int 2))))
  (lvasgn safe
    (csend :value
      (send :obj
        nil)))
  (begin
    (ensure
      (rescue
        (send :risky_call
          nil)
        (resbody
          (exceptions
            (const :StandardError
              nil))
          (lvasgn e
            nil)
          (send :log
            nil
            (lvar e)))
        nil)
      (send :cleanup
        nil)))
  (def :find
    nil
    (args
      (arg list)
      (arg target))
    (begin
      (block
        (send :each
          (lvar list))
        (args
          (arg item))
        (if
          (send :==
            (lvar item)
            (lvar target))
          (return
            (lvar item))
          nil))
      (nil)))
  (lvasgn result
    (send :find
      nil
      (lvar numbers)
      (int 3)))
  (send :puts
    nil
    (lvar label))
  (lvasgn words
    (xstr
      (str "echo hello")))
  (lvasgn pattern
    (regexp opts="i"
      (str "a")
      (begin
        (lvar b))
      (str "c"))))
