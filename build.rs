fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/icon.png");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "PRC Editor");
        res.set("FileDescription", "PRC Editor");
        if let Err(err) = res.compile() {
            println!("cargo:warning=Failed to embed Windows icon: {err}");
        }
    }
}
