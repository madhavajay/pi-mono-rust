//! Terminal image rendering support for Kitty and iTerm2 protocols.
//!
//! Ported from pi-mono/packages/tui/src/terminal-image.ts

use base64::{engine::general_purpose::STANDARD, Engine};
use std::sync::OnceLock;

/// Image protocol supported by the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageProtocol {
    Kitty,
    ITerm2,
}

/// Terminal capabilities for image rendering.
#[derive(Debug, Clone)]
pub struct TerminalCapabilities {
    pub images: Option<ImageProtocol>,
    pub true_color: bool,
    pub hyperlinks: bool,
}

/// Cell dimensions in pixels.
#[derive(Debug, Clone, Copy)]
pub struct CellDimensions {
    pub width_px: u32,
    pub height_px: u32,
}

impl Default for CellDimensions {
    fn default() -> Self {
        // Default cell dimensions - updated by TUI when terminal responds to query
        Self {
            width_px: 9,
            height_px: 18,
        }
    }
}

/// Image dimensions in pixels.
#[derive(Debug, Clone, Copy)]
pub struct ImageDimensions {
    pub width_px: u32,
    pub height_px: u32,
}

/// Options for rendering images.
#[derive(Debug, Clone, Default)]
pub struct ImageRenderOptions {
    pub max_width_cells: Option<u32>,
    pub max_height_cells: Option<u32>,
    pub preserve_aspect_ratio: Option<bool>,
}

static CACHED_CAPABILITIES: OnceLock<TerminalCapabilities> = OnceLock::new();
static CELL_DIMENSIONS: std::sync::RwLock<CellDimensions> =
    std::sync::RwLock::new(CellDimensions {
        width_px: 9,
        height_px: 18,
    });

/// Get the current cell dimensions.
pub fn get_cell_dimensions() -> CellDimensions {
    *CELL_DIMENSIONS.read().unwrap()
}

/// Set the cell dimensions (updated when terminal responds to query).
pub fn set_cell_dimensions(dims: CellDimensions) {
    *CELL_DIMENSIONS.write().unwrap() = dims;
}

/// Detect terminal capabilities from environment variables.
pub fn detect_capabilities() -> TerminalCapabilities {
    let term_program = std::env::var("TERM_PROGRAM")
        .unwrap_or_default()
        .to_lowercase();
    let term = std::env::var("TERM").unwrap_or_default().to_lowercase();
    let color_term = std::env::var("COLORTERM")
        .unwrap_or_default()
        .to_lowercase();

    // Kitty
    if std::env::var("KITTY_WINDOW_ID").is_ok() || term_program == "kitty" {
        return TerminalCapabilities {
            images: Some(ImageProtocol::Kitty),
            true_color: true,
            hyperlinks: true,
        };
    }

    // Ghostty uses Kitty protocol
    if term_program == "ghostty"
        || term.contains("ghostty")
        || std::env::var("GHOSTTY_RESOURCES_DIR").is_ok()
    {
        return TerminalCapabilities {
            images: Some(ImageProtocol::Kitty),
            true_color: true,
            hyperlinks: true,
        };
    }

    // WezTerm uses Kitty protocol
    if std::env::var("WEZTERM_PANE").is_ok() || term_program == "wezterm" {
        return TerminalCapabilities {
            images: Some(ImageProtocol::Kitty),
            true_color: true,
            hyperlinks: true,
        };
    }

    // iTerm2
    if std::env::var("ITERM_SESSION_ID").is_ok() || term_program == "iterm.app" {
        return TerminalCapabilities {
            images: Some(ImageProtocol::ITerm2),
            true_color: true,
            hyperlinks: true,
        };
    }

    // VSCode - no image support
    if term_program == "vscode" {
        return TerminalCapabilities {
            images: None,
            true_color: true,
            hyperlinks: true,
        };
    }

    // Alacritty - no image support
    if term_program == "alacritty" {
        return TerminalCapabilities {
            images: None,
            true_color: true,
            hyperlinks: true,
        };
    }

    // Fallback
    let true_color = color_term == "truecolor" || color_term == "24bit";
    TerminalCapabilities {
        images: None,
        true_color,
        hyperlinks: true,
    }
}

/// Get terminal capabilities (cached).
pub fn get_capabilities() -> &'static TerminalCapabilities {
    CACHED_CAPABILITIES.get_or_init(detect_capabilities)
}

