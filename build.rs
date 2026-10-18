//! Windows only: puts the app icon inside `silex-desktop.exe`, so the
//! Explorer, the Start menu and the list of installed apps show it (the
//! installer's shortcuts take their icon from the .exe). ADR 0005.
//! Other systems get their icon from the package (macOS .icns, Linux
//! hicolor PNGs) and the window icon from `app::window_icon`.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app-icon/silex.ico");
    // `cfg(windows)` is the build machine; the icon is only built natively.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("assets/app-icon/silex.ico")
            .set("ProductName", "Silex")
            .set("FileDescription", "Silex");
        // Only the app's .exe (not tests or examples). A missing icon must
        // fail the build, not ship unnoticed.
        resource.compile_for("silex-desktop")?;
    }
    Ok(())
}
