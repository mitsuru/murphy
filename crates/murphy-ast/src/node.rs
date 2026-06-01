//! Core node types for the Murphy arena AST. See ADR 0037.

/// Index into [`Ast::nodes`](crate::Ast). 32-bit: an arena holds one file.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

/// Optional [`NodeId`]. Uses the sentinel `u32::MAX` for `None` rather than
/// relying on an enum niche, so the layout is explicit across the ABI
/// (ADR 0037).
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OptNodeId(pub u32);

impl OptNodeId {
    /// The `None` sentinel.
    pub const NONE: OptNodeId = OptNodeId(u32::MAX);

    /// Wrap a present [`NodeId`].
    pub fn some(id: NodeId) -> OptNodeId {
        debug_assert!(
            id.0 != u32::MAX,
            "NodeId u32::MAX collides with the OptNodeId sentinel"
        );
        OptNodeId(id.0)
    }

    /// Resolve to an `Option`.
    pub fn get(self) -> Option<NodeId> {
        if self.0 == u32::MAX {
            None
        } else {
            Some(NodeId(self.0))
        }
    }

    /// `true` iff this is the sentinel.
    pub fn is_none(self) -> bool {
        self.0 == u32::MAX
    }
}

impl From<Option<NodeId>> for OptNodeId {
    fn from(o: Option<NodeId>) -> Self {
        match o {
            Some(id) => OptNodeId::some(id),
            None => OptNodeId::NONE,
        }
    }
}

/// Interned identifier (method name, variable name, …). Index into the
/// [`Interner`](crate::Interner).
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Symbol(pub u32);

/// Interned string-literal contents. Index into the
/// [`Interner`](crate::Interner).
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StringId(pub u32);

/// A half-open byte range into the source buffer.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Range {
    pub start: u32,
    pub end: u32,
}

impl Range {
    /// The empty range `[0, 0)`. Used as the sentinel for "no recorded
    /// position", notably on [`NodeLoc::name`] for nodes without an
    /// identifier.
    pub const ZERO: Range = Range { start: 0, end: 0 };
}

/// Per-node source-location bundle — Murphy's analog of the parser
/// gem's `node.loc` accessor. `expression` is the AST node's full
/// source range; `name` is the identifier source range (the
/// `node.loc.name` analog) and is [`Range::ZERO`] for nodes without
/// an identifier (literals, atoms, structural nodes).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeLoc {
    pub expression: Range,
    pub name: Range,
}

/// Parser-provided closing paren for a call node's own argument list.
///
/// Stored out-of-line so [`NodeLoc`] stays ABI-stable and non-call nodes pay
/// no per-node storage cost.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallClosingLoc {
    pub node: NodeId,
    pub closing: Range,
}

/// Parser-provided call operator for a call node (`.` or `&.`).
///
/// Stored out-of-line so [`NodeLoc`] stays compact and call nodes without an
/// explicit operator pay no per-node storage cost.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallOperatorLoc {
    pub node: NodeId,
    pub operator: Range,
}

/// A compact source token for RuboCop-style layout cops.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceToken {
    pub range: Range,
    pub kind: SourceTokenKind,
}

/// Murphy's compact classification of Prism source tokens.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceTokenKind {
    LeftParen,
    RightParen,
    Comment,
    Newline,
    IgnoredNewline,
    HeredocStart,
    HeredocEnd,
    Other,
    // New variants are appended at the tail to preserve the `#[repr(u8)]`
    // discriminants of the existing kinds (the serialized format and the
    // plugin ABI both encode `kind as u8`). Adding a variant here is an
    // additive ABI change: bump `MURPHY_PLUGIN_ABI_VERSION` and
    // `FORMAT_VERSION`.
    /// A `,` separator token (`PM_TOKEN_COMMA`). Consumed by comma-spacing
    /// and trailing-comma layout cops.
    Comma,
    /// A `{` opening brace for a hash literal or brace block
    /// (`PM_TOKEN_BRACE_LEFT`). Distinct from string-interpolation `#{`
    /// (`PM_TOKEN_EMBEXPR_BEGIN`) and lambda `-> {` (`PM_TOKEN_LAMBDA_BEGIN`),
    /// which keep their own prism token types and so fall through to
    /// [`SourceTokenKind::Other`].
    LeftBrace,
    /// A `}` closing brace (`PM_TOKEN_BRACE_RIGHT`). Closes a hash literal,
    /// a brace block, *or* a lambda body (`-> { }`); string interpolation
    /// `}` is `PM_TOKEN_EMBEXPR_END` and stays [`SourceTokenKind::Other`].
    RightBrace,
}

