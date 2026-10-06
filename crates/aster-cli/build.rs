//! On macOS the `aster` binary carries an Info.plist, so permission prompts for
//! the microphone and speech recognition can say what Aster wants them for.

fn main() {
    println!("cargo:rerun-if-changed=macos/Info.plist");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
        println!(
            "cargo:rustc-link-arg-bin=aster=-Wl,-sectcreate,__TEXT,__info_plist,{dir}/macos/Info.plist"
        );
    }
}
