use crate::cop::{Cop, CopContext};
use crate::cops::support::{offense_with_edit, replace_edit, simple_receiver_name};
use crate::{Offense, Range};

pub struct DeprecatedClassMethods;

const DEPRECATED: &[(&str, &str, &str)] =
    &[("File", "exists?", "exist?"), ("Dir", "exists?", "exist?")];

impl Cop for DeprecatedClassMethods {
    fn name(&self) -> &str {
        "Lint/DeprecatedClassMethods"
    }

    fn on_call_node(
        &self,
        node: &ruby_prism::CallNode<'_>,
        ctx: &CopContext<'_>,
        sink: &mut Vec<Offense>,
    ) {
        let Some(receiver) = node.receiver() else {
            return;
        };
        let receiver = receiver.location();
        let Some(receiver) = simple_receiver_name(receiver.as_slice()) else {
            return;
        };
        let name = node.name();
        let name = name.as_slice();

        let Some((_, _, replacement)) = DEPRECATED
            .iter()
            .find(|(class, method, _)| receiver == class.as_bytes() && name == method.as_bytes())
        else {
            return;
        };

        let Some(loc) = node.message_loc() else {
            return;
        };
        let range = Range::from_prism_location(&loc);
        sink.push(offense_with_edit(
            ctx.file,
            self.name(),
            range,
            "Use the non-deprecated class method.",
            replace_edit(range.start_offset, range.end_offset, replacement),
        ));
    }
}

#[cfg(test)]
mod tests {
    use crate::cops::lint::DeprecatedClassMethods;
    use crate::cops::support::run_single_cop;

    #[test]
    fn corrects_deprecated_file_and_dir_exists() {
        let offenses = run_single_cop(
            Box::new(DeprecatedClassMethods),
            "File.exists?(path)\nDir.exists?(path)\n",
        );
        let replacements: Vec<_> = offenses
            .iter()
            .map(|o| {
                o.autocorrect.as_ref().unwrap().edits[0]
                    .replacement
                    .as_str()
            })
            .collect();
        assert_eq!(replacements, vec!["exist?", "exist?"]);
    }

    #[test]
    fn corrects_simple_wrapped_class_receivers() {
        let offenses = run_single_cop(
            Box::new(DeprecatedClassMethods),
            "::File.exists?(path)\n::Dir.exists?(path)\n(File).exists?(path)\n(Dir).exists?(path)\n",
        );
        let ranges_and_replacements: Vec<_> = offenses
            .iter()
            .map(|o| {
                let edit = &o.autocorrect.as_ref().unwrap().edits[0];
                (
                    edit.range.start_offset,
                    edit.range.end_offset,
                    edit.replacement.as_str(),
                )
            })
            .collect();

        assert_eq!(
            ranges_and_replacements,
            vec![
                (7, 14, "exist?"),
                (27, 34, "exist?"),
                (48, 55, "exist?"),
                (68, 75, "exist?"),
            ]
        );
    }
}
