use super::*;
use std::{
    cell::RefCell,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

struct FakeDecoder {
    decoded: RgbImage,
}
impl Decoder for FakeDecoder {
    fn decode(&self, _source: &[u8]) -> Result<RgbImage, RenderError> {
        Ok(self.decoded.clone())
    }
}

#[derive(Default)]
struct FakeWriter(RefCell<Vec<(PathBuf, Vec<u8>)>>);
impl DerivedWriter for FakeWriter {
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), RenderError> {
        self.0.borrow_mut().push((path.to_owned(), bytes.to_vec()));
        Ok(())
    }
}

fn sha256(bytes: &[u8]) -> String {
    let mut child = Command::new("shasum")
        .args(["-a", "256"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .or_else(|_| {
            Command::new("sha256sum")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
        })
        .expect("shasum or sha256sum must be installed for the original integrity test");
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

fn golden() -> RgbImage {
    RgbImage {
        width: 2,
        height: 1,
        pixels: vec![20, 30, 40, 100, 120, 140],
    }
}

#[test]
fn golden_before_after_and_explicit_derivative_preserve_original_sha256() {
    let source = b"raw-source-fixture: immutable original bytes".to_vec();
    let before_hash = sha256(&source);
    let decoded = golden();
    let rendered = render_before_after(
        &FakeDecoder {
            decoded: decoded.clone(),
        },
        &source,
        Recipe {
            exposure_ev: 1.0,
            temperature_kelvin: 6500,
            tint: 0,
        },
    )
    .unwrap();
    assert_eq!(rendered.before, decoded);
    assert_eq!(rendered.after.width, 2);
    assert_eq!(rendered.after.height, 1);
    assert_eq!(rendered.after.pixels, vec![40, 60, 78, 200, 239, 255]);

    let writer = FakeWriter::default();
    write_derived(&writer, Path::new("derived.ppm"), &rendered.after).unwrap();
    let writes = writer.0.borrow();
    assert_eq!(writes.len(), 1);
    assert!(writes[0].1.starts_with(b"P6\n2 1\n255\n"));
    assert_eq!(sha256(&source), before_hash);
}

#[test]
fn pp3_suggestion_is_golden_and_does_not_require_a_source_write() {
    let pp3 = rawtherapee_pp3(Recipe {
        exposure_ev: -0.5,
        temperature_kelvin: 5200,
        tint: 4,
    })
    .unwrap();
    assert_eq!(
        pp3,
        "[Version]\nVersion=500\n\n[White Balance]\nSetting=Custom\nTemperature=5200\nGreen=4\n\n[Exposure]\nCompensation=-0.50\n"
    );
}

#[test]
fn warning_families_are_independent() {
    let report = warnings(MetricWarnings {
        focus_variance: 2.0,
        noise_level: 0.8,
        clipping_ratio: 0.1,
        recoverability: 0.1,
    });
    assert_eq!(report.focus, Some("low_focus_confidence"));
    assert_eq!(report.noise, Some("high_noise"));
    assert_eq!(report.clipping, Some("visible_clipping"));
    assert_eq!(report.recoverability, Some("low_recoverability"));
    let clear = warnings(MetricWarnings {
        focus_variance: 20.0,
        noise_level: 0.2,
        clipping_ratio: 0.0,
        recoverability: 0.9,
    });
    assert_eq!(
        clear,
        WarningReport {
            focus: None,
            noise: None,
            clipping: None,
            recoverability: None
        }
    );
}

#[test]
fn invalid_recipes_and_decoder_buffers_fail_closed() {
    assert!(matches!(
        rawtherapee_pp3(Recipe {
            exposure_ev: f32::NAN,
            ..Recipe::default()
        }),
        Err(RenderError::InvalidRecipe(_))
    ));
    let invalid = RgbImage {
        width: 1,
        height: 1,
        pixels: vec![1, 2],
    };
    assert!(matches!(
        render_before_after(
            &FakeDecoder { decoded: invalid },
            b"source",
            Recipe::default()
        ),
        Err(RenderError::InvalidDimensions)
    ));
}
