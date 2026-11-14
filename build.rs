fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("tolo.ico");
        res.set("ProductName", "Tolo");
        res.set("FileDescription", "Tolo");
        res.set("InternalName", "Tolo");
        res.set("OriginalFilename", "Tolo.exe");
        res.set("LegalCopyright", "Copyright © 2026 Yuvraj Rode");
        res.compile().expect("Failed to compile Windows resource with winres");
    }
}