/// Encode image data using the Kitty graphics protocol.
pub fn encode_kitty(base64_data: &str, columns: Option<u32>, rows: Option<u32>) -> String {
    const CHUNK_SIZE: usize = 4096;

    let mut params = vec!["a=T".to_string(), "f=100".to_string(), "q=2".to_string()];

    if let Some(c) = columns {
        params.push(format!("c={c}"));
    }
    if let Some(r) = rows {
        params.push(format!("r={r}"));
    }

    if base64_data.len() <= CHUNK_SIZE {
        return format!("\x1b_G{};{}\x1b\\", params.join(","), base64_data);
    }

    let mut chunks = Vec::new();
    let mut offset = 0;
    let mut is_first = true;

    while offset < base64_data.len() {
        let end = std::cmp::min(offset + CHUNK_SIZE, base64_data.len());
        let chunk = &base64_data[offset..end];
        let is_last = end >= base64_data.len();

        if is_first {
            chunks.push(format!("\x1b_G{},m=1;{}\x1b\\", params.join(","), chunk));
            is_first = false;
        } else if is_last {
            chunks.push(format!("\x1b_Gm=0;{}\x1b\\", chunk));
        } else {
            chunks.push(format!("\x1b_Gm=1;{}\x1b\\", chunk));
        }

        offset = end;
    }

    chunks.join("")
}

/// Encode image data using the iTerm2 inline images protocol.
pub fn encode_iterm2(
    base64_data: &str,
    width: Option<&str>,
    height: Option<&str>,
    name: Option<&str>,
    preserve_aspect_ratio: Option<bool>,
) -> String {
    let mut params = vec!["inline=1".to_string()];

    if let Some(w) = width {
        params.push(format!("width={w}"));
    }
    if let Some(h) = height {
        params.push(format!("height={h}"));
    }
    if let Some(n) = name {
        let name_base64 = STANDARD.encode(n);
        params.push(format!("name={name_base64}"));
    }
    if preserve_aspect_ratio == Some(false) {
        params.push("preserveAspectRatio=0".to_string());
    }

    format!("\x1b]1337;File={}:{}\x07", params.join(";"), base64_data)
}

/// Calculate the number of terminal rows needed to display an image.
pub fn calculate_image_rows(
    image_dimensions: ImageDimensions,
    target_width_cells: u32,
    cell_dims: CellDimensions,
) -> u32 {
    let target_width_px = target_width_cells * cell_dims.width_px;
    let scale = target_width_px as f64 / image_dimensions.width_px as f64;
    let scaled_height_px = image_dimensions.height_px as f64 * scale;
    let rows = (scaled_height_px / cell_dims.height_px as f64).ceil() as u32;
    std::cmp::max(1, rows)
}

/// Extract PNG dimensions from base64-encoded data.
pub fn get_png_dimensions(base64_data: &str) -> Option<ImageDimensions> {
    let buffer = STANDARD.decode(base64_data).ok()?;

    if buffer.len() < 24 {
        return None;
    }

    // Check PNG signature: 0x89 0x50 0x4e 0x47
    if buffer[0] != 0x89 || buffer[1] != 0x50 || buffer[2] != 0x4e || buffer[3] != 0x47 {
        return None;
    }

    let width = u32::from_be_bytes([buffer[16], buffer[17], buffer[18], buffer[19]]);
    let height = u32::from_be_bytes([buffer[20], buffer[21], buffer[22], buffer[23]]);

    Some(ImageDimensions {
        width_px: width,
        height_px: height,
    })
}

/// Extract JPEG dimensions from base64-encoded data.
pub fn get_jpeg_dimensions(base64_data: &str) -> Option<ImageDimensions> {
    let buffer = STANDARD.decode(base64_data).ok()?;

    if buffer.len() < 2 {
        return None;
    }

    // Check JPEG signature: 0xff 0xd8
    if buffer[0] != 0xff || buffer[1] != 0xd8 {
        return None;
    }

    let mut offset = 2;
    while offset < buffer.len().saturating_sub(9) {
        if buffer[offset] != 0xff {
            offset += 1;
            continue;
        }

        let marker = buffer[offset + 1];

        // SOF markers (Start of Frame)
        if (0xc0..=0xc2).contains(&marker) {
            let height = u16::from_be_bytes([buffer[offset + 5], buffer[offset + 6]]);
            let width = u16::from_be_bytes([buffer[offset + 7], buffer[offset + 8]]);
            return Some(ImageDimensions {
                width_px: width as u32,
                height_px: height as u32,
            });
        }

        if offset + 3 >= buffer.len() {
            return None;
        }
        let length = u16::from_be_bytes([buffer[offset + 2], buffer[offset + 3]]) as usize;
        if length < 2 {
            return None;
        }
        offset += 2 + length;
    }

    None
}