/// A reference to a contiguous slice of `node_lists` — the side table for
/// variable-length children (call args, array elements, …).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeList {
    pub start: u32,
    pub len: u32,
}

impl NodeList {
    /// The empty list.
    pub const EMPTY: NodeList = NodeList { start: 0, len: 0 };
}

/// A single AST node: a fixed-size POD value. The discriminated payload
/// lives in `kind`; `parent` is filled in by [`AstBuilder::finish`].
///
/// `loc.expression` is the node's full source range; `loc.name` is the
/// identifier range when the node has one (the parser-gem `node.loc.name`
/// analog), otherwise [`Range::ZERO`].
#[repr(C)]
// No `Eq`: `NodeKind` carries `Float(f64)`, and `f64` is not `Eq`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AstNode {
    pub kind: NodeKind,
    /// Parent node. `OptNodeId::NONE` for the root.
    pub parent: OptNodeId,
    pub loc: NodeLoc,
}

/// The kind of an AST node, with its inline payload.
///
/// `#[repr(C, u8)]` gives a stable layout with a `u8` discriminant. The
/// **declaration order is the discriminant** and is **frozen** — new
/// variants append at the end only (ADR 0037). v1 follows the Ruby
/// `parser` gem's node shapes.
#[repr(C, u8)]
// No `Eq`: the `Float(f64)` variant means `f64` participates, and it is
// not `Eq`. `PartialEq` is enough for the round-trip equality test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeKind {
    /// A prism parse error. Dispatch skips it so syntax errors never crash
    /// a cop.
    Error,

    // --- atoms / literals ---
    Nil,
    True_,
    False_,
    SelfExpr,
    Int(i64),
    Float(f64),
    Str(StringId),
    Sym(Symbol),

    // --- variable reads ---
    Lvar(Symbol),
    Ivar(Symbol),
    Cvar(Symbol),
    Gvar(Symbol),
    Const {
        scope: OptNodeId,
        name: Symbol,
    },

    // --- assignments ---
    Lvasgn {
        name: Symbol,
        value: OptNodeId,
    },
    Ivasgn {
        name: Symbol,
        value: OptNodeId,
    },
    Casgn {
        scope: OptNodeId,
        name: Symbol,
        value: OptNodeId,
    },

    // --- calls / blocks ---
    Send {
        receiver: OptNodeId,
        method: Symbol,
        args: NodeList,
    },
    /// Safe-navigation call (`&.`). The receiver is always present.
    Csend {
        receiver: NodeId,
        method: Symbol,
        args: NodeList,
    },
    Block {
        call: NodeId,
        /// The `args` node (always present, may be an empty `Args`).
        args: NodeId,
        body: OptNodeId,
    },
    BlockPass(OptNodeId),
    Splat(OptNodeId),

    // --- collections ---
    Array(NodeList),
    Hash(NodeList),
    Pair {
        key: NodeId,
        value: NodeId,
    },

    // --- control flow ---
    If {
        cond: NodeId,
        then_: OptNodeId,
        else_: OptNodeId,
    },
    Case {
        subject: OptNodeId,
        whens: NodeList,
        else_: OptNodeId,
    },
    When {
        conds: NodeList,
        body: OptNodeId,
    },
    Begin(NodeList),
    Return(OptNodeId),
    And {
        lhs: NodeId,
        rhs: NodeId,
    },
    Or {
        lhs: NodeId,
        rhs: NodeId,
    },

    // --- definitions ---
    Def {
        /// singleton method（`def self.foo`）なら `receiver` が `Some`。
        receiver: OptNodeId,
        name: Symbol,
        args: NodeId,
        body: OptNodeId,
    },
    Class {
        name: NodeId,
        superclass: OptNodeId,
        body: OptNodeId,
    },
    Module {
        name: NodeId,
        body: OptNodeId,
    },

    // --- arguments ---
    Args(NodeList),
    Arg(Symbol),

    // --- fallback ---
    /// A valid prism node with no `NodeKind` mapping yet. Dispatch may
    /// treat it as opaque; `murphy-translate` never panics on unknown
    /// input. Distinct from `Error` (a prism *parse* error).
    Unknown,

    // --- assignments (appended post-`Unknown` per ADR 0037: variants are
    // append-only; declaration order is the frozen discriminant) ---
    /// `$g = expr` — global-variable assignment.
    Gvasgn {
        name: Symbol,
        value: OptNodeId,
    },
    /// `@@c = expr` — class-variable assignment.
    Cvasgn {
        name: Symbol,
        value: OptNodeId,
    },

    // --- arguments (appended post-`Cvasgn` per ADR 0037: variants are
    // append-only; declaration order is the frozen discriminant) ---
    /// `def f(a = 1)` の `a = 1` — optional positional parameter.
    Optarg {
        name: Symbol,
        default: NodeId,
    },
    /// `*rest` — splat parameter. 匿名 `*` は `name` が空文字 interned。
    Restarg(Symbol),
    /// `def f(k:)` — required keyword parameter.
    Kwarg(Symbol),
    /// `def f(k: 1)` — optional keyword parameter.
    Kwoptarg {
        name: Symbol,
        default: NodeId,
    },
    /// `**opts` — keyword splat parameter. 匿名 `**` は `name` が空文字 interned。
    Kwrestarg(Symbol),
    /// `&blk` — block parameter. 匿名 `&` は `name` が空文字 interned。
    Blockarg(Symbol),

    // --- collections (appended post-`Blockarg` per ADR 0037: variants are
    // append-only; declaration order is the frozen discriminant) ---
    /// `**h` — ハッシュ内のキーワード splat（`AssocSplatNode`）。匿名 `**` は
    /// 内側が `None`。
    Kwsplat(OptNodeId),

    // --- control flow (appended post-`Kwsplat` per ADR 0037: variants are
    // append-only; declaration order is the frozen discriminant) ---
    /// `while cond ... end`。`is_begin_modifier` を `post` に畳む
    /// （`while`/`while_post` の collapse）。
    While {
        cond: NodeId,
        body: OptNodeId,
        /// `true` なら do-while（`begin..end while c`）。
        post: bool,
    },
    /// `until cond ... end`。`post` は [`NodeKind::While`] と同じ意味。
    Until {
        cond: NodeId,
        body: OptNodeId,
        post: bool,
    },
    /// `a..b` / `a...b`（`RangeNode`）。beginless/endless は端が `None`。
    /// 型名 `RangeExpr` は既存のソース範囲 struct [`Range`] との衝突回避。
    RangeExpr {
        begin_: OptNodeId,
        end_: OptNodeId,
        /// `true` なら `...`（終端排他）。
        exclusive: bool,
    },

    // --- definitions (appended post-`RangeExpr` per ADR 0037: variants are
    // append-only; declaration order is the frozen discriminant) ---
    /// `class << expr ... end`（`SingletonClassNode`）— singleton class body。
    Sclass {
        expr: NodeId,
        body: OptNodeId,
    },

    // --- control flow / jumps (appended post-`Sclass` per ADR 0037: variants
    // are append-only; declaration order is the frozen discriminant) ---
    /// `break`（引数 0→`None`、1→その式、複数→`Array`）。
    Break(OptNodeId),
    /// `next`（`Break` と同じ引数畳み込み）。
    Next(OptNodeId),
    /// `yield`（引数リスト）。
    Yield(NodeList),
    /// `super(args)`（明示引数あり、`SuperNode`）。
    Super(NodeList),
    /// `super`（引数も括弧も無いゼロ引数 super、`ForwardingSuperNode`）。
    Zsuper,
    /// `defined?(expr)`。
    Defined(NodeId),

    // --- exceptions (appended post-`Defined` per ADR 0037: variants are
    // append-only; declaration order is the frozen discriminant) ---
    /// `begin..rescue..else..end` の rescue 構造。
    Rescue {
        /// 保護対象本体。
        body: OptNodeId,
        /// `Resbody` の並び。
        resbodies: NodeList,
        /// `else` 節。
        else_: OptNodeId,
    },
    /// 単一の `rescue Exc => e; ...` 節。
    Resbody {
        /// 捕捉する例外クラスの並び（無指定なら空）。
        exceptions: NodeList,
        /// `=> e` の束縛先（無ければ `None`）。
        var: OptNodeId,
        body: OptNodeId,
    },
    /// `ensure` 構造。`body` は保護本体（rescue 節 or 素の本体）。
    Ensure {
        body: OptNodeId,
        ensure_: OptNodeId,
    },

    // --- op-assign (appended post-`Ensure` per ADR 0037: variants are
    // append-only; declaration order is the frozen discriminant) ---
    /// `target op= value`（`+=` `-=` 等）。`target` は値なし write ノード
    /// （`Lvasgn`/`Ivasgn`/`Cvasgn`/`Gvasgn`/`Casgn` の `value` が `None`）。
    OpAsgn {
        target: NodeId,
        op: Symbol,
        value: NodeId,
    },
    /// `target ||= value`。`target` は値なし write ノード。
    OrAsgn {
        target: NodeId,
        value: NodeId,
    },
    /// `target &&= value`。`target` は値なし write ノード。
    AndAsgn {
        target: NodeId,
        value: NodeId,
    },

    // --- string interpolation / regexp / xstring (appended post-`AndAsgn`
    // per ADR 0037: variants are append-only; declaration order is the
    // frozen discriminant) ---
    /// 補間文字列 `"a#{b}"` / 隣接文字列連結（`InterpolatedStringNode`）。
    /// 部品の並び。
    Dstr(NodeList),
    /// 補間シンボル `:"a#{b}"`（`InterpolatedSymbolNode`）。
    Dsym(NodeList),
    /// バッククォート文字列 `` `cmd` ``（`XStringNode` /
    /// `InterpolatedXStringNode`、補間あり/なし両方）。部品の並び。
    Xstr(NodeList),
    /// 正規表現 `/re/imx`（`RegularExpressionNode` /
    /// `InterpolatedRegularExpressionNode`、補間あり/なし両方）。`opts` は
    /// フラグ文字列（`"imx"` 等）を interned した `Symbol`。
    Regexp {
        parts: NodeList,
        opts: Symbol,
    },

    // --- multiple assignment (appended post-`Regexp` per ADR 0037: variants
    // are append-only; declaration order is the frozen discriminant) ---
    /// 多重代入 `a, b = 1, 2`（`MultiWriteNode`）。`lhs` は `Mlhs`。
    Masgn {
        lhs: NodeId,
        rhs: NodeId,
    },
    /// 多重代入の左辺ターゲット並び（`MultiWriteNode` / `MultiTargetNode`）。
    Mlhs(NodeList),

    // --- HIGH-priority NodeKindTag extensions (murphy-w5ba, beads epic
    // murphy-xvjv). Added so RuboCop `def_node_matcher` strings that touch
    // these node kinds compile under `def_node_matcher!`. Subject-side support
    // (murphy-translate producing these from real Ruby source) lands per
    // node kind as cops actually need it — see the survey report at
    // docs/superpowers/specs/2026-05-29-rubocop-pattern-gap-survey.md.
    // Payload shapes mirror parser-gem's AST_FORMAT.md.
    /// `for x in iter; body; end`（`ForNode`）。
    For {
        var: NodeId,
        iter: NodeId,
        body: OptNodeId,
    },
    /// `-> { ... }` の短縮ラムダ（`LambdaNode`）。`block { send(:lambda) }`
    /// と異なる構文表現を区別する目印 — payload なしの marker variant。
    Lambda,
    /// `def self.foo(...)`（`SingletonMethodDefinitionNode`）。
    Defs {
        receiver: NodeId,
        name: Symbol,
        args: NodeId,
        body: OptNodeId,
    },
    /// `foo[i, j]` の bracket call。parser-gem は `send` から分離した `index`
    /// 表現を持つ。`receiver` + 添字 NodeList。
    Index {
        receiver: NodeId,
        args: NodeList,
    },
    /// `foo[i, j] = x` の bracket assignment。`args` は添字、`value` は最後の
    /// 引数 (parser-gem は同じ NodeList に詰めるが Murphy では分離保持)。
    IndexAsgn {
        receiver: NodeId,
        args: NodeList,
        value: NodeId,
    },
    /// `begin ... end` キーワード形式の begin block (parser-gem `kwbegin`)。
    /// 暗黙の begin (rescue/ensure 文脈で挿入) ではなく **キーワード明示**
    /// 形式のみ。
    Kwbegin(NodeList),
    /// `::Foo` の `::` 部分（top-level namespace root、`ConstantBaseNode`）。
    /// payload なしの marker。
    Cbase,
    /// `/.../[imxo]*` の flag 部分（`RegularExpressionOptionsNode`）。
    /// parser-gem は `(regopt :i :m)` と個別 sym で表現するが、Murphy は
    /// [`NodeKind::Regexp`] の `opts: Symbol` 表現と統一して flag 文字列
    /// (`"im"` 等) を interned `Symbol` 1 つで保持する。順序保持・順序非
    /// 依存どちらの解釈でも扱える、最も alloc-cheap な形。
    Regopt(Symbol),
    /// `1r` の rational literal（`RationalNode`）。payload は raw text を
    /// interned した string id。
    Rational(StringId),
    /// `1i` の complex literal（`ImaginaryNode`）。同上。
    Complex(StringId),
    /// `not foo` キーワード（`! foo` と AST 上は別表現）。
    Not(NodeId),
    /// `retry` キーワード。payload なし。
    Retry,
    /// `redo` キーワード。payload なし。
    Redo,
    /// `{ _1 + _2 }` の numbered-parameter block (`NumberedParametersNode`)。
    /// `send` は block の receiver、`max_n` は使用された最大 `_N` 番号、
    /// `body` は block 本体。
    Numblock {
        send: NodeId,
        max_n: u8,
        body: OptNodeId,
    },
    /// 単一引数 proc の暗黙のラップ (`procarg0` per AST_FORMAT.md)。
    Procarg0(NodeList),
    /// `def foo(...)` の forwarding arg parameter (declarer side)。
    ForwardArgs,
    /// `bar(...)` の forwarding arg passing (caller side)。
    ForwardedArgs,

    // ── MID-priority NodeKindTag extensions (murphy-o57f, parent
    // murphy-xvjv). Ruby 3 pattern matching family + `it` block.
    // Parser-only: subject-side translate support lands per cop as needed.
    /// `case expr; in pat; ...; end`（`CaseMatchNode`）。
    CaseMatch {
        subject: NodeId,
        in_patterns: NodeList,
        else_body: OptNodeId,
    },
    /// `in <pattern> [if|unless guard]; body` 一節（`InNode`）。
    /// `guard` は parser-gem の `if-guard` / `unless-guard` 個別 wrap を本
    /// サブセットでは導入せず、`OptNodeId` で guard 式を直接保持する。
    /// 互換 cop で if/unless 区別が必要になったら個別 variant を追加。
    InPattern {
        pattern: NodeId,
        guard: OptNodeId,
        body: OptNodeId,
    },
    /// `[a, b, c]` の array pattern (parser-gem `array-pattern`)。
    ArrayPattern(NodeList),
    /// `{a:, b:}` の hash pattern (parser-gem `hash-pattern`)。
    HashPattern(NodeList),
    /// `match_var` — pattern 内で名前束縛する identifier (`in [x, y]` の `x`/`y`)。
    MatchVar(Symbol),
    /// `it { ... }` block (Ruby 3.4+; parser-gem `itblock`)。`send` は block
    /// の receiver call、`body` は block 本体。`:it` marker は variant 自身。
    Itblock {
        send: NodeId,
        body: OptNodeId,
    },

    // ── LOW-priority NodeKindTag extensions (murphy-s4b4, parent
    // murphy-xvjv). Niche / legacy parser-gem node kinds — used only by a
    // handful of cops (Style/Alias, Style/SpecialGlobalVars 等). Parser-only;
    // subject-side translate support lands per cop as actually needed.
    /// `alias new_name old_name`（`AliasNode`）。
    /// `new_name` / `old_name` は `Sym` または `Gvar`。
    Alias {
        new_name: NodeId,
        old_name: NodeId,
    },
    /// `undef name, ...`（`UndefNode`）。children は `Sym` の並び。
    Undef(NodeList),
    /// `BEGIN { ... }`（`PreExeNode`）。body は単一 NodeId、空ブロック想定で
    /// `OptNodeId`。
    Preexe(OptNodeId),
    /// `END { ... }`（`PostExeNode`）。同上。
    Postexe(OptNodeId),
    /// `$~`、`$&`、`$\``、`$\` などの正規表現マッチ系 special variable
    /// (`BackReferenceReadNode`)。payload は variable 名 (`~`、`&` 等) の
    /// Symbol。
    BackRef(Symbol),
    /// `$1`、`$2`、…の正規表現キャプチャ参照 (`NumberedReferenceReadNode`)。
    /// payload は 1-based index。
    NthRef(u32),
    /// `-> (a; x) { ... }` の shadow arg `x`（`ShadowargNode`）。payload は
    /// shadowed variable 名。
    Shadowarg(Symbol),
    /// `**nil` (kwarg suppression、`NoKeywordsParameterNode`)。marker。
    Kwnilarg,
    /// `&nil` (block suppression、`BlocksArgumentSuppressionNode`)。marker。
    Blocknilarg,

    // ── murphy-jw5t pattern-match lowering extensions (tags 101, 102) ────
    /// `[*, x, *]` の find pattern (parser-gem `find_pattern`)。要素は
    /// requireds + 先頭/末尾の SplatNode (`MatchRest`) を含む NodeList。
    FindPattern(NodeList),
    /// `a | b` の alternation pattern (parser-gem `match_alt`)。
    MatchAlt {
        left: NodeId,
        right: NodeId,
    },
    // ── murphy-j1j2 PM-B pattern-match array/hash extensions (tags 103-105) ─
    /// `*rest` または bare `*` in array pattern (parser-gem `match_rest`)。
    /// inner は `Some(MatchVar)` (named) or `None` (bare `*`).
    MatchRest(OptNodeId),
    /// `**nil` in hash pattern — no-other-keys constraint
    /// (parser-gem `match_nil_pattern`)。marker。
    MatchNilPattern,
    /// `[a, b,]` — array pattern with trailing comma
    /// (parser-gem `array_pattern_with_tail`)。children はコンマ前の要素。
    ArrayPatternWithTail(NodeList),

    // ── murphy-j1j2 PM-C one-liner pattern matching (tags 106, 107) ──────
    /// `expr in pat` — boolean one-liner pattern match (Ruby 3.0+).
    /// prism: `MatchPredicateNode`. parser-gem: `match_pattern_p`.
    /// `value` は左辺 (matchable expression)、`pattern` は右辺パターン。
    MatchPatternP {
        value: NodeId,
        pattern: NodeId,
    },
    /// `expr => pat` — assignment-form one-liner pattern match (Ruby 3.0+).
    /// prism: `MatchRequiredNode`. parser-gem: `match_pattern`.
    /// `value` は左辺 (matchable expression)、`pattern` は右辺パターン。
    MatchPattern {
        value: NodeId,
        pattern: NodeId,
    },
    // ── murphy-j1j2 PM-D advanced patterns (tags 108, 109) ───────────────
    /// `pat => name` — capture pattern inside pattern matching (Ruby 3.0+).
    /// prism: `CapturePatternNode`. parser-gem: `match_as`.
    /// `value` は左辺パターン、`name` は `MatchVar` (束縛対象の local var)。
    MatchAs {
        value: NodeId,
        name: NodeId,
    },
    /// `Some(x)` — deconstruct-style pattern with a constant prefix.
    /// prism: `ArrayPatternNode`/`HashPatternNode`/`FindPatternNode` の
    /// `constant` フィールドが存在する場合に包む wrapper。
    /// parser-gem: `const_pattern`. `const_` は定数ノード、`pattern` は
    /// 内側の array/hash/find_pattern ノード。
    ConstPattern {
        const_: NodeId,
        pattern: NodeId,
    },

    // ── murphy-j1j2 PM-E pin & guard (tags 110, 111, 112) ────────────────
    /// `^x` — pin operator in pattern matching (Ruby 3.0+).
    /// prism: `PinnedVariableNode` (lvar/ivar/cvar/gvar) または
    /// `PinnedExpressionNode` (arbitrary expr in parentheses `^(expr)`).
    /// parser-gem: `pin`. inner は pin 対象の変数/式ノード。
    Pin(NodeId),
    /// `if cond` — guard clause attached to an `in` arm (Ruby 3.0+).
    /// prism: `IfNode` が `InNode` の pattern slot に挿入される形で出現。
    /// parser-gem: `if_guard`. inner は guard 条件式。
    IfGuard(NodeId),
    /// `unless cond` — negated guard clause attached to an `in` arm (Ruby 3.0+).
    /// prism: `UnlessNode` が `InNode` の pattern slot に挿入される形で出現。
    /// parser-gem: `unless_guard`. inner は guard 条件式。
    UnlessGuard(NodeId),
    /// `/(?<name>...)/ =~ value` — regexp named captures that implicitly bind
    /// local variables. `call` is the `=~` send; `targets` are value-less
    /// `Lvasgn` nodes for each named capture.
    MatchWithLvasgn {
        call: NodeId,
        targets: NodeList,
    },
}

