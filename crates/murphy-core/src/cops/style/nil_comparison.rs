use crate::cop::{Cop, CopContext};
use crate::cops::support::{offense_with_edit, replace_edit};
use crate::{Offense, Range};

pub struct NilComparison;

impl Cop for NilComparison {
    fn name(&self) -> &str {
        "Style/NilComparison"
    }

    fn on_call_node(
        &self,
        node: &ruby_prism::CallNode<'_>,
        ctx: &CopContext<'_>,
        sink: &mut Vec<Offense>,
    ) {
        let name = node.name();
        let operator = name.as_slice();
        if operator != b"==" && operator != b"!=" {
            return;
        }

        let Some(receiver) = node.receiver() else {
            return;
        };
        let Some(argument) = only_argument(node) else {
            return;
        };
        if !matches!(argument, ruby_prism::Node::NilNode { .. }) {
            return;
        }

        let receiver_range = Range::from_prism_location(&receiver.location());
        let comparison_range = Range::from_prism_location(&node.location());
        let Some(receiver_source) = source_slice(ctx.source, receiver_range) else {
            return;
        };
        let Some(comparison_source) = source_slice(ctx.source, comparison_range) else {
            return;
        };
        if receiver_source.contains(&b'#') || comparison_source.contains(&b'#') {
            return;
        }
        let Ok(receiver_source) = std::str::from_utf8(receiver_source) else {
            return;
        };

        let replacement = if operator == b"==" {
            format!("{receiver_source}.nil?")
        } else {
            format!("!{receiver_source}.nil?")
        };

        sink.push(offense_with_edit(
            ctx.file,
            self.name(),
            comparison_range,
            "Prefer `nil?` to comparing with `nil`.",
            replace_edit(
                comparison_range.start_offset,
                comparison_range.end_offset,
                &replacement,
            ),
        ));
    }
}

fn only_argument<'pr>(node: &ruby_prism::CallNode<'pr>) -> Option<ruby_prism::Node<'pr>> {
    let arguments = node.arguments()?.arguments();
    if arguments.len() != 1 {
        return None;
    }
    arguments.first()
}

fn source_slice(source: &[u8], range: Range) -> Option<&[u8]> {
    source.get(range.start_offset as usize..range.end_offset as usize)
}

#[cfg(test)]
mod tests {
    use crate::cops::style::NilComparison;
    use crate::cops::support::run_single_cop;

    #[test]
    fn corrects_equal_nil_to_nil_query() {
        let offenses = run_single_cop(Box::new(NilComparison), "x == nil\n");

        assert_eq!(offenses.len(), 1);
        assert_eq!(
            offenses[0].message,
            "Prefer `nil?` to comparing with `nil`."
        );
        let edit = &offenses[0].autocorrect.as_ref().unwrap().edits[0];
        assert_eq!(edit.range.start_offset, 0);
        assert_eq!(edit.range.end_offset, 8);
        assert_eq!(edit.replacement, "x.nil?");
    }

    #[test]
    fn corrects_not_equal_nil_to_negated_nil_query() {
        let offenses = run_single_cop(Box::new(NilComparison), "value != nil\n");

        assert_eq!(offenses.len(), 1);
        let edit = &offenses[0].autocorrect.as_ref().unwrap().edits[0];
        assert_eq!(edit.range.start_offset, 0);
        assert_eq!(edit.range.end_offset, 12);
        assert_eq!(edit.replacement, "!value.nil?");
    }

    #[test]
    fn leaves_nil_on_left_clean() {
        let offenses = run_single_cop(Box::new(NilComparison), "nil == value\n");

        assert!(offenses.is_empty());
    }

    #[test]
    fn leaves_commented_comparison_range_clean() {
        let offenses = run_single_cop(Box::new(NilComparison), "value == # keep\n  nil\n");

        assert!(offenses.is_empty());
    }
}
