//! Image input utilities — load local images into content parts.

use crate::types::{ContentPart, MessageContent};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::path::Path;

/// Load an image file and encode it as a base64 data URL content part.
pub fn load_image_part(path: &Path) -> Result<ContentPart> {
    let data = std::fs::read(path).with_context(|| format!("read image {}", path.display()))?;
    let mime = guess_mime(path);
    let b64 = STANDARD.encode(data);
    Ok(ContentPart::image_from_base64(&mime, &b64))
}

/// Build a multipart message content from text and a list of image paths.
pub fn build_multipart_content(
    text: &str,
    image_paths: &[impl AsRef<Path>],
) -> Result<MessageContent> {
    let mut parts = vec![ContentPart::Text {
        text: text.to_string(),
    }];
    for path in image_paths {
        parts.push(load_image_part(path.as_ref())?);
    }
    Ok(MessageContent::Parts(parts))
}

fn guess_mime(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .map(|e| match e.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            "svg" => "image/svg+xml",
            _ => "image/png",
        })
        .unwrap_or("image/png")
        .to_string()
}

/// Return a human-readable size string for an image.
pub fn format_image_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Read image file size without loading contents.
pub fn image_file_size(path: &Path) -> Result<usize> {
    let meta = std::fs::metadata(path).with_context(|| format!("stat image {}", path.display()))?;
    Ok(meta.len() as usize)
}
