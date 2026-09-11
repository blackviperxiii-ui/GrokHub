fn main() {
    let ico = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/windows/grokhub.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    if std::env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico.to_str().expect("grokhub.ico path"));
    res.set("ProductName", "GrokHub");
    res.set("FileDescription", "GrokHub");
    res.compile().expect("embed grokhub.ico into grokhub.exe");
}
