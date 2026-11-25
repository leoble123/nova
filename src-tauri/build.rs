fn main() {
    println!("cargo:rerun-if-changed=nova.manifest");
    let attrs = tauri_build::Attributes::new().windows_attributes(tauri_build::WindowsAttributes::new().app_manifest(include_str!("nova.manifest")));
    tauri_build::try_build(attrs).expect("failed to run tauri-build");
}