/// A source comment, stored outside the node tree.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Comment {
    pub range: Range,
    pub kind: CommentKind,
}

/// Whether a comment is a `#` line comment or a `=begin`/`=end` block.
// A fieldless enum: `#[repr(u8)]` alone (not `#[repr(C, u8)]`, which the
// compiler rejects as a conflicting hint for a C-like enum) pins a stable
// `u8` discriminant.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommentKind {
    Inline,
    Block,
}

/// A structured file-level magic comment, stored outside the node tree.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MagicComment {
    /// Full source range for the shebang or comment line, excluding newline.
    pub range: Range,
    /// Source range for the magic-comment key, or [`Range::ZERO`] for shebang.
    pub key_range: Range,
    /// Source range for the magic-comment value, or [`Range::ZERO`] for shebang.
    pub value_range: Range,
    pub kind: MagicCommentKind,
    /// `1` for true `frozen_string_literal`, `0` otherwise.
    pub value_bool: u8,
}

/// The structured magic comments Murphy exposes to cops.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MagicCommentKind {
    Shebang,
    FrozenStringLiteral,
    Encoding,
}

/// The owned source text and path for one file. All [`Range`] values index
/// into `text` as byte offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBuffer {
    pub text: String,
    pub path: std::path::PathBuf,
}

