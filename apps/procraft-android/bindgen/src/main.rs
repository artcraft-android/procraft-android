//! `uniffi-bindgen` for `procraft-android`: generates its bindings from the built library, e.g.
//! `cargo run -p procraft-android-bindgen -- generate libprocraft_android.so --language kotlin --out-dir out`
//! (`android/app/build.gradle.kts` runs it).

fn main() {
    uniffi::uniffi_bindgen_main()
}
