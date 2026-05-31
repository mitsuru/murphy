# Basic case/in (pattern matching) — exercises CaseMatch, InPattern, MatchVar.
# Arm 1: const pattern (no guard).
# Arm 2: match_var with if-guard.
# else: fallthrough body.
case http_status
in Integer
  :matched
in y if y > 0
  :positive
else
  :other
end
