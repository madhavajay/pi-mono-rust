//! Terminal image tests - ported from pi-mono/packages/tui/test/image-test.ts
//!
//! These tests verify the terminal image encoding and dimension parsing functionality.

use base64::{engine::general_purpose::STANDARD, Engine};
use pi::tui::{
    calculate_image_rows, encode_iterm2, encode_kitty, get_gif_dimensions, get_image_dimensions,
    get_jpeg_dimensions, get_png_dimensions, get_webp_dimensions, image_fallback, CellDimensions,
    ImageDimensions, ImageProtocol, TerminalCapabilities,
};

// ============================================================================
// Kitty Protocol Tests
// ============================================================================

#[test]
fn test_encode_kitty_small_data() {
    // Small data should be sent in a single chunk
    let data = "SGVsbG8gV29ybGQ="; // "Hello World" in base64
    let result = encode_kitty(data, Some(40), Some(10));

    assert!(result.starts_with("\x1b_G"));
    assert!(result.contains("a=T")); // Transmit action
    assert!(result.contains("f=100")); // Format: auto-detect
    assert!(result.contains("q=2")); // Quiet mode
    assert!(result.contains("c=40")); // Columns
    assert!(result.contains("r=10")); // Rows
    assert!(result.ends_with("\x1b\\"));
    assert!(!result.contains(",m=")); // No chunking
}

#[test]
fn test_encode_kitty_chunked_data() {
    // Large data should be chunked (>4096 bytes)
    let large_data = "A".repeat(5000);
    let result = encode_kitty(&large_data, None, None);

    // Should have multiple chunks
    assert!(result.contains(",m=1")); // More data coming
    assert!(result.contains("m=0")); // Last chunk

    // Count escape sequences
    let chunk_count = result.matches("\x1b_G").count();
    assert!(chunk_count > 1);
}

#[test]
fn test_encode_kitty_no_options() {
    let data = "SGVsbG8=";
    let result = encode_kitty(data, None, None);

    assert!(result.starts_with("\x1b_G"));
    assert!(result.contains("a=T"));
    assert!(!result.contains("c=")); // No columns
    assert!(!result.contains("r=")); // No rows
}

// ============================================================================
// iTerm2 Protocol Tests
// ============================================================================

#[test]
fn test_encode_iterm2_basic() {
    let data = "SGVsbG8gV29ybGQ=";
    let result = encode_iterm2(data, None, None, None, None);

    assert!(result.starts_with("\x1b]1337;File="));
    assert!(result.contains("inline=1"));
    assert!(result.ends_with("\x07"));
}

#[test]
fn test_encode_iterm2_with_options() {
    let data = "SGVsbG8=";
    let result = encode_iterm2(data, Some("40"), Some("auto"), Some("test.png"), None);

    assert!(result.contains("width=40"));
    assert!(result.contains("height=auto"));
    assert!(result.contains("name=")); // Base64-encoded name
}

#[test]
fn test_encode_iterm2_preserve_aspect_ratio_false() {
    let data = "SGVsbG8=";
    let result = encode_iterm2(data, None, None, None, Some(false));

    assert!(result.contains("preserveAspectRatio=0"));
}

// ============================================================================
// PNG Dimension Tests
// ============================================================================

