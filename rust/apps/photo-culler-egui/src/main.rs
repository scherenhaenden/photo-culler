use photo_culler_core::Frontend;

#[derive(Debug, Default)]
struct NativeAppState {
    selected_photo_id: Option<String>,
    catalog_count: usize,
}

fn main() {
    let state = NativeAppState::default();
    println!(
        "egui/wgpu bootstrap ({}%): selected={:?}, catalog_count={}",
        Frontend::EguiWgpu.readiness(),
        state.selected_photo_id,
        state.catalog_count
    );
}
