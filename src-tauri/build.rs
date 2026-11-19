fn main() {
    // MinGW otherwise auto-exports dependency symbols from the desktop DLL and
    // can exceed PE's 65,535-export limit. Mobile entry points are unaffected.
    if std::env::var("TARGET").is_ok_and(|t| t.ends_with("windows-gnu")) {
        println!("cargo:rustc-link-arg-cdylib=-Wl,--exclude-all-symbols");
    }
    tauri_build::build()
}
