use image::{DynamicImage, ImageBuffer, Rgb, RgbImage, codecs::jpeg::JpegEncoder};
use std::{error::Error, fs::File, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let mut photo: RgbImage = ImageBuffer::new(1024, 640);
    for (x, y, pixel) in photo.enumerate_pixels_mut() {
        let fy = y as f32 / 640.0;
        let sky = [
            (32.0 + 86.0 * fy) as u8,
            (112.0 + 96.0 * fy) as u8,
            (184.0 + 48.0 * fy) as u8,
        ];
        let ridge_back = (315.0 + (x as f32 / 73.0).sin() * 34.0 - (x as f32 / 39.0).cos() * 16.0)
            .round() as i32;
        let ridge_front = (423.0 + (x as f32 / 91.0).sin() * 34.0 + (x as f32 / 47.0).cos() * 17.0)
            .round() as i32;
        let color = if (x as i32 - 770).pow(2) + (y as i32 - 145).pow(2) < 62_i32.pow(2) {
            [255, 218, 116]
        } else if y > 535 {
            [43, 112, 151]
        } else if (y as i32) > ridge_front {
            [33, 83, 64]
        } else if (y as i32) > ridge_back {
            [70, 112, 105]
        } else {
            sky
        };
        *pixel = Rgb(color);
    }
    // A small red cabin and its bright roof make the scene recognizable even
    // when reduced to the demo thumbnail size.
    for y in 388..468 {
        for x in 175..286 {
            photo.put_pixel(x, y, Rgb([190, 74, 51]));
        }
    }
    for y in 354..405 {
        let inset = (y - 354) * 2;
        for x in (175 + inset)..(286 - inset) {
            photo.put_pixel(x, y, Rgb([78, 48, 44]));
        }
    }
    for y in 420..468 {
        for x in 219..244 {
            photo.put_pixel(x, y, Rgb([246, 206, 123]));
        }
    }
    for y in [557, 568, 586] {
        let start = 620 + (y % 3) * 17;
        for x in start..(start + 145) {
            photo.put_pixel(x, y, Rgb([115, 180, 195]));
        }
    }

    write_jpeg(&root.join("demo-001.jpg"), &photo, 90)?;
    let thumbnail =
        image::imageops::resize(&photo, 400, 250, image::imageops::FilterType::Triangle);
    write_jpeg(&root.join("demo-001-thumbnail.jpg"), &thumbnail, 82)?;
    Ok(())
}

fn write_jpeg(path: &std::path::Path, image: &RgbImage, quality: u8) -> Result<(), Box<dyn Error>> {
    let file = File::create(path)?;
    JpegEncoder::new_with_quality(file, quality)
        .encode_image(&DynamicImage::ImageRgb8(image.clone()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use image::GenericImageView;

    #[test]
    fn committed_jpegs_decode_with_expected_dimensions_and_thumbnail_limit() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let source = image::open(root.join("demo-001.jpg")).expect("source JPEG decodes");
        let thumbnail =
            image::open(root.join("demo-001-thumbnail.jpg")).expect("thumbnail JPEG decodes");
        assert_eq!(source.dimensions(), (1024, 640));
        assert_eq!(thumbnail.dimensions(), (400, 250));
        assert!(thumbnail.width().max(thumbnail.height()) <= 2048);
        assert_ne!(
            std::fs::read(root.join("demo-001.jpg")).unwrap(),
            std::fs::read(root.join("demo-001-thumbnail.jpg")).unwrap()
        );
    }
}