/// Extract GIF dimensions from base64-encoded data.
pub fn get_gif_dimensions(base64_data: &str) -> Option<ImageDimensions> {
    let buffer = STANDARD.decode(base64_data).ok()?;

    if buffer.len() < 10 {
        return None;
    }

    // Check GIF signature: "GIF87a" or "GIF89a"
    let sig = std::str::from_utf8(&buffer[0..6]).ok()?;
    if sig != "GIF87a" && sig != "GIF89a" {
        return None;
    }

    let width = u16::from_le_bytes([buffer[6], buffer[7]]);
    let height = u16::from_le_bytes([buffer[8], buffer[9]]);

    Some(ImageDimensions {
        width_px: width as u32,
        height_px: height as u32,
    })
}

/// Extract WebP dimensions from base64-encoded data.
pub fn get_webp_dimensions(base64_data: &str) -> Option<ImageDimensions> {
    let buffer = STANDARD.decode(base64_data).ok()?;

    if buffer.len() < 30 {
        return None;
    }

    // Check RIFF header and WEBP signature
    let riff = std::str::from_utf8(&buffer[0..4]).ok()?;
    let webp = std::str::from_utf8(&buffer[8..12]).ok()?;
    if riff != "RIFF" || webp != "WEBP" {
        return None;
    }

    let chunk = std::str::from_utf8(&buffer[12..16]).ok()?;

    match chunk {
        "VP8 " => {
            if buffer.len() < 30 {
                return None;
            }
            let width = (u16::from_le_bytes([buffer[26], buffer[27]]) & 0x3fff) as u32;
            let height = (u16::from_le_bytes([buffer[28], buffer[29]]) & 0x3fff) as u32;
            Some(ImageDimensions {
                width_px: width,
                height_px: height,
            })
        }
        "VP8L" => {
            if buffer.len() < 25 {
                return None;
            }
            let bits = u32::from_le_bytes([buffer[21], buffer[22], buffer[23], buffer[24]]);
            let width = (bits & 0x3fff) + 1;
            let height = ((bits >> 14) & 0x3fff) + 1;
            Some(ImageDimensions {
                width_px: width,
                height_px: height,
            })
        }
        "VP8X" => {
            if buffer.len() < 30 {
                return None;
            }
            let width =
                (buffer[24] as u32 | ((buffer[25] as u32) << 8) | ((buffer[26] as u32) << 16)) + 1;
            let height =
                (buffer[27] as u32 | ((buffer[28] as u32) << 8) | ((buffer[29] as u32) << 16)) + 1;
            Some(ImageDimensions {
                width_px: width,
                height_px: height,
            })
        }
        _ => None,
    }
}

/// Get image dimensions based on MIME type.
pub fn get_image_dimensions(base64_data: &str, mime_type: &str) -> Option<ImageDimensions> {
    match mime_type {
        "image/png" => get_png_dimensions(base64_data),
        "image/jpeg" => get_jpeg_dimensions(base64_data),
        "image/gif" => get_gif_dimensions(base64_data),
        "image/webp" => get_webp_dimensions(base64_data),
        _ => None,
    }
}

/// Result of rendering an image.
pub struct ImageRenderResult {
    pub sequence: String,
    pub rows: u32,
}

/// Render an image to a terminal escape sequence.
pub fn render_image(
    base64_data: &str,
    image_dimensions: ImageDimensions,
    options: &ImageRenderOptions,
) -> Option<ImageRenderResult> {
    let caps = get_capabilities();

    let protocol = caps.images?;

    let max_width = options.max_width_cells.unwrap_or(80);
    let rows = calculate_image_rows(image_dimensions, max_width, get_cell_dimensions());

    let sequence = match protocol {
        ImageProtocol::Kitty => encode_kitty(base64_data, Some(max_width), Some(rows)),
        ImageProtocol::ITerm2 => encode_iterm2(
            base64_data,
            Some(&max_width.to_string()),
            Some("auto"),
            None,
            options.preserve_aspect_ratio,
        ),
    };

    Some(ImageRenderResult { sequence, rows })
}

