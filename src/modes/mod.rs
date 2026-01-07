use crate::cli::file_inputs::FileInputImage;
use crate::core::messages::{ContentBlock, UserContent};

pub mod interactive;
pub mod print;

pub use interactive::run_interactive_mode_session;
pub use print::run_print_mode_session;

pub(crate) fn build_user_content_from_files(
    message: Option<&str>,
    images: &[FileInputImage],
) -> Result<UserContent, String> {
    let mut blocks = Vec::new();
    if let Some(message) = message {
        if !message.trim().is_empty() {
            blocks.push(ContentBlock::Text {
                text: message.to_string(),
                text_signature: None,
            });
        }
    }
    for image in images {
        blocks.push(ContentBlock::Image {
            data: image.data.clone(),
            mime_type: image.mime_type.clone(),
        });
    }
    if blocks.is_empty() {
        return Err("No prompt content provided.".to_string());
    }
    Ok(UserContent::Blocks(blocks))
}
