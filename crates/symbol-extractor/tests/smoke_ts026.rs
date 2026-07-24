#[test]
fn extracts_rust_symbols_on_tree_sitter_026() {
    let src = "pub fn alpha() {}\nstruct Beta { x: i32 }\npub fn gamma(n: usize) -> usize { n }\n";
    let syms = symbol_extractor::extract_symbols(src, "x.rs");
    let names: Vec<_> = syms
        .iter()
        .filter_map(|s| s.symbol_name.as_deref())
        .collect();
    assert!(
        !syms.is_empty(),
        "no symbols extracted — grammar broke on tree-sitter 0.26"
    );
    assert!(names.contains(&"alpha"), "got {names:?}");
    assert!(names.contains(&"gamma"), "got {names:?}");
}
