fn main() {
    if std::env::var("TARGET").is_ok_and(|target| target.contains("apple-darwin")) {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        if let Ok(output) = std::process::Command::new("xcode-select")
            .arg("-p")
            .output()
        {
            if output.status.success() {
                let developer_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let swift_runtime = if developer_dir.ends_with("CommandLineTools") {
                    format!("{developer_dir}/usr/lib/swift-5.5/macosx")
                } else {
                    format!(
                        "{developer_dir}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx"
                    )
                };
                println!("cargo:rustc-link-arg=-Wl,-rpath,{swift_runtime}");
            }
        }
    }
    let mut attributes = tauri_build::Attributes::new();
    if std::env::var("TARGET").is_ok_and(|target| target.ends_with("windows-msvc")) {
        // Tauri's default resource only reaches app binaries, leaving library tests
        // without Common Controls v6: https://github.com/tauri-apps/tauri/issues/13419
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        let manifest =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
    tauri_build::try_build(attributes).expect("failed to run Tauri build script");
}
