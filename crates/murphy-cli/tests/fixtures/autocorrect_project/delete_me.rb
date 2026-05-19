# Fixture: delete_me — a bare pp call that DeletePpCop removes entirely.
# The selector token "pp" is removed (replaced with empty string) via
# fix.remove. Murphy/NoReceiverPuts does NOT fire on pp (it only targets
# puts/print/p), so only the mruby cop offense appears here.
# Fully fixable: post-fix the offense is gone → exit 0.
pp "bye"
