use crate::cop::{Cop, CopContext};
use crate::cops::support::simple_receiver_name;
use crate::{Offense, Range, Severity};

pub struct Debugger;

const BARE_DEBUGGER_CALLS: [&[u8]; 3] = [b"debugger", b"byebug", b"pry"];
const BINDING_DEBUGGER_CALLS: [&[u8]; 4] = [b"pry", b"irb", b"b", b"break"];
const REQUIRE_DEBUG_PATH: [&[u8]; 2] = [b"'debug/start'", b"\"debug/start\""];

impl Cop for Debugger {
    fn name(&self) -> &str {
        "Lint/Debugger"
    }

    fn on_call_node(
        &self,
        node: &ruby_prism::CallNode<'_>,
        ctx: &CopContext<'_>,
        sink: &mut Vec<Offense>,
    ) {
        let name = node.name();
        let name = name.as_slice();

        let flagged = if is_require_debug_start(node) {
            true
        } else {
            match node.receiver() {
                None => BARE_DEBUGGER_CALLS.contains(&name),
                Some(receiver) => {
                    simple_receiver_name(receiver.location().as_slice())
                        == Some(b"binding".as_slice())
                        && BINDING_DEBUGGER_CALLS.contains(&name)
                }
            }
        };
        if !flagged {
            return;
        }

        let Some(loc) = node.message_loc() else {
            return;
        };
        sink.push(Offense::new(
            ctx.file,
            self.name(),
            Range::from_prism_location(&loc),
            Severity::Warning,
            "Remove debugger entry point.",
        ));
    }
}

fn is_require_debug_start(node: &ruby_prism::CallNode<'_>) -> bool {
    if node.receiver().is_some() {
        return false;
    }

    if node.name().as_slice() != b"require" {
        return false;
    }

    let Some(arguments) = node.arguments() else {
        return false;
    };

    let Some(argument) = arguments.arguments().first() else {
        return false;
    };

    if arguments.arguments().len() != 1 {
        return false;
    }

    let value = argument.location().as_slice();
    REQUIRE_DEBUG_PATH.contains(&value)
}

#[cfg(test)]
mod tests {
    use crate::cops::lint::Debugger;
    use crate::cops::support::run_single_cop;

    #[test]
    fn flags_binding_pry_and_byebug() {
        let offenses = run_single_cop(Box::new(Debugger), "binding.pry\nbyebug\n");

        assert_eq!(offenses.len(), 2);
        assert!(
            offenses
                .iter()
                .all(|offense| offense.cop_name == "Lint/Debugger")
        );
    }

    #[test]
    fn flags_supported_debugger_calls() {
        let offenses = run_single_cop(
            Box::new(Debugger),
            "binding.pry\nbinding.irb\ndebugger\nbyebug\n",
        );

        assert_eq!(offenses.len(), 4);
    }

    #[test]
    fn flags_parenthesized_binding_receiver() {
        let offenses = run_single_cop(Box::new(Debugger), "(binding).pry\n");

        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range.start_offset, 10);
        assert_eq!(offenses[0].range.end_offset, 13);
    }

    #[test]
    fn flags_require_debug_start() {
        let offenses = run_single_cop(
            Box::new(Debugger),
            "require 'debug/start'\nrequire \"debug/start\"\n",
        );

        assert_eq!(offenses.len(), 2);
        assert!(
            offenses
                .iter()
                .all(|offense| offense.cop_name == "Lint/Debugger")
        );
    }

    #[test]
    fn ignores_non_literal_require_call() {
        let offenses = run_single_cop(
            Box::new(Debugger),
            "require path\nrequire_file('debug/start')\nrequire(\"debug/#{name}\")\n",
        );

        assert!(offenses.is_empty());
    }
}
