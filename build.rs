//! Embeds the app icon and version info into the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    // Checked at run time rather than with #[cfg], so cross-compiling for Windows works too.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("ProductName", "Core Board")
            .set("FileDescription", "Core Board")
            .set("CompanyName", "TaikoGames");
        if let Err(err) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {err}");
        }
    }
}
