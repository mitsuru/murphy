//! Rails `db/schema.rb` view for schema-dependent cops (murphy-s0vb).
//!
//! [`RailsSchema`] is the engine-provided table/index view that
//! `Rails/UniqueValidationWithoutIndex` and `Rails/UnusedIgnoredColumns`
//! read through [`Cx`](crate::Cx). The host (`murphy-core`) loads
//! `db/schema.rb` once per run, parses it to an [`murphy_ast::Ast`] via
//! `murphy-translate`, extracts tables with [`schema_from_ast`], serializes
//! to JSON, and threads the JSON into every cop's [`CxRaw`](crate::CxRaw)
//! (`rails_schema_json`, ABI v4 lockstep, no numeric bump per policy).
//! The plugin decodes with [`RailsSchema::from_json`] — no filesystem
//! access inside the `.so`, no per-cop re-parse.
//!
//! Without schema JSON (`None`/empty) both cops do nothing, matching
//! rubocop-rails 2.35.0 (`return unless schema`).

use murphy_ast::{Ast, NodeId, NodeKind};

/// One `create_table` entry.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct RailsTable {
    /// Table name as written in `create_table "name"`.
    pub name: String,
    /// Column names from `t.<type> "col"` (excluding `t.index`).
    pub columns: Vec<String>,
    /// Indices from `t.index ...` plus merged top-level `add_index`.
    pub indices: Vec<RailsIndex>,
}

/// One `t.index` / `add_index` entry.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct RailsIndex {
    /// Indexed columns (empty for expression indices).
    pub columns: Vec<String>,
    /// Raw expression for `t.index 'lower(col)'` (None for column lists).
    pub expression: Option<String>,
    /// `true` when `unique: ...` is present (mirrors upstream
    /// `Index#analyze_keywords!` which sets `@unique = true` on key
    /// presence, ignoring the value).
    pub unique: bool,
}

/// Whole-schema view: all `create_table` tables.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct RailsSchema {
    pub tables: Vec<RailsTable>,
}

impl RailsSchema {
    /// Decode host-provided JSON. `None` on empty/invalid input.
    pub fn from_json(json: &str) -> Option<Self> {
        if json.trim().is_empty() {
            return None;
        }
        serde_json::from_str(json).ok()
    }

    /// Encode for the `CxRaw` wire.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"tables":[]}"#.to_owned())
    }

    /// Find a table by exact name.
    pub fn table_by(&self, name: &str) -> Option<&RailsTable> {
        self.tables.iter().find(|t| t.name == name)
    }
}

impl RailsTable {
    /// `true` when `name` is a declared column.
    pub fn with_column(&self, name: &str) -> bool {
        self.columns.iter().any(|c| c == name)
    }
}

