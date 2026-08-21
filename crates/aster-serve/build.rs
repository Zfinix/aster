/// `include_dir!` embeds `ui/` at compile time but does not watch it, so a
/// rebuilt bundle would ship stale without this.
fn main() {
    println!("cargo:rerun-if-changed=ui");
}
