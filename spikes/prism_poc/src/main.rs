// Spike 0.1 PoC: prism Rust binding evaluation.
//
// Goal: parse a `.rb` string to an AST and walk it from Rust, visiting every
// call node and printing its method name + byte range. The byte range is the
// load-bearing concern: Murphy keys offenses on {start_offset, end_offset}.
//
// This is throwaway spike code. It is NOT carried into crates/.

use ruby_prism::{parse, Visit};

/// A call site we found, with the byte range of its *message* (selector) token
/// — that is the range a cop like NoReceiverPuts would flag.
#[derive(Debug)]
struct CallHit {
    name: String,
    has_receiver: bool,
    // Byte range of the whole call node.
    node_start: usize,
    node_end: usize,
    // Byte range of just the method-name token, when prism exposes it.
    msg_start: Option<usize>,
    msg_end: Option<usize>,
}

struct CallCollector {
    hits: Vec<CallHit>,
}

impl<'pr> Visit<'pr> for CallCollector {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        let loc = node.location();
        let name = String::from_utf8_lossy(node.name().as_slice()).into_owned();

        let (msg_start, msg_end) = match node.message_loc() {
            Some(m) => (Some(m.start_offset()), Some(m.end_offset())),
            None => (None, None),
        };

        self.hits.push(CallHit {
            name,
            has_receiver: node.receiver().is_some(),
            node_start: loc.start_offset(),
            node_end: loc.end_offset(),
            msg_start,
            msg_end,
        });

        // Recurse so nested calls (e.g. `foo.bar(baz)`) are all visited.
        ruby_prism::visit_call_node(self, node);
    }
}

fn main() {
    // Hand-checked snippet. Byte offsets verified by hand below.
    //
    //  bytes: 0:p 1:u 2:t 3:s 4:(space) 5:" 6:h 7:i 8:" 9:\n
    //         10:l 11:o 12:g ... "logger.info(x)" then "\n" then "obj.foo"
    let src = "puts \"hi\"\nlogger.info(x)\nobj.foo\n";

    let result = parse(src.as_bytes());

    // Robustness criterion: syntax errors must surface structurally, not panic.
    let errors: Vec<_> = result.errors().collect();
    if !errors.is_empty() {
        eprintln!("parse errors ({}):", errors.len());
        for e in &errors {
            let l = e.location();
            eprintln!(
                "  [{}..{}] {}",
                l.start_offset(),
                l.end_offset(),
                String::from_utf8_lossy(e.message().as_bytes())
            );
        }
    }

    let mut collector = CallCollector { hits: Vec::new() };
    collector.visit(&result.node());

    println!("source ({} bytes): {:?}", src.len(), src);
    println!("call nodes found: {}", collector.hits.len());
    for h in &collector.hits {
        let slice = |a: usize, b: usize| &src[a..b];
        println!(
            "  name={:<8} receiver={:<5} node[{:>2}..{:>2}]={:?} msg[{:?}..{:?}]={:?}",
            h.name,
            h.has_receiver,
            h.node_start,
            h.node_end,
            slice(h.node_start, h.node_end),
            h.msg_start,
            h.msg_end,
            match (h.msg_start, h.msg_end) {
                (Some(a), Some(b)) => Some(slice(a, b)),
                _ => None,
            },
        );
    }

    // ---- Assertions: byte ranges must match the hand-checked snippet ----
    // `puts` at bytes 0..4, no receiver.
    let puts = collector
        .hits
        .iter()
        .find(|h| h.name == "puts")
        .expect("expected a `puts` call");
    assert_eq!(puts.has_receiver, false, "puts must have no receiver");
    assert_eq!((puts.msg_start, puts.msg_end), (Some(0), Some(4)));
    assert_eq!(&src[0..4], "puts");

    // `info` is `logger.info` — has a receiver; message token is `info`.
    let info = collector
        .hits
        .iter()
        .find(|h| h.name == "info")
        .expect("expected an `info` call");
    assert_eq!(info.has_receiver, true, "logger.info must have a receiver");
    let (ms, me) = (info.msg_start.unwrap(), info.msg_end.unwrap());
    assert_eq!(&src[ms..me], "info");

    // `foo` is `obj.foo` — has a receiver.
    let foo = collector
        .hits
        .iter()
        .find(|h| h.name == "foo")
        .expect("expected a `foo` call");
    assert_eq!(foo.has_receiver, true);
    let (fs, fe) = (foo.msg_start.unwrap(), foo.msg_end.unwrap());
    assert_eq!(&src[fs..fe], "foo");

    // ---- Multibyte UTF-8 robustness (Japanese comment + string literal) ----
    // Ruby source routinely contains non-ASCII. prism returns *byte* offsets
    // into source.as_bytes(); cop code must slice by byte index, never char.
    let mb = "# あいさつ\nputs \"こんにちは\"\n";
    let mr = parse(mb.as_bytes());
    assert!(mr.errors().collect::<Vec<_>>().is_empty(), "mb must parse clean");
    let mut mc = CallCollector { hits: Vec::new() };
    mc.visit(&mr.node());
    let mputs = mc.hits.iter().find(|h| h.name == "puts").expect("puts");
    let (a, b) = (mputs.msg_start.unwrap(), mputs.msg_end.unwrap());
    // "# あいさつ\n" = 1 + (3*4 bytes? no: あ=3 bytes each) ... do not hand-count;
    // assert the contract instead: the byte slice must equal "puts" and the
    // offset must be a valid UTF-8 char boundary so &str slicing is panic-free.
    assert!(mb.is_char_boundary(a) && mb.is_char_boundary(b), "offsets must be char boundaries");
    assert_eq!(&mb[a..b], "puts", "byte-indexed slice must recover the selector");
    println!(
        "multibyte: `puts` msg bytes [{}..{}] = {:?}  (src byte len {})",
        a, b, &mb[a..b], mb.len()
    );

    println!("\nALL BYTE-RANGE ASSERTIONS PASSED");
}
