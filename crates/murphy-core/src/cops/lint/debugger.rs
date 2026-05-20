use crate::cop::{Cop, CopContext};
use crate::{Offense, Range, Severity};

pub struct Debugger;

const BARE_DEBUGGER_CALLS: [&[u8]; 3] = [b"debugger", b"byebug", b"pry"];
const BINDING_DEBUGGER_CALLS: [&[u8]; 4] = [b"pry", b"irb", b"b", b"break"];

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

        let flagged = match node.receiver() {
            None => BARE_DEBUGGER_CALLS.contains(&name),
            Some(receiver) => {
                receiver.location().as_slice() == b"binding"
                    && BINDING_DEBUGGER_CALLS.contains(&name)
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
}
