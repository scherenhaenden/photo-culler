use photo_culler_core::Frontend;

fn main() {
    println!(
        "Tauri/WebGL bootstrap: {}% ready; frontend prototype is in web/index.html",
        Frontend::TauriWebGl.readiness()
    );
}
