//! The desktop app's version is the Android app's (procraft.version in android/gradle.properties,
//! the one number a release is tagged with), given to the code as PROCRAFT_VERSION. The Windows
//! .exe carries it with the app's icon (assets/app-icon/procraft.ico, made by
//! tools/app-icon/make_icon.py) and its name, for Explorer, the task bar and shortcuts.

const PROPERTIES: &str = "../android/gradle.properties";

fn main() {
    println!("cargo:rerun-if-changed={PROPERTIES}");
    println!("cargo:rerun-if-changed=../assets/app-icon/procraft.ico");
    let version = std::fs::read_to_string(PROPERTIES)
        .ok()
        .and_then(|text| text.lines().find_map(|l| l.trim().strip_prefix("procraft.version=").map(str::to_owned)))
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_default());
    println!("cargo:rustc-env=PROCRAFT_VERSION={version}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // the numbers Windows compares (1.2.3 of 1.2.3-beta), each in 16 bits
        let numbers = version.split(['-', '+']).next().unwrap_or_default();
        let packed = numbers
            .split('.')
            .take(3)
            .chain(std::iter::repeat("0"))
            .take(4)
            .fold(0u64, |packed, part| (packed << 16) | part.parse::<u64>().unwrap_or(0).min(0xFFFF));
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("../assets/app-icon/procraft.ico")
            .set("ProductName", "ProCraft")
            .set("FileDescription", "ProCraft")
            .set("ProductVersion", &version)
            .set("FileVersion", &version)
            .set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed)
            .set_version_info(winresource::VersionInfo::FILEVERSION, packed);
        if let Err(e) = resource.compile() {
            println!("cargo:warning=the app's icon wasn't put in the .exe: {e}");
        }
    }
}
