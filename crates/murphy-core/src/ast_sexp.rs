use crate::Ast;
use ruby_prism::Node;

pub fn ast_to_sexp(ast: &Ast<'_>) -> String {
    render_node(ast.root())
}

fn render_node(node: Node<'_>) -> String {
    if let Some(program) = node.as_program_node() {
        let statements: Vec<String> = program
            .statements()
            .body()
            .iter()
            .map(render_node)
            .collect();

        return match statements.len() {
            0 => "s(:begin)".to_string(),
            1 => statements.into_iter().next().unwrap(),
            _ => format!("s(:begin, {})", statements.join(", ")),
        };
    }
    if let Some(call) = node.as_call_node() {
        return render_call(&call);
    }
    if node.as_nil_node().is_some() {
        return "s(:nil)".to_string();
    }
    render_unknown(node)
}

fn render_call(call: &ruby_prism::CallNode<'_>) -> String {
    let name = bytes_to_string(call.name().as_slice());
    if call.receiver().is_none() && call.arguments().is_none() && is_identifier(&name) {
        return format!("s(:lvar, {})", render_symbol(&name));
    }

    let receiver = call
        .receiver()
        .map(render_node)
        .unwrap_or_else(|| "nil".to_string());
    let mut parts = vec!["s(:send".to_string(), receiver, render_symbol(&name)];
    if let Some(arguments) = call.arguments() {
        for arg in arguments.arguments().iter() {
            parts.push(render_node(arg));
        }
    }
    format!("{})", parts.join(", "))
}

fn render_unknown(node: Node<'_>) -> String {
    format!("s(:unknown, {:?})", format!("{node:?}"))
}

fn bytes_to_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn render_symbol(value: &str) -> String {
    if is_identifier(value) || matches!(value, "==" | "!=" | "<" | ">" | "<=" | ">=") {
        format!(":{value}")
    } else {
        format!(":{}", quote_string(value))
    }
}

fn quote_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn sexp(source: &str) -> String {
        let ast = parse(source).expect("source parses");
        ast_to_sexp(&ast)
    }

    #[test]
    fn dumps_x_equal_nil() {
        assert_eq!(sexp("x == nil"), "s(:send, s(:lvar, :x), :==, s(:nil))");
    }

    #[test]
    fn dumps_nil_equal_x() {
        assert_eq!(sexp("nil == x"), "s(:send, s(:nil), :==, s(:lvar, :x))");
    }

    #[test]
    fn dumps_x_not_equal_nil() {
        assert_eq!(sexp("x != nil"), "s(:send, s(:lvar, :x), :!=, s(:nil))");
    }

    #[test]
    fn dumps_multiple_statements_as_begin_node() {
        assert_eq!(
            sexp("x == nil; nil == x"),
            "s(:begin, s(:send, s(:lvar, :x), :==, s(:nil)), s(:send, s(:nil), :==, s(:lvar, :x)))"
        );
    }
}
