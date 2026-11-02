//! The Windows .exe carries the app's icon (assets/app-icon/procraft.ico, made by
//! tools/app-icon/make_icon.py) and its name, for Explorer, the task bar and shortcuts.

fn main() {
    println!("cargo:rerun-if-changed=../assets/app-icon/procraft.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("../assets/app-icon/procraft.ico")
            .set("ProductName", "ProCraft")
            .set("FileDescription", "ProCraft");
        if let Err(e) = resource.compile() {
            println!("cargo:warning=the app's icon wasn't put in the .exe: {e}");
        }
    }
}