#[test]
fn test_get_png_dimensions() {
    // Minimal valid PNG with 100x50 dimensions
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
fn test_get_png_dimensions_1920x1080() {
    // PNG with 1920x1080 dimensions
    let png_bytes: Vec<u8> = vec![
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, // PNG signature
        0x00, 0x00, 0x00, 0x0d, // IHDR length
        0x49, 0x48, 0x44, 0x52, // "IHDR"
        0x00, 0x00, 0x07, 0x80, // width: 1920 (0x780)
        0x00, 0x00, 0x04, 0x38, // height: 1080 (0x438)
        0x08, 0x06, 0x00, 0x00, 0x00, // bit depth, color type, etc.
    ];
    let base64_data = STANDARD.encode(&png_bytes);
    let dims = get_png_dimensions(&base64_data).unwrap();

    assert_eq!(dims.width_px, 1920);
    assert_eq!(dims.height_px, 1080);
}

#[test]
fn test_get_png_dimensions_invalid_signature() {
    // Not a PNG
    let data = STANDARD.encode(b"Not a PNG image");
    assert!(get_png_dimensions(&data).is_none());
}

#[test]
fn test_get_png_dimensions_too_short() {
    // Too short to be valid
    let data = STANDARD.encode([0x89, 0x50, 0x4e, 0x47]);
    assert!(get_png_dimensions(&data).is_none());
}

// ============================================================================
// GIF Dimension Tests
// ============================================================================

#[test]
fn test_get_gif_dimensions_gif89a() {
    // GIF89a header with 100x50 dimensions
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
fn test_get_gif_dimensions_gif87a() {
    // GIF87a header with 320x200 dimensions
    let gif_bytes: Vec<u8> = vec![
        0x47, 0x49, 0x46, 0x38, 0x37, 0x61, // "GIF87a"
        0x40, 0x01, // width: 320 (little-endian)
        0xc8, 0x00, // height: 200 (little-endian)
    ];
    let base64_data = STANDARD.encode(&gif_bytes);
    let dims = get_gif_dimensions(&base64_data).unwrap();

    assert_eq!(dims.width_px, 320);
    assert_eq!(dims.height_px, 200);
}

#[test]
fn test_get_gif_dimensions_invalid() {
    let data = STANDARD.encode(b"Not a GIF image");
    assert!(get_gif_dimensions(&data).is_none());
}

// ============================================================================
// JPEG Dimension Tests
// ============================================================================

#[test]
fn test_get_jpeg_dimensions() {
    // Minimal JPEG with SOF0 marker (0xffc0) containing 640x480 dimensions
    let jpeg_bytes: Vec<u8> = vec![
        0xff, 0xd8, // SOI marker
        0xff, 0xc0, // SOF0 marker
        0x00, 0x11, // Length (17 bytes)
        0x08, // Precision
        0x01, 0xe0, // Height: 480
        0x02, 0x80, // Width: 640
        0x03, // Number of components
        0x01, 0x22, 0x00, // Component 1
        0x02, 0x11, 0x01, // Component 2
        0x03, 0x11, 0x01, // Component 3
    ];
    let base64_data = STANDARD.encode(&jpeg_bytes);
    let dims = get_jpeg_dimensions(&base64_data).unwrap();

    assert_eq!(dims.width_px, 640);
    assert_eq!(dims.height_px, 480);
}

#[test]
fn test_get_jpeg_dimensions_invalid() {
    let data = STANDARD.encode(b"Not a JPEG image");
    assert!(get_jpeg_dimensions(&data).is_none());
}

// ============================================================================
// WebP Dimension Tests (VP8 Lossy)
// ============================================================================

#[test]
fn test_get_webp_dimensions_vp8() {
    // WebP with VP8 chunk (lossy), 320x240
    let mut webp_bytes: Vec<u8> = vec![
        0x52, 0x49, 0x46, 0x46, // "RIFF"
        0x00, 0x00, 0x00, 0x00, // File size (placeholder)
        0x57, 0x45, 0x42, 0x50, // "WEBP"
        0x56, 0x50, 0x38, 0x20, // "VP8 "
        0x00, 0x00, 0x00, 0x00, // Chunk size
        0x30, 0x01, 0x00, // VP8 bitstream signature
        0x9d, 0x01, 0x2a, // VP8 frame header
    ];
    // Width: 320 (0x140), Height: 240 (0x0f0) at bytes 26-29
    webp_bytes.extend_from_slice(&[0x40, 0x01]); // Width with reserved bits
    webp_bytes.extend_from_slice(&[0xf0, 0x00]); // Height with reserved bits

    let base64_data = STANDARD.encode(&webp_bytes);
    let dims = get_webp_dimensions(&base64_data).unwrap();

    assert_eq!(dims.width_px, 320);
    assert_eq!(dims.height_px, 240);
}

// ============================================================================
// getImageDimensions dispatcher Tests
// ============================================================================

#[test]
fn test_get_image_dimensions_png() {
    let png_bytes: Vec<u8> = vec![
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x64, // width: 100
        0x00, 0x00, 0x00, 0x32, // height: 50
        0x08, 0x06, 0x00, 0x00, 0x00,
    ];
    let base64_data = STANDARD.encode(&png_bytes);
    let dims = get_image_dimensions(&base64_data, "image/png").unwrap();

    assert_eq!(dims.width_px, 100);
    assert_eq!(dims.height_px, 50);
}

#[test]
fn test_get_image_dimensions_jpeg() {
    let jpeg_bytes: Vec<u8> = vec![
        0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x01, 0xe0, // Height: 480
        0x02, 0x80, // Width: 640
        0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01,
    ];
    let base64_data = STANDARD.encode(&jpeg_bytes);
    let dims = get_image_dimensions(&base64_data, "image/jpeg").unwrap();

    assert_eq!(dims.width_px, 640);
    assert_eq!(dims.height_px, 480);
}

#[test]
fn test_get_image_dimensions_gif() {
    let gif_bytes: Vec<u8> = vec![
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, // "GIF89a"
        0x64, 0x00, // width: 100
        0x32, 0x00, // height: 50
    ];
    let base64_data = STANDARD.encode(&gif_bytes);
    let dims = get_image_dimensions(&base64_data, "image/gif").unwrap();

    assert_eq!(dims.width_px, 100);
    assert_eq!(dims.height_px, 50);
}

#[test]
fn test_get_image_dimensions_unknown_mime_type() {
    let data = STANDARD.encode(b"some data");
    assert!(get_image_dimensions(&data, "application/octet-stream").is_none());
}

// ============================================================================
// Row Calculation Tests
// ============================================================================

#[test]
fn test_calculate_image_rows_basic() {
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
fn test_calculate_image_rows_wide_image() {
    let dims = ImageDimensions {
        width_px: 1920,
        height_px: 1080,
    };
    let cell_dims = CellDimensions {
        width_px: 9,
        height_px: 18,
    };
    let rows = calculate_image_rows(dims, 60, cell_dims);

    // 60 cells * 9px = 540px width
    // Scale: 540/1920 = 0.28125
    // Scaled height: 1080 * 0.28125 = 303.75px
    // Rows: ceil(303.75/18) = 17
    assert_eq!(rows, 17);
}

#[test]
fn test_calculate_image_rows_minimum_one() {
    let dims = ImageDimensions {
        width_px: 1000,
        height_px: 1,
    };
    let cell_dims = CellDimensions {
        width_px: 9,
        height_px: 18,
    };
    let rows = calculate_image_rows(dims, 40, cell_dims);

    // Even very small images should have at least 1 row
    assert_eq!(rows, 1);
}

// ============================================================================
// Fallback Text Tests
// ============================================================================

#[test]
fn test_image_fallback_full() {
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

#[test]
fn test_image_fallback_no_dimensions() {
    let fallback = image_fallback("image/gif", None, Some("animation.gif"));
    assert_eq!(fallback, "[Image: animation.gif [image/gif]]");
}

#[test]
fn test_image_fallback_minimal() {
    let fallback = image_fallback("image/webp", None, None);
    assert_eq!(fallback, "[Image: [image/webp]]");
}

// ============================================================================
// Terminal Capabilities Tests
// ============================================================================

#[test]
fn test_terminal_capabilities_struct() {
    let caps = TerminalCapabilities {
        images: Some(ImageProtocol::Kitty),
        true_color: true,
        hyperlinks: true,
    };

    assert_eq!(caps.images, Some(ImageProtocol::Kitty));
    assert!(caps.true_color);
    assert!(caps.hyperlinks);
}

#[test]
fn test_terminal_capabilities_no_images() {
    let caps = TerminalCapabilities {
        images: None,
        true_color: true,
        hyperlinks: true,
    };

    assert!(caps.images.is_none());
}

// ============================================================================
// Cell Dimensions Tests
// ============================================================================

#[test]
fn test_cell_dimensions_default() {
    let dims = CellDimensions::default();
    assert_eq!(dims.width_px, 9);
    assert_eq!(dims.height_px, 18);
}
