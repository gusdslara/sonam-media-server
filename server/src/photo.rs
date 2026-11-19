//! Photo details read from the file itself (EXIF): capture date, camera,
//! exposure and GPS location. Nothing is sent to external services.
use exif::{In, Tag, Value};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Default, Debug, PartialEq)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: Option<f64>,
}

#[derive(Serialize, Default, Debug)]
pub struct PhotoDetails {
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// "YYYY-MM-DDTHH:MM:SS" in the camera's local time, plus the offset when
    /// the camera recorded one (e.g. "-03:00").
    pub taken: Option<String>,
    pub taken_offset: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub aperture: Option<f64>,
    pub exposure: Option<String>,
    pub iso: Option<u32>,
    pub focal_length: Option<f64>,
    pub focal_length_35mm: Option<u32>,
    pub flash: Option<bool>,
    pub software: Option<String>,
    pub location: Option<Location>,
}

fn text(exif: &exif::Exif, tag: Tag) -> Option<String> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Ascii(parts) => {
            let s = parts
                .iter()
                .map(|p| String::from_utf8_lossy(p).trim().to_string())
                .find(|s| !s.is_empty())?;
            Some(s.chars().filter(|c| !c.is_control()).take(120).collect())
        }
        _ => None,
    }
}

fn rational(exif: &exif::Exif, tag: Tag) -> Option<f64> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Rational(v) => v.first().filter(|r| r.denom != 0).map(|r| r.to_f64()),
        Value::SRational(v) => v.first().filter(|r| r.denom != 0).map(|r| r.to_f64()),
        // Some editors write floating point instead of rationals.
        Value::Double(v) => v.first().copied().filter(|f| f.is_finite()),
        Value::Float(v) => v.first().map(|f| *f as f64).filter(|f| f.is_finite()),
        _ => None,
    }
}

fn uint(exif: &exif::Exif, tag: Tag) -> Option<u32> {
    exif.get_field(tag, In::PRIMARY)?.value.get_uint(0)
}

fn dms(exif: &exif::Exif, tag: Tag, reference: Tag, negative: u8) -> Option<f64> {
    let Value::Rational(v) = &exif.get_field(tag, In::PRIMARY)?.value else {
        return None;
    };
    if v.len() < 3 || v.iter().any(|r| r.denom == 0) {
        return None;
    }
    let value = v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0;
    let sign = match &exif.get_field(reference, In::PRIMARY)?.value {
        Value::Ascii(p) if p.first().and_then(|s| s.first()) == Some(&negative) => -1.0,
        _ => 1.0,
    };
    Some(value * sign)
}

fn exposure(seconds: f64) -> String {
    if seconds >= 1.0 {
        format!("{} s", (seconds * 10.0).round() / 10.0)
    } else {
        format!("1/{} s", (1.0 / seconds).round())
    }
}

fn datetime(raw: &str) -> Option<String> {
    // EXIF: "2024:05:01 13:22:10"
    let b = raw.as_bytes();
    if b.len() < 19 || b[4] != b':' || b[7] != b':' || b[10] != b' ' || raw.starts_with("0000") {
        return None;
    }
    Some(format!(
        "{}-{}-{}T{}",
        &raw[0..4],
        &raw[5..7],
        &raw[8..10],
        &raw[11..19]
    ))
}

pub fn details(path: &Path) -> PhotoDetails {
    let mut d = PhotoDetails::default();
    if let Ok((w, h)) = image::ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(image::ImageError::IoError)
        .and_then(|r| r.into_dimensions())
    {
        d.width = Some(w);
        d.height = Some(h);
    }
    let Ok(file) = std::fs::File::open(path) else {
        return d;
    };
    let Ok(exif) = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(file))
    else {
        return d;
    };
    // EXIF orientation 5-8 means the stored pixels are rotated 90°.
    if uint(&exif, Tag::Orientation).is_some_and(|o| (5..=8).contains(&o)) {
        std::mem::swap(&mut d.width, &mut d.height);
    }
    d.taken = text(&exif, Tag::DateTimeOriginal)
        .or_else(|| text(&exif, Tag::DateTime))
        .and_then(|t| datetime(&t));
    d.taken_offset = text(&exif, Tag::OffsetTimeOriginal).filter(|o| o.len() <= 6);
    d.make = text(&exif, Tag::Make);
    d.model = text(&exif, Tag::Model);
    // Many cameras repeat the brand in the model ("Canon" + "Canon EOS R6").
    if let (Some(make), Some(model)) = (&d.make, &d.model) {
        if model.to_lowercase().starts_with(&make.to_lowercase()) {
            d.make = None;
        }
    }
    d.lens = text(&exif, Tag::LensModel);
    d.aperture = rational(&exif, Tag::FNumber).map(|f| (f * 100.0).round() / 100.0);
    d.exposure = rational(&exif, Tag::ExposureTime)
        .filter(|s| *s > 0.0)
        .map(exposure);
    d.iso = uint(&exif, Tag::PhotographicSensitivity);
    d.focal_length = rational(&exif, Tag::FocalLength).map(|f| (f * 10.0).round() / 10.0);
    d.focal_length_35mm = uint(&exif, Tag::FocalLengthIn35mmFilm).filter(|f| *f > 0);
    d.flash = uint(&exif, Tag::Flash).map(|f| f & 1 == 1);
    d.software = text(&exif, Tag::Software);
    if let (Some(latitude), Some(longitude)) = (
        dms(&exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, b'S'),
        dms(&exif, Tag::GPSLongitude, Tag::GPSLongitudeRef, b'W'),
    ) {
        if (-90.0..=90.0).contains(&latitude)
            && (-180.0..=180.0).contains(&longitude)
            && (latitude, longitude) != (0.0, 0.0)
        {
            let below = matches!(
                exif.get_field(Tag::GPSAltitudeRef, In::PRIMARY).map(|f| &f.value),
                Some(Value::Byte(b)) if b.first() == Some(&1)
            );
            d.location = Some(Location {
                latitude: (latitude * 1e6).round() / 1e6,
                longitude: (longitude * 1e6).round() / 1e6,
                altitude: rational(&exif, Tag::GPSAltitude)
                    .map(|a| (if below { -a } else { a }).round()),
            });
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(exposure(1.0 / 125.0), "1/125 s");
        assert_eq!(exposure(2.5), "2.5 s");
        assert_eq!(
            datetime("2024:05:01 13:22:10"),
            Some("2024-05-01T13:22:10".into())
        );
        assert_eq!(datetime("0000:00:00 00:00:00"), None);
        assert_eq!(datetime("garbage"), None);
    }

    #[test]
    fn missing_or_plain_files_do_not_fail() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.jpg");
        std::fs::write(&p, b"\xff\xd8\xffnot really").unwrap();
        let info = details(&p);
        assert!(info.taken.is_none() && info.location.is_none());
        assert!(details(&d.path().join("missing.jpg")).width.is_none());
    }
}
