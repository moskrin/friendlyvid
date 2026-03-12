#[test]
fn test_font_kit_loading() {
    let source = font_kit::source::SystemSource::new();
    let families = source.all_families().unwrap();
    println!("Total system fonts: {}", families.len());
    for i in 0..5.min(families.len()) {
        println!("  [{}] {}", i, families[i]);
    }
    println!("---");
    for name in ["Sans", "sans-serif", "DejaVu Sans", "Liberation Sans", "Noto Sans", "Monospace"] {
        match source.select_family_by_name(name) {
            Ok(fam) => {
                println!("OK '{}': {} variant(s)", name, fam.fonts().len());
                for h in fam.fonts() {
                    if let Ok(f) = h.load() {
                        let p = f.properties();
                        let has = f.copy_font_data().is_some();
                        println!("    w={:.0} s={:?} data={}", p.weight.0, p.style, has);
                    }
                }
            }
            Err(e) => println!("FAIL '{}': {}", name, e),
        }
    }
}
