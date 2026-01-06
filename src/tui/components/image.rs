//! Image component for TUI rendering.
//!
//! Ported from pi-mono/packages/tui/src/components/image.ts

use super::Component;
use crate::tui::terminal_image::{
    get_capabilities, get_image_dimensions, image_fallback, render_image, ImageDimensions,
    ImageRenderOptions,
};

/// Theme for image rendering.
pub struct ImageTheme {
    /// Function to apply fallback text styling.
    pub fallback_color: Box<dyn Fn(&str) -> String + Send + Sync>,
}

impl Default for ImageTheme {
    fn default() -> Self {
        Self {
            fallback_color: Box::new(|s| format!("\x1b[33m{s}\x1b[0m")), // Yellow
        }
    }
}

/// Options for image rendering.
#[derive(Debug, Clone, Default)]
pub struct ImageOptions {
    pub max_width_cells: Option<u32>,
    pub max_height_cells: Option<u32>,
    pub filename: Option<String>,
}

/// Image component for displaying inline images in the TUI.
pub struct Image {
    base64_data: String,
    mime_type: String,
    dimensions: ImageDimensions,
    theme: ImageTheme,
    options: ImageOptions,
    cached_lines: Option<Vec<String>>,
    cached_width: Option<usize>,
}

impl Image {
    /// Create a new Image component.
    pub fn new(
        base64_data: String,
        mime_type: String,
        theme: ImageTheme,
        options: ImageOptions,
        dimensions: Option<ImageDimensions>,
    ) -> Self {
        let dims = dimensions
            .or_else(|| get_image_dimensions(&base64_data, &mime_type))
            .unwrap_or(ImageDimensions {
                width_px: 800,
                height_px: 600,
            });

        Self {
            base64_data,
            mime_type,
            dimensions: dims,
            theme,
            options,
            cached_lines: None,
            cached_width: None,
        }
    }

    /// Invalidate the cached render.
    pub fn invalidate(&mut self) {
        self.cached_lines = None;
        self.cached_width = None;
    }
}

impl Component for Image {
    fn render(&self, width: usize) -> Vec<String> {
        // Check cache
        if let (Some(ref lines), Some(cached_width)) = (&self.cached_lines, self.cached_width) {
            if cached_width == width {
                return lines.clone();
            }
        }

        let max_width = std::cmp::min(
            width.saturating_sub(2),
            self.options.max_width_cells.unwrap_or(60) as usize,
        );

        let caps = get_capabilities();

        let lines = if caps.images.is_some() {
            let result = render_image(
                &self.base64_data,
                self.dimensions,
                &ImageRenderOptions {
                    max_width_cells: Some(max_width as u32),
                    ..Default::default()
                },
            );

            if let Some(result) = result {
                // Return `rows` lines so TUI accounts for image height
                // First (rows-1) lines are empty (TUI clears them)
                // Last line: move cursor back up, then output image sequence
                let mut lines = Vec::new();
                for _ in 0..(result.rows.saturating_sub(1)) {
                    lines.push(String::new());
                }
                // Move cursor up to first row, then output image
                let move_up = if result.rows > 1 {
                    format!("\x1b[{}A", result.rows - 1)
                } else {
                    String::new()
                };
                lines.push(format!("{}{}", move_up, result.sequence));
                lines
            } else {
                let fallback = image_fallback(
                    &self.mime_type,
                    Some(self.dimensions),
                    self.options.filename.as_deref(),
                );
                vec![(self.theme.fallback_color)(&fallback)]
            }
        } else {
            let fallback = image_fallback(
                &self.mime_type,
                Some(self.dimensions),
                self.options.filename.as_deref(),
            );
            vec![(self.theme.fallback_color)(&fallback)]
        };

        lines
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
