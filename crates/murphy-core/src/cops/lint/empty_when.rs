use crate::cop::{Cop, CopContext};
use crate::{Offense, Range, Severity};

pub struct EmptyWhen;

impl Cop for EmptyWhen {
    fn name(&self) -> &str {
        "Lint/EmptyWhen"
    }

    fn on_call_node(
        &self,
        _node: &ruby_prism::CallNode<'_>,
        _ctx: &CopContext<'_>,
        _sink: &mut Vec<Offense>,
    ) {
    }

    fn on_case_node(
        &self,
        node: &ruby_prism::CaseNode<'_>,
        ctx: &CopContext<'_>,
        sink: &mut Vec<Offense>,
    ) {
        for when_node in &node.conditions() {
            let Some(when_node) = when_node.as_when_node() else {
                continue;
            };

            if has_no_statements(&when_node) {
                let range = Range::from_prism_location(&when_node.location());
                sink.push(Offense::new(
                    ctx.file,
                    self.name(),
                    range,
                    Severity::Warning,
                    "This `when` branch is empty.",
                ));
            }
        }
    }
}

fn has_no_statements(node: &ruby_prism::WhenNode<'_>) -> bool {
    let Some(statements) = node.statements() else {
        return true;
    };

    statements.body().is_empty()
}

#[cfg(test)]
mod tests {
    use crate::cops::lint::EmptyWhen;
    use crate::cops::support::run_single_cop;

    #[test]
    fn flags_when_without_statements() {
        let offenses = run_single_cop(Box::new(EmptyWhen), "case x\nwhen 1\nend\n");

        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].cop_name, "Lint/EmptyWhen");
        assert_eq!(offenses[0].message, "This `when` branch is empty.");
    }

    #[test]
    fn leaves_non_empty_when_alone() {
        let offenses = run_single_cop(Box::new(EmptyWhen), "case x\nwhen 1\n  puts 1\nend\n");

        assert!(offenses.is_empty());
    }

    #[test]
    fn flags_each_empty_when_in_a_case() {
        let offenses = run_single_cop(
            Box::new(EmptyWhen),
            "case x\nwhen 1\nwhen 2\nelse\n  puts 3\nend\n",
        );

        assert_eq!(offenses.len(), 2);
    }

    #[test]
    fn handles_empty_else_clause() {
        let offenses = run_single_cop(Box::new(EmptyWhen), "case x\nwhen 1\nelse\n  puts 3\nend\n");

        assert_eq!(offenses.len(), 1);
    }
}
