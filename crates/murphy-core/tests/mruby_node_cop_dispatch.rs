//! Integration test for murphy-9cr.24.1: an mruby user cop dispatched
//! through `run_cops` as a `PluginCopV1` value.
//!
//! Builds an arena AST programmatically, eval's a tiny `Murphy::Cop`
//! subclass inside a per-cop `MrubyState`, wraps it in `MrubyCopProxy`,
//! populates the thread-local proxy map, runs `dispatch::run_cops`, and
//! asserts that the cop's `on_send` hook emitted exactly one offense.

#![cfg(feature = "mruby-user-cops")]

use std::collections::HashMap;

use murphy_ast::{AstBuilder, NodeKind, NodeList, OptNodeId, Range as AstRange, Symbol};
use murphy_plugin_api::{NodeKindTag, PluginCopV1};

use murphy_core::dispatch::{OffenseSink, run_cops};
use murphy_core::{
    MrubyCopProxy, build_mruby_cop, current_mruby_proxies_drain, current_mruby_proxies_populate,
    with_current_mruby_proxies,
};

/// Build a one-node `Send` AST whose root is `puts` with no receiver and
/// no arguments — the dispatcher's per-kind index only needs a Send-tagged
/// node for the cop's `on_send` hook to fire once.
fn build_send_root_ast() -> murphy_ast::Ast {
    let mut b = AstBuilder::new("puts".to_string(), "t.rb");
    let puts_sym = b.intern_symbol("puts");
    let empty_args = b.push_list(&[]);
    let send = b.push(
        NodeKind::Send {
            receiver: OptNodeId::NONE,
            method: puts_sym,
            args: empty_args,
        },
        AstRange { start: 0, end: 4 },
    );
    b.finish(send)
}

#[test]
fn mruby_cop_on_send_hook_emits_one_offense() {
    let ast = build_send_root_ast();

    // The Send tag is structurally fixed — we use a zero-payload sentinel
    // Send to read its discriminant byte. (Symbol(0) / empty NodeList are
    // payload placeholders; the tag depends only on the variant.)
    let send_tag = NodeKindTag::of(&NodeKind::Send {
        receiver: OptNodeId::NONE,
        method: Symbol(0),
        args: NodeList { start: 0, len: 0 },
    });

    let cop_source = r#"
        class TestSendCop < Murphy::Cop
          def on_send(node)
            add_offense(node.range, message: "send hit")
          end
        end
    "#;

    let proxy = MrubyCopProxy::for_test(
        "Murphy/TestSend",
        "Tests on_send dispatch",
        cop_source,
        "TestSendCop",
        &[send_tag],
    )
    .expect("proxy construction must succeed");

    let mut proxies: HashMap<Vec<u8>, Box<MrubyCopProxy>> = HashMap::new();
    proxies.insert(b"Murphy/TestSend".to_vec(), Box::new(proxy));
    current_mruby_proxies_populate(proxies);

    // Build PluginCopV1 AFTER population so the kinds_ptr / name slices
    // reference each proxy's stable Box-owned memory.
    let plugin_cop: PluginCopV1 = with_current_mruby_proxies(|m| {
        build_mruby_cop(m.get(&b"Murphy/TestSend"[..]).expect("proxy present"))
    });

    let mut sink = OffenseSink::new("t.rb");
    run_cops(&ast, &[&plugin_cop], &mut sink);

    let drained = current_mruby_proxies_drain();
    drop(drained);

    let offenses = sink.into_offenses();
    assert_eq!(offenses.len(), 1, "exactly one Send-hook offense expected");
    assert_eq!(offenses[0].message, "send hit");
    assert_eq!(offenses[0].cop_name, "Murphy/TestSend");
}

