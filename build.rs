// Windows: embed the icon and version info into swapbar.exe.
// macOS bundles these in Swapbar.app instead (scripts/bundle.sh), so nothing happens there.

fn main() {
    #[cfg(target_os = "windows")]
    windows_resources();
}

#[cfg(target_os = "windows")]
fn windows_resources() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    for path in ["assets/windows/swapbar.rc", "assets/AppIcon.ico"] {
        println!("cargo:rerun-if-changed={path}");
    }
    let version = env!("CARGO_PKG_VERSION");
    let parts: Vec<&str> = version
        .split(['-', '+'])
        .next()
        .unwrap_or("0")
        .split('.')
        .collect();
    let part = |i: usize| parts.get(i).copied().unwrap_or("0");
    let defines = [
        format!("SWAPBAR_VERSION=\"{version}\""),
        format!(
            "SWAPBAR_VERSION_PARTS={},{},{},0",
            part(0),
            part(1),
            part(2)
        ),
    ];
    embed_resource::compile("assets/windows/swapbar.rc", defines)
        .manifest_optional()
        .unwrap();
}
