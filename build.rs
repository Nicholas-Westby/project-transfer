//! Embeds the Windows executable icon. Nothing happens on other targets.

fn main() {
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.compile().expect("could not embed the Windows icon");
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
}
