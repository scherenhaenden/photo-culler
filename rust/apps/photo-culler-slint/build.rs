fn main() {
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-env-changed=PHOTO_CULLER_SLINT_ENTRY");
    let entry = std::env::var("PHOTO_CULLER_SLINT_ENTRY")
        .unwrap_or_else(|_| "ui/demo_window.slint".to_owned());
    slint_build::compile(entry).expect("compile selected Photo Culler Slint entry");
}
