fn main() {
    // Windows takes the window, taskbar and file icons and the name in Task
    // Manager from the executable's resources.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../packaging/windows/request-eagle.ico")
            .set("FileDescription", "Request Eagle")
            .set("ProductName", "Request Eagle")
            .compile()
            .expect("Windows resources should compile");
    }
}
