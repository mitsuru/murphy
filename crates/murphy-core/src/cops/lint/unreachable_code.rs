use crate::cop::{Cop, CopContext};
use crate::{Offense, Range, Severity};
use ruby_prism::Visit;

pub struct UnreachableCode;

impl Cop for UnreachableCode {
    fn name(&self) -> &str {
        "Lint/UnreachableCode"
    }

    fn inspect_file(&self, ctx: &CopContext<'_>, sink: &mut Vec<Offense>) {
        let source = match std::str::from_utf8(ctx.source) {
            Ok(source) => source,
            Err(_) => return,
        };
        let ast = match crate::parse::parse(source) {
            Ok(ast) => ast,
            Err(_) => return,
        };
        let mut visitor = UnreachableCodeVisitor::new(self.name(), ctx.file, sink);
        visitor.visit(&ast.root());
    }

    fn on_call_node(
        &self,
        _node: &ruby_prism::CallNode<'_>,
        _ctx: &CopContext<'_>,
        _sink: &mut Vec<Offense>,
    ) {
    }
}

struct UnreachableCodeVisitor<'a, 'file> {
    cop_name: &'a str,
    file: &'file str,
    sink: &'a mut Vec<Offense>,
}

impl<'a, 'file> UnreachableCodeVisitor<'a, 'file> {
    fn new(cop_name: &'a str, file: &'file str, sink: &'a mut Vec<Offense>) -> Self {
        Self {
            cop_name,
            file,
            sink,
        }
    }

    fn inspect_statements(&mut self, node: &ruby_prism::StatementsNode<'_>) {
        let statements = node.body();

        let mut terminated = false;
        for statement in &statements {
            if terminated {
                let range = Range::from_prism_location(&statement.location());
                self.sink.push(Offense::new(
                    self.file,
                    self.cop_name,
                    range,
                    Severity::Warning,
                    "This code is unreachable.",
                ));
            }

            if statement_is_terminator(&statement) {
                terminated = true;
            }
        }
    }
}

impl<'pr, 'a, 'file> Visit<'pr> for UnreachableCodeVisitor<'a, 'file> {
    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'pr>) {
        self.inspect_statements(node);
        ruby_prism::visit_statements_node(self, node);
    }
}

fn statement_is_terminator(node: &ruby_prism::Node<'_>) -> bool {
    node.as_return_node().is_some()
        || node.as_break_node().is_some()
        || node.as_next_node().is_some()
        || node.as_redo_node().is_some()
        || is_raise_call(node)
}

fn is_raise_call(node: &ruby_prism::Node<'_>) -> bool {
    let Some(node) = node.as_call_node() else {
        return false;
    };

    if node.receiver().is_some() {
        return false;
    }

    node.name().as_slice() == b"raise"
}

#[cfg(test)]
mod tests {
    use crate::cops::lint::UnreachableCode;
    use crate::cops::support::run_single_cop;

    #[test]
    fn flags_every_statement_after_return() {
        let source = "def x\n  return 1\n  puts 1\n  puts 2\nend\n";
        let offenses = run_single_cop(Box::new(UnreachableCode), source);

        assert_eq!(offenses.len(), 2);
        assert_eq!(offenses[0].message, "This code is unreachable.");
        assert_eq!(offenses[1].message, "This code is unreachable.");
    }

    #[test]
    fn flags_after_next_and_break() {
        let source = "[1, 2].each do |x|\n  next\n  puts x\n  puts x\nend\n";
        let offenses = run_single_cop(Box::new(UnreachableCode), source);

        assert_eq!(offenses.len(), 2);
    }

    #[test]
    fn does_not_flag_across_nested_statement_blocks() {
        let source = "if cond\n  return\n  puts 1\nend\nputs 2\n";
        let offenses = run_single_cop(Box::new(UnreachableCode), source);

        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn flags_unreachable_after_raise() {
        let offenses = run_single_cop(Box::new(UnreachableCode), "raise\nputs 1\n");

        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn skip_if_terminator_has_parent_condition() {
        let source = "if cond\n  puts 1\n  return\nend\nputs 2\n";
        let offenses = run_single_cop(Box::new(UnreachableCode), source);

        assert_eq!(offenses.len(), 0);
    }
}
