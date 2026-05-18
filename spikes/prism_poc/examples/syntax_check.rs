fn main() {
    let bad = "def (\n";
    let r = ruby_prism::parse(bad.as_bytes());
    let errs: Vec<_> = r.errors().collect();
    println!("errors={} (no panic)", errs.len());
    for e in errs { let l=e.location(); println!("  [{}..{}] {}", l.start_offset(), l.end_offset(), String::from_utf8_lossy(e.message().as_bytes())); }
}