impl NodeKind {
    /// This variant's `u8` discriminant (declaration order, frozen — ADR
    /// 0037). Exhaustive `match`: a new variant breaks compilation here.
    pub fn tag(&self) -> crate::NodeKindTag {
        let t: u8 = match self {
            NodeKind::Error => 0,
            NodeKind::Nil => 1,
            NodeKind::True_ => 2,
            NodeKind::False_ => 3,
            NodeKind::SelfExpr => 4,
            NodeKind::Int(_) => 5,
            NodeKind::Float(_) => 6,
            NodeKind::Str(_) => 7,
            NodeKind::Sym(_) => 8,
            NodeKind::Lvar(_) => 9,
            NodeKind::Ivar(_) => 10,
            NodeKind::Cvar(_) => 11,
            NodeKind::Gvar(_) => 12,
            NodeKind::Const { .. } => 13,
            NodeKind::Lvasgn { .. } => 14,
            NodeKind::Ivasgn { .. } => 15,
            NodeKind::Casgn { .. } => 16,
            NodeKind::Send { .. } => 17,
            NodeKind::Csend { .. } => 18,
            NodeKind::Block { .. } => 19,
            NodeKind::BlockPass(_) => 20,
            NodeKind::Splat(_) => 21,
            NodeKind::Array(_) => 22,
            NodeKind::Hash(_) => 23,
            NodeKind::Pair { .. } => 24,
            NodeKind::If { .. } => 25,
            NodeKind::Case { .. } => 26,
            NodeKind::When { .. } => 27,
            NodeKind::Begin(_) => 28,
            NodeKind::Return(_) => 29,
            NodeKind::And { .. } => 30,
            NodeKind::Or { .. } => 31,
            NodeKind::Def { .. } => 32,
            NodeKind::Class { .. } => 33,
            NodeKind::Module { .. } => 34,
            NodeKind::Args(_) => 35,
            NodeKind::Arg(_) => 36,
            NodeKind::Unknown => 37,
            NodeKind::Gvasgn { .. } => 38,
            NodeKind::Cvasgn { .. } => 39,
            NodeKind::Optarg { .. } => 40,
            NodeKind::Restarg(_) => 41,
            NodeKind::Kwarg(_) => 42,
            NodeKind::Kwoptarg { .. } => 43,
            NodeKind::Kwrestarg(_) => 44,
            NodeKind::Blockarg(_) => 45,
            NodeKind::Kwsplat(_) => 46,
            NodeKind::While { .. } => 47,
            NodeKind::Until { .. } => 48,
            NodeKind::RangeExpr { .. } => 49,
            NodeKind::Sclass { .. } => 50,
            NodeKind::Break(_) => 51,
            NodeKind::Next(_) => 52,
            NodeKind::Yield(_) => 53,
            NodeKind::Super(_) => 54,
            NodeKind::Zsuper => 55,
            NodeKind::Defined(_) => 56,
            NodeKind::Rescue { .. } => 57,
            NodeKind::Resbody { .. } => 58,
            NodeKind::Ensure { .. } => 59,
            NodeKind::OpAsgn { .. } => 60,
            NodeKind::OrAsgn { .. } => 61,
            NodeKind::AndAsgn { .. } => 62,
            NodeKind::Dstr(_) => 63,
            NodeKind::Dsym(_) => 64,
            NodeKind::Xstr(_) => 65,
            NodeKind::Regexp { .. } => 66,
            NodeKind::Masgn { .. } => 67,
            NodeKind::Mlhs(_) => 68,
            // murphy-w5ba HIGH-priority extensions
            NodeKind::For { .. } => 69,
            NodeKind::Lambda => 70,
            NodeKind::Defs { .. } => 71,
            NodeKind::Index { .. } => 72,
            NodeKind::IndexAsgn { .. } => 73,
            NodeKind::Kwbegin(_) => 74,
            NodeKind::Cbase => 75,
            NodeKind::Regopt(_) => 76,
            NodeKind::Rational(_) => 77,
            NodeKind::Complex(_) => 78,
            NodeKind::Not(_) => 79,
            NodeKind::Retry => 80,
            NodeKind::Redo => 81,
            NodeKind::Numblock { .. } => 82,
            NodeKind::Procarg0(_) => 83,
            NodeKind::ForwardArgs => 84,
            NodeKind::ForwardedArgs => 85,
            // murphy-o57f MID-priority extensions
            NodeKind::CaseMatch { .. } => 86,
            NodeKind::InPattern { .. } => 87,
            NodeKind::ArrayPattern(_) => 88,
            NodeKind::HashPattern(_) => 89,
            NodeKind::MatchVar(_) => 90,
            NodeKind::Itblock { .. } => 91,
            // murphy-s4b4 LOW-priority extensions
            NodeKind::Alias { .. } => 92,
            NodeKind::Undef(_) => 93,
            NodeKind::Preexe(_) => 94,
            NodeKind::Postexe(_) => 95,
            NodeKind::BackRef(_) => 96,
            NodeKind::NthRef(_) => 97,
            NodeKind::Shadowarg(_) => 98,
            NodeKind::Kwnilarg => 99,
            NodeKind::Blocknilarg => 100,
            // murphy-jw5t pattern-match lowering extensions
            NodeKind::FindPattern(_) => 101,
            NodeKind::MatchAlt { .. } => 102,
            // murphy-j1j2 PM-B pattern-match array/hash extensions
            NodeKind::MatchRest(_) => 103,
            NodeKind::MatchNilPattern => 104,
            NodeKind::ArrayPatternWithTail(_) => 105,
            // murphy-j1j2 PM-C one-liner pattern matching
            NodeKind::MatchPatternP { .. } => 106,
            NodeKind::MatchPattern { .. } => 107,
            NodeKind::MatchWithLvasgn { .. } => 113,
            // murphy-j1j2 PM-D advanced patterns
            NodeKind::MatchAs { .. } => 108,
            NodeKind::ConstPattern { .. } => 109,
            // murphy-j1j2 PM-E pin & guard
            NodeKind::Pin(_) => 110,
            NodeKind::IfGuard(_) => 111,
            NodeKind::UnlessGuard(_) => 112,
        };
        crate::NodeKindTag(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opt_node_id_round_trips() {
        assert_eq!(OptNodeId::NONE.get(), None);
        assert!(OptNodeId::NONE.is_none());
        let some = OptNodeId::some(NodeId(7));
        assert_eq!(some.get(), Some(NodeId(7)));
        assert!(!some.is_none());
        assert_eq!(OptNodeId::from(Some(NodeId(3))).get(), Some(NodeId(3)));
        assert_eq!(OptNodeId::from(None).get(), None);
        // NodeId(0) is the typical first-pushed arena node — it must not be
        // confused with the `None` sentinel. Also exercise the value just
        // below the sentinel.
        assert_eq!(OptNodeId::some(NodeId(0)).get(), Some(NodeId(0)));
        assert!(!OptNodeId::some(NodeId(0)).is_none());
        assert_eq!(
            OptNodeId::some(NodeId(u32::MAX - 1)).get(),
            Some(NodeId(u32::MAX - 1))
        );
    }

    #[test]
    fn node_list_empty_is_zero_len() {
        assert_eq!(NodeList::EMPTY.len, 0);
    }

    #[test]
    fn layout_invariants() {
        use std::mem::{align_of, size_of};

        // 4-byte handles.
        assert_eq!(size_of::<NodeId>(), 4);
        assert_eq!(size_of::<OptNodeId>(), 4);
        assert_eq!(size_of::<Symbol>(), 4);
        assert_eq!(size_of::<StringId>(), 4);
        // 8-byte side-table refs.
        assert_eq!(size_of::<Range>(), 8);
        assert_eq!(size_of::<NodeList>(), 8);

        // AstNode is a fixed-size POD node, small enough for a flat arena.
        assert!(size_of::<AstNode>() <= 48, "AstNode unexpectedly large");
        assert_eq!(align_of::<AstNode>(), 8, "i64 payload forces 8-byte align");

        // NodeKind carries the largest payload but stays compact.
        assert!(size_of::<NodeKind>() <= 32);
    }

    #[test]
    fn node_kind_is_copy() {
        // A POD enum: cheap to copy, no heap, no pointers.
        let k = NodeKind::Int(42);
        let copy = k;
        assert_eq!(k, copy);
    }
}
