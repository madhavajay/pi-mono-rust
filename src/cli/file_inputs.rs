use std::env;
use std::path::PathBuf;

#[derive(Clone)]
pub struct FileInputImage {
    pub mime_type: String,
    pub data: String,
}

pub struct FileInputs {
    pub text_prefix: String,
    pub images: Vec<FileInputImage>,
}

pub fn build_file_inputs(file_args: &[String]) -> Result<FileInputs, String> {
    let mut text = String::new();
    let mut images = Vec::new();
    for file_arg in file_args {
        let path = resolve_file_arg(file_arg);
        let data = std::fs::read(&path)
            .map_err(|err| format!("Error: Could not read file {}: {}", path.display(), err))?;
        if data.is_empty() {
            continue;
        }

        if let Some(mime_type) = detect_image_mime_type(&data) {
            let encoded = base64_encode(&data);
            images.push(FileInputImage {
                mime_type: mime_type.to_string(),
                data: encoded,
            });
            text.push_str(&format!("<file name=\"{}\"></file>\n", path.display()));
            continue;
        }

        let content = String::from_utf8(data)
            .map_err(|err| format!("Error: Could not read file {}: {}", path.display(), err))?;
        if content.trim().is_empty() {
            continue;
        }
        text.push_str(&format!("<file name=\"{}\">\n", path.display()));
        text.push_str(&content);
        if !content.ends_with('\n') {
            text.push('\n');
        }
        text.push_str("</file>\n");
    }
    Ok(FileInputs {
        text_prefix: text,
        images,
    })
}

fn resolve_file_arg(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }

    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else if let Ok(cwd) = env::current_dir() {
        cwd.join(path)
    } else {
        path
    }
}

fn detect_image_mime_type(data: &[u8]) -> Option<&'static str> {
    let png_magic: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if data.len() >= png_magic.len() && data[..png_magic.len()] == png_magic {
        return Some("image/png");
    }

    if data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        return Some("image/jpeg");
    }

    if data.len() >= 6 {
        let header = &data[..6];
        if header == b"GIF87a" || header == b"GIF89a" {
            return Some("image/gif");
        }
    }

    if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return Some("image/webp");
    }

    None
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i];
        let b1 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] } else { 0 };
        output.push(TABLE[(b0 >> 2) as usize] as char);
        output.push(TABLE[((b0 & 0x03) << 4 | (b1 >> 4)) as usize] as char);
        if i + 1 < data.len() {
            output.push(TABLE[((b1 & 0x0f) << 2 | (b2 >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if i + 2 < data.len() {
            output.push(TABLE[(b2 & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
        i += 3;
    }
    output
}