/// Generate fallback text for unsupported terminals.
pub fn image_fallback(
    mime_type: &str,
    dimensions: Option<ImageDimensions>,
    filename: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if let Some(name) = filename {
        parts.push(name.to_string());
    }
    parts.push(format!("[{mime_type}]"));
    if let Some(dims) = dimensions {
        parts.push(format!("{}x{}", dims.width_px, dims.height_px));
    }
    format!("[Image: {}]", parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_kitty_small_image() {
        let data = "SGVsbG8gV29ybGQ="; // "Hello World" in base64
        let result = encode_kitty(data, Some(40), Some(10));
        assert!(result.starts_with("\x1b_G"));
        assert!(result.contains("a=T"));
        assert!(result.contains("c=40"));
        assert!(result.contains("r=10"));
        assert!(result.ends_with("\x1b\\"));
    }

    #[test]
    fn test_encode_kitty_chunked() {
        // Create data larger than 4096 bytes
        let large_data = "A".repeat(5000);
        let result = encode_kitty(&large_data, None, None);
        // Should have multiple chunks
        assert!(result.contains("m=1"));
        assert!(result.contains("m=0"));
    }

    #[test]
    fn test_encode_iterm2() {
        let data = "SGVsbG8gV29ybGQ=";
        let result = encode_iterm2(data, Some("40"), Some("auto"), Some("test.png"), None);
        assert!(result.starts_with("\x1b]1337;File="));
        assert!(result.contains("inline=1"));
        assert!(result.contains("width=40"));
        assert!(result.contains("height=auto"));
        assert!(result.ends_with("\x07"));
    }

    #[test]
    fn test_calculate_image_rows() {
        let dims = ImageDimensions {
            width_px: 800,
            height_px: 600,
        };
        let cell_dims = CellDimensions {
            width_px: 9,
            height_px: 18,
        };
        let rows = calculate_image_rows(dims, 40, cell_dims);
        // 40 cells * 9px = 360px width
        // Scale: 360/800 = 0.45
        // Scaled height: 600 * 0.45 = 270px
        // Rows: ceil(270/18) = 15
        assert_eq!(rows, 15);
    }

    #[test]
    fn test_get_png_dimensions() {
        // Minimal valid PNG with 100x50 dimensions
        // PNG header + IHDR chunk
        let png_bytes: Vec<u8> = vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, // PNG signature
            0x00, 0x00, 0x00, 0x0d, // IHDR length
            0x49, 0x48, 0x44, 0x52, // "IHDR"
            0x00, 0x00, 0x00, 0x64, // width: 100
            0x00, 0x00, 0x00, 0x32, // height: 50
            0x08, 0x06, 0x00, 0x00, 0x00, // bit depth, color type, etc.
        ];
        let base64_data = STANDARD.encode(&png_bytes);
        let dims = get_png_dimensions(&base64_data).unwrap();
        assert_eq!(dims.width_px, 100);
        assert_eq!(dims.height_px, 50);
    }

    #[test]
    fn test_get_gif_dimensions() {
        // Minimal GIF header with 100x50 dimensions
        let gif_bytes: Vec<u8> = vec![
            0x47, 0x49, 0x46, 0x38, 0x39, 0x61, // "GIF89a"
            0x64, 0x00, // width: 100 (little-endian)
            0x32, 0x00, // height: 50 (little-endian)
        ];
        let base64_data = STANDARD.encode(&gif_bytes);
        let dims = get_gif_dimensions(&base64_data).unwrap();
        assert_eq!(dims.width_px, 100);
        assert_eq!(dims.height_px, 50);
    }

    #[test]
    fn test_image_fallback() {
        let fallback = image_fallback(
            "image/png",
            Some(ImageDimensions {
                width_px: 800,
                height_px: 600,
            }),
            Some("test.png"),
        );
        assert_eq!(fallback, "[Image: test.png [image/png] 800x600]");
    }

    #[test]
    fn test_image_fallback_no_filename() {
        let fallback = image_fallback(
            "image/jpeg",
            Some(ImageDimensions {
                width_px: 1920,
                height_px: 1080,
            }),
            None,
        );
        assert_eq!(fallback, "[Image: [image/jpeg] 1920x1080]");
    }
}
