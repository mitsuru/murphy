(begin
  (module
    (const :Greeter
      nil)
    (class
      (const :Base
        nil)
      nil
      (begin
        (def :initialize
          nil
          (args
            (arg name))
          (ivasgn @name
            (lvar name)))
        (def :greet
          nil
          (args
            (arg prefix)
            (optarg suffix
              (str "!"))
            (restarg "extras")
            (kwarg tone)
            (kwoptarg volume
              (int 1))
            (kwrestarg "opts")
            (blockarg "block"))
          (begin
            (lvasgn message
              (dstr
                (begin
                  (lvar prefix))
                (str " ")
                (begin
                  (ivar @name))
                (begin
                  (lvar suffix))))
            (if
              (lvar block)
              (yield
                (lvar message))
              nil)
            (lvar message)))
        (def :default
          (self)
          (args)
          (send :new
            nil
            (str "world"))))))
  (def :add
    nil
    (args
      (arg a)
      (arg b))
    (send :+
      (lvar a)
      (lvar b)))
  (sclass
    (self)
    (def :helper
      nil
      (args)
      (int 42))))
