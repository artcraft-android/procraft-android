//! `uniffi-bindgen` for `procraft-core`: generates its bindings from the built library, e.g.
//! `cargo run -p procraft-bindgen -- generate libprocraft_core.so --language kotlin --out-dir out`
//! (`android/app/build.gradle.kts` runs it).

fn main() {
    uniffi::uniffi_bindgen_main()
}
