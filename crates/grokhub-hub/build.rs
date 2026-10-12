fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set("ProductName", "GrokHub");
    res.set("FileDescription", "GrokHub Hub");
    res.compile()
        .expect("embed Windows version resources into grokhub-hub.exe");
}
