# ADR 0001 GATE FIXTURE — DO NOT EDIT WITHOUT RE-BLESSING THE SNAPSHOT.
#
# This fixture gates ADR 0001: the Murphy/NoReceiverPuts offense `range`
# on the `puts` below is a BYTE offset, NOT a char offset. The two differ
# here ON PURPOSE because the line immediately before `puts` is multibyte
# UTF-8 (3 bytes per CJK char, 1 char each). A regression to char-indexing
# would shift that offset and this fixture's snapshot entry would change.
#
# The byte-vs-char distinction is LOAD-BEARING for the frozen Phase-1
# contract. Editing this file (renaming the comment, adding/removing a
# character or a line) silently shifts every byte offset below it, so you
# MUST re-derive crates/murphy-cli/tests/snapshots/sample_project.json from
# the binary's ACTUAL output and re-hand-verify the multibyte byte offset.
# Keep a non-ASCII UTF-8 line immediately before `puts` or the gate dies.
# コメント
puts "日本語"