/// Extract tables from a parsed `db/schema.rb` AST.
///
/// Searches all `create_table "name" do |t| ... end` blocks (any nesting —
/// schema.rb only contains the `Schema.define` wrapper) and all bare
/// `add_index "table", ...` sends, merging the latter into matching tables.
/// Returns an empty schema (no tables) when nothing matches; the caller
/// decides whether "no schema file" (no JSON) vs "empty schema" (empty
/// tables) matters — both yield no offenses downstream.
pub fn schema_from_ast(ast: &Ast) -> RailsSchema {
    let raw = ast.raw_parts();
    let lists = raw.node_lists;
    let get_list = |l: murphy_ast::NodeList| -> &[NodeId] {
        let s = l.start as usize;
        let e = s + l.len as usize;
        if e <= lists.len() { &lists[s..e] } else { &[] }
    };
    let resolve_sym = |s: murphy_ast::Symbol| -> &str { ast.interner().resolve(s.0) };
    let resolve_str = |s: murphy_ast::StringId| -> &str { ast.interner().resolve(s.0) };

    // First-arg str/sym value of a Send, if present.
    let first_arg_value = |call: NodeId| -> Option<String> {
        let NodeKind::Send { args, .. } = *ast.kind(call) else {
            return None;
        };
        let args = get_list(args);
        let first = *args.first()?;
        match *ast.kind(first) {
            NodeKind::Str(sid) => Some(resolve_str(sid).to_owned()),
            NodeKind::Sym(sym) => Some(resolve_sym(sym).to_owned()),
            _ => None,
        }
    };

    // Method name of a Send, if Send.
    let send_method = |id: NodeId| -> Option<&str> {
        if let NodeKind::Send { method, .. } = *ast.kind(id) {
            Some(resolve_sym(method))
        } else {
            None
        }
    };
    // True for bare calls (`nil` receiver).
    let is_bare = |id: NodeId| -> bool {
        if let NodeKind::Send { receiver, .. } = *ast.kind(id) {
            receiver.get().is_none()
        } else {
            false
        }
    };
    // Call arguments slice.
    let call_args = |id: NodeId| -> &[NodeId] {
        if let NodeKind::Send { args, .. } = *ast.kind(id) {
            get_list(args)
        } else {
            &[]
        }
    };
    // Hash pairs of a Hash node.
    let hash_pairs = |id: NodeId| -> Vec<NodeId> {
        if let NodeKind::Hash(l) = *ast.kind(id) {
            get_list(l).to_vec()
        } else {
            Vec::new()
        }
    };
    // Pair key str/sym value.
    let pair_key_is = |pair: NodeId, want: &str| -> bool {
        let NodeKind::Pair { key, .. } = *ast.kind(pair) else {
            return false;
        };
        match *ast.kind(key) {
            NodeKind::Sym(sym) => resolve_sym(sym) == want,
            NodeKind::Str(sid) => resolve_str(sid) == want,
            _ => false,
        }
    };
    // `unique:` present in trailing hash args?
    let has_unique_flag = |call: NodeId| -> bool {
        for &arg in call_args(call) {
            if !matches!(*ast.kind(arg), NodeKind::Hash(_)) {
                continue;
            }
            for pair in hash_pairs(arg) {
                if pair_key_is(pair, "unique") {
                    return true;
                }
            }
        }
        false
    };
    // Array elements as strings (str/sym only; non-literals skipped).
    let array_string_values = |id: NodeId| -> Vec<String> {
        if let NodeKind::Array(l) = *ast.kind(id) {
            get_list(l)
                .iter()
                .filter_map(|&e| match *ast.kind(e) {
                    NodeKind::Str(sid) => Some(resolve_str(sid).to_owned()),
                    NodeKind::Sym(sym) => Some(resolve_sym(sym).to_owned()),
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        }
    };
    // Parse `t.index COLS_OR_EXPR, ...` (receiver form) into an index.
    let parse_t_index = |call: NodeId| -> Option<RailsIndex> {
        let args = call_args(call);
        let first = *args.first()?;
        let (columns, expression) = match *ast.kind(first) {
            NodeKind::Array(_) => (array_string_values(first), None),
            NodeKind::Str(sid) => (Vec::new(), Some(resolve_str(sid).to_owned())),
            NodeKind::Sym(sym) => (Vec::new(), Some(resolve_sym(sym).to_owned())),
            _ => return None,
        };
        Some(RailsIndex {
            columns,
            expression,
            unique: has_unique_flag(call),
        })
    };
    // Parse bare `add_index TABLE, COLS_OR_EXPR, ...` into (table, index).
    let parse_add_index = |call: NodeId| -> Option<(String, RailsIndex)> {
        let args = call_args(call);
        if args.len() < 2 {
            return None;
        }
        let table = match *ast.kind(args[0]) {
            NodeKind::Str(sid) => resolve_str(sid).to_owned(),
            NodeKind::Sym(sym) => resolve_sym(sym).to_owned(),
            _ => return None,
        };
        let (columns, expression) = match *ast.kind(args[1]) {
            NodeKind::Array(_) => (array_string_values(args[1]), None),
            NodeKind::Str(sid) => (Vec::new(), Some(resolve_str(sid).to_owned())),
            NodeKind::Sym(sym) => (Vec::new(), Some(resolve_sym(sym).to_owned())),
            _ => (Vec::new(), None),
        };
        Some((
            table,
            RailsIndex {
                columns,
                expression,
                unique: has_unique_flag(call),
            },
        ))
    };

    let mut schema = RailsSchema { tables: Vec::new() };

    // Pass 1: create_table blocks.
    let node_count = raw.nodes.len();
    for i in 0..node_count {
        let id = NodeId(i as u32);
        // Block form: `create_table ... do |t| ... end`.
        let (call, body) = match *ast.kind(id) {
            NodeKind::Block { call, body, .. } => (call, body.get()),
            NodeKind::Numblock { send, body, .. } => (send, body.get()),
            NodeKind::Itblock { send, body } => (send, body.get()),
            _ => continue,
        };
        if send_method(call) != Some("create_table") || !is_bare(call) {
            continue;
        }
        let Some(table_name) = first_arg_value(call) else {
            continue;
        };
        let mut table = RailsTable {
            name: table_name,
            columns: Vec::new(),
            indices: Vec::new(),
        };
        // Body statements: Begin list or single node.
        let mut stmts: Vec<NodeId> = Vec::new();
        if let Some(b) = body {
            if let NodeKind::Begin(l) = *ast.kind(b) {
                stmts.extend(get_list(l).iter().copied());
            } else {
                stmts.push(b);
            }
        }
        for stmt in stmts {
            if !matches!(*ast.kind(stmt), NodeKind::Send { .. }) {
                continue;
            }
            let method = send_method(stmt).unwrap_or("");
            if method == "index" {
                if let Some(idx) = parse_t_index(stmt) {
                    // Skip empty non-unique noise? No — keep all; cops filter.
                    table.indices.push(idx);
                }
                continue;
            }
            // Any other `t.<type> "col"` is a column when first arg is str/sym.
            // `t.check_constraint nil, ...` has a nil first arg → ignored.
            // `t.timestamps` has no args → ignored (no column name).
            if let Some(col) = first_arg_value(stmt) {
                // Avoid duplicating the table-name arg of nested create_table?
                // Nested create_table cannot appear inside a table body in
                // real schema.rb; no guard needed.
                if !table.columns.contains(&col) {
                    table.columns.push(col);
                }
            }
        }
        schema.tables.push(table);
    }

    // Pass 2: top-level add_index merges.
    for i in 0..node_count {
        let id = NodeId(i as u32);
        if !matches!(*ast.kind(id), NodeKind::Send { .. }) {
            continue;
        }
        if send_method(id) != Some("add_index") || !is_bare(id) {
            continue;
        }
        // Skip `t.add_index` receiver forms (none in real schema.rb, but the
        // bare guard above already excludes them since `t.` is a receiver).
        let Some((table_name, idx)) = parse_add_index(id) else {
            continue;
        };
        if let Some(t) = schema.tables.iter_mut().find(|t| t.name == table_name) {
            t.indices.push(idx);
        }
    }

    schema
}

/// Approximate ActiveSupport `tableize` for `table_name` inference.
///
/// `class_path` is `::`-joined (`Admin::User`, `User`). Joins namespaces
/// with `_`, underscores `CamelCase`, then naive-pluralizes. Already-plural
/// names (ending in `s`) are kept as-is to avoid `articleses`; singular
/// `s`-endings (`Status` → `status` instead of `statuses`) are a known
/// false-negative-safe gap (lookup misses → no offense, never a false
/// positive).
pub fn tableize(class_path: &str) -> String {
    let path = class_path.strip_prefix("::").unwrap_or(class_path);
    let parts: Vec<String> = path.split("::").map(underscore).collect();
    let joined = parts.join("_");
    pluralize(&joined)
}

fn underscore(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                let prev = chars[i - 1];
                let next = chars.get(i + 1).copied();
                // `aB` → `a_b`; `ABc` → `a_bc` (acronym boundary).
                if prev.is_ascii_lowercase()
                    || prev.is_ascii_digit()
                    || (prev.is_ascii_uppercase() && next.is_some_and(|n| n.is_ascii_lowercase()))
                {
                    out.push('_');
                }
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn pluralize(word: &str) -> String {
    if word.is_empty() {
        return String::new();
    }
    // Idempotent for already-plural regulars (`users`, `articles`): avoids
    // `articleses` for plural class names like `WrittenArticles`.
    // Known FN gap: singular s-endings (`status` → `status`, not `statuses`).
    if word.ends_with('s') {
        return word.to_owned();
    }
    if word.ends_with("ch") || word.ends_with('x') || word.ends_with('z') || word.ends_with("sh") {
        return format!("{word}es");
    }
    // consonant + y → ies (`category` → `categories`).
    if word.ends_with('y') && word.len() >= 2 {
        let b = word.as_bytes();
        let prev = b[word.len() - 2] as char;
        if !"aeiouAEIOU".contains(prev) {
            return format!("{}ies", &word[..word.len() - 1]);
        }
    }
    format!("{word}s")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tableize_covers_spec_shapes() {
        assert_eq!(tableize("User"), "users");
        assert_eq!(tableize("Article"), "articles");
        assert_eq!(tableize("Email"), "emails");
        assert_eq!(tableize("Admin::User"), "admin_users");
        assert_eq!(tableize("::Admin::User"), "admin_users");
        // Already-plural class name stays singular-plural stable.
        assert_eq!(tableize("WrittenArticles"), "written_articles");
    }

    #[test]
    fn json_round_trip() {
        let s = RailsSchema {
            tables: vec![RailsTable {
                name: "users".to_owned(),
                columns: vec!["account".to_owned()],
                indices: vec![RailsIndex {
                    columns: vec!["account".to_owned()],
                    expression: None,
                    unique: true,
                }],
            }],
        };
        let json = s.to_json();
        assert_eq!(RailsSchema::from_json(&json), Some(s));
        assert_eq!(RailsSchema::from_json(""), None);
        assert_eq!(RailsSchema::from_json("not json"), None);
    }
}
