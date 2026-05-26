// Chained postfix quantifiers (`int++`, `int*?`, etc.) are rejected at parse
// time (murphy-ycx). RuboCop's NodePattern has the same rule — there is no
// reluctant/possessive layer to disambiguate the inner quantifier.

murphy_plugin_macros::node_pattern!(m, "(array int++)");

fn main() {}
