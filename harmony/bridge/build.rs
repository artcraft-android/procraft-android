fn main() {
    // Node-API's link setup, on OpenHarmony only: built for any other system the crate is just its
    // pen maths and their tests
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("ohos") {
        napi_build_ohos::setup();
    }
}