/// Two `.rb` cops both hooking `on_send`. They emit in the order their
/// `PluginCopV1` slice is presented to `run_cops` — the dispatch loop's
/// outer iteration is per-cop (`crates/murphy-core/src/dispatch.rs`),
/// so cop A's offense lands first regardless of node walk order.
#[test]
fn multiple_mruby_cops_dispatch_in_deterministic_order() {
    let ast = build_send_root_ast();
    let send_tag = NodeKindTag::of(&NodeKind::Send {
        receiver: OptNodeId::NONE,
        method: Symbol(0),
        args: NodeList { start: 0, len: 0 },
    });

    let cop_a_src = r#"
        class CopAlpha < Murphy::Cop
          def on_send(node)
            add_offense(node.range, message: "alpha")
          end
        end
    "#;
    let cop_b_src = r#"
        class CopBravo < Murphy::Cop
          def on_send(node)
            add_offense(node.range, message: "bravo")
          end
        end
    "#;

    let proxy_a = MrubyCopProxy::for_test(
        "Murphy/CopAlpha",
        "Alpha",
        cop_a_src,
        "CopAlpha",
        &[send_tag],
    )
    .expect("alpha proxy");
    let proxy_b = MrubyCopProxy::for_test(
        "Murphy/CopBravo",
        "Bravo",
        cop_b_src,
        "CopBravo",
        &[send_tag],
    )
    .expect("bravo proxy");

    let mut proxies: HashMap<Vec<u8>, Box<MrubyCopProxy>> = HashMap::new();
    proxies.insert(b"Murphy/CopAlpha".to_vec(), Box::new(proxy_a));
    proxies.insert(b"Murphy/CopBravo".to_vec(), Box::new(proxy_b));
    current_mruby_proxies_populate(proxies);

    let (plugin_a, plugin_b): (PluginCopV1, PluginCopV1) = with_current_mruby_proxies(|m| {
        (
            build_mruby_cop(m.get(&b"Murphy/CopAlpha"[..]).unwrap()),
            build_mruby_cop(m.get(&b"Murphy/CopBravo"[..]).unwrap()),
        )
    });

    let mut sink = OffenseSink::new("t.rb");
    run_cops(&ast, &[&plugin_a, &plugin_b], &mut sink);
    drop(current_mruby_proxies_drain());

    let offenses = sink.into_offenses();
    assert_eq!(offenses.len(), 2, "both cops emit");
    assert_eq!(offenses[0].message, "alpha", "cop slice order is preserved");
    assert_eq!(offenses[1].message, "bravo");
}

/// Cop A's `on_send` raises; cop B's `on_send` must still run. This is
/// the per-cop fault isolation contract documented on
/// `dispatch::run_cops` and asserted by the parent §4 design.
#[test]
fn mruby_cop_raise_disables_only_that_cop() {
    let ast = build_send_root_ast();
    let send_tag = NodeKindTag::of(&NodeKind::Send {
        receiver: OptNodeId::NONE,
        method: Symbol(0),
        args: NodeList { start: 0, len: 0 },
    });

    let bad_src = r#"
        class RaisingCop < Murphy::Cop
          def on_send(node)
            raise "boom"
          end
        end
    "#;
    let good_src = r#"
        class SurvivingCop < Murphy::Cop
          def on_send(node)
            add_offense(node.range, message: "survived")
          end
        end
    "#;

    let bad = MrubyCopProxy::for_test(
        "Murphy/Raising",
        "Always raises",
        bad_src,
        "RaisingCop",
        &[send_tag],
    )
    .expect("bad proxy");
    let good = MrubyCopProxy::for_test(
        "Murphy/Surviving",
        "Survives",
        good_src,
        "SurvivingCop",
        &[send_tag],
    )
    .expect("good proxy");

    let mut proxies: HashMap<Vec<u8>, Box<MrubyCopProxy>> = HashMap::new();
    proxies.insert(b"Murphy/Raising".to_vec(), Box::new(bad));
    proxies.insert(b"Murphy/Surviving".to_vec(), Box::new(good));
    current_mruby_proxies_populate(proxies);

    let (bad_v1, good_v1): (PluginCopV1, PluginCopV1) = with_current_mruby_proxies(|m| {
        (
            build_mruby_cop(m.get(&b"Murphy/Raising"[..]).unwrap()),
            build_mruby_cop(m.get(&b"Murphy/Surviving"[..]).unwrap()),
        )
    });

    let mut sink = OffenseSink::new("t.rb");
    run_cops(&ast, &[&bad_v1, &good_v1], &mut sink);
    drop(current_mruby_proxies_drain());

    let offenses = sink.into_offenses();
    assert_eq!(
        offenses.len(),
        1,
        "raising cop emitted nothing; surviving cop still ran"
    );
    assert_eq!(offenses[0].message, "survived");
    assert_eq!(offenses[0].cop_name, "Murphy/Surviving");
}
