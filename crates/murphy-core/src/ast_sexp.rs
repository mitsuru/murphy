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
    if node.as_true_node().is_some() {
        return "s(:true)".to_string();
    }
    if node.as_false_node().is_some() {
        return "s(:false)".to_string();
    }
    if let Some(integer) = node.as_integer_node() {
        return format!("s(:int, {})", bytes_to_string(integer.location().as_slice()));
    }
    if let Some(string) = node.as_string_node() {
        return format!(
            "s(:str, {})",
            quote_string(&strip_string_delimiters(&bytes_to_string(string.location().as_slice())))
        );
    }
    if let Some(symbol) = node.as_symbol_node() {
        return format!("s(:sym, {})", render_symbol(&bytes_to_string(symbol.unescaped())));
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

fn strip_string_delimiters(raw: &str) -> &str {
    let bytes = raw.as_bytes();
    if bytes.is_empty() {
        return "";
    }

    if bytes.len() >= 2 && (bytes[0] == b'"' || bytes[0] == b'\'') && bytes[bytes.len() - 1] == bytes[0]
    {
        &raw[1..raw.len() - 1]
    } else if bytes.len() >= 2 && bytes[0] == b'%' {
        let (delimiter, body_start) = match bytes.get(1) {
            Some(&b'Q') | Some(&b'q') | Some(&b'W') | Some(&b'w') | Some(&b'X') | Some(&b'x')
            | Some(&b'I') | Some(&b'i') | Some(&b'R') | Some(&b'r') => {
                if bytes.len() < 3 {
                    return raw;
                }
                (bytes[2], 3)
            }
            Some(&delimiter) => (delimiter, 2),
            None => return raw,
        };

        let close = match delimiter {
            b'(' => b')',
            b'[' => b']',
            b'{' => b'}',
            b'<' => b'>',
            b'|' => b'|',
            b'/' => b'/',
            b'!' => b'!',
            b'"' => b'"',
            b'\'' => b'\'',
            open @ _ => open,
        };

        let end = bytes.len().saturating_sub(1);
        if bytes.len() > body_start && bytes[end] == close {
            &raw[body_start..end]
        } else {
            raw
        }
    } else {
        raw
    }
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
    fn dumps_receiver_and_argument_call() {
        assert_eq!(sexp("obj.foo(1)"), "s(:send, s(:lvar, :obj), :foo, s(:int, 1))");
    }

    #[test]
    fn dumps_receiverless_method_call() {
        assert_eq!(sexp("foo(1)"), "s(:send, nil, :foo, s(:int, 1))");
    }

    #[test]
    fn dumps_basic_literals() {
        assert_eq!(sexp("true"), "s(:true)");
        assert_eq!(sexp("false"), "s(:false)");
        assert_eq!(sexp("1"), "s(:int, 1)");
        assert_eq!(sexp("'x'"), "s(:str, \"x\")");
        assert_eq!(sexp(":x"), "s(:sym, :x)");
    }

    #[test]
    fn escapes_string_literals() {
        assert_eq!(sexp("'a\\nb'"), "s(:str, \"a\\\\nb\")");
    }

    #[test]
    fn escapes_double_quoted_strings_distinctly() {
        assert_eq!(sexp("\"\\n\""), "s(:str, \"\\\\n\")");
        assert_eq!(sexp("\"\n\""), "s(:str, \"\\n\")");
    }

    #[test]
    fn percent_string_delimiters_are_stripped() {
        assert_eq!(sexp("%q(foo)"), "s(:str, \"foo\")");
        assert_eq!(sexp("%Q(\\n)"), "s(:str, \"\\\\n\")");
    }

    #[test]
    fn dumps_multiple_statements_as_begin_node() {
        assert_eq!(
            sexp("x == nil; nil == x"),
            "s(:begin, s(:send, s(:lvar, :x), :==, s(:nil)), s(:send, s(:nil), :==, s(:lvar, :x)))"
        );
    }
}
