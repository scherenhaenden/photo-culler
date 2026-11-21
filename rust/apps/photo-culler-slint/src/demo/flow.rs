//! Convert the bundled read-only API response into the first gallery view.

use crate::{DemoWindow, demo::thumbnail_image};

pub fn render(
    window: &DemoWindow,
    health_json: &[u8],
    gallery_json: &[u8],
    thumbnail_jpeg: &[u8],
) -> Result<(), String> {
    let health: serde_json::Value = serde_json::from_slice(health_json)
        .map_err(|_| "The local service returned invalid health data.".to_owned())?;
    if health.get("status").and_then(serde_json::Value::as_str) != Some("ready") {
        return Err("The local Photo Culler service is not ready.".into());
    }

    let gallery: serde_json::Value = serde_json::from_slice(gallery_json)
        .map_err(|_| "The local service returned invalid gallery data.".to_owned())?;
    if gallery
        .get("gallery_id")
        .and_then(serde_json::Value::as_str)
        != Some("demo")
    {
        return Err("The local service returned an unexpected gallery.".into());
    }
    let items = gallery
        .get("items")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "The local service returned an invalid photo list.".to_owned())?;
    if items.is_empty() {
        window.set_photo_count(0);
        window.set_photo_index(0);
        window.set_navigation_label("0 photos".into());
        window.set_view_state(2);
        return Ok(());
    }

    let photo = &items[0];
    if photo.get("photo_id").and_then(serde_json::Value::as_str) != Some("demo-001") {
        return Err("The local service returned an unsupported fixture photo.".into());
    }
    let filename = photo
        .get("filename")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "The local service returned a photo without a filename.".to_owned())?;
    let image = thumbnail_image::decode_jpeg(thumbnail_jpeg, "image/jpeg")
        .map_err(|error| format!("The gallery thumbnail could not be decoded: {error:?}"))?;

    window.set_photo_name(filename.into());
    window.set_photo_thumbnail(image);
    window.set_photo_index(0);
    window.set_photo_count(1);
    window.set_navigation_label("Photo 1 of 1".into());
    window.set_view_state(1);
    Ok(())
}

pub fn show_error(window: &DemoWindow, message: impl Into<slint::SharedString>) {
    window.set_error_message(message.into());
    window.set_view_state(3);
}
