fn main() {
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=icon-256px.ico");
    println!("cargo:rerun-if-env-changed=REGUI_BUILD_TIME");

    // The CI appends the build timestamp; keep the same escape hatch as the
    // original Python build so the about page can show "Built at ...".
    if let Ok(build_time) = std::env::var("REGUI_BUILD_TIME") {
        println!("cargo:rustc-env=REGUI_BUILD_TIME={build_time}");
    }

    // Embed icon-256px.ico (plus version metadata) into the Windows
    // executable so Explorer, the taskbar and notifications show the app icon.
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("icon-256px.ico")
            .set("ProductName", "Real-ESRGAN GUI")
            .set("FileDescription", "Real-ESRGAN GUI")
            .set("OriginalFilename", "realesrgan-gui.exe")
            .compile()
            .expect("failed to compile the Windows resource file");
    }

    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config("ui/app.slint", config)
        .expect("failed to compile ui/app.slint");
}
