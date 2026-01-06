use pi::ai::{complete, get_model, Context, Message, StreamOptions};
use pi::{ContentBlock, UserContent, UserMessage};
use std::time::{SystemTime, UNIX_EPOCH};

// Source: packages/ai/test/image-limits.test.ts

#[test]
fn should_accept_a_small_number_of_images_5() {
    let model = get_model("mock", "test-model");
    let context = Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage {
            content: UserContent::Blocks(multi_image_blocks(5, "small")),
            timestamp: now_millis(),
        })],
        tools: None,
    };

    let response = complete(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    assert_eq!(response.stop_reason, "stop");
    assert!(!response.content.is_empty());
}

#[test]
fn should_find_maximum_image_count_limit() {
    let model = get_model("mock", "test-model");
    let mut last_success = 0;
    for count in [5, 10, 20] {
        let context = Context {
            system_prompt: None,
            messages: vec![Message::User(UserMessage {
                content: UserContent::Blocks(multi_image_blocks(count, "small")),
                timestamp: now_millis(),
            })],
            tools: None,
        };
        let response = complete(
            &model,
            &context,
            StreamOptions {
                signal: None,
                reasoning_effort: None,
            },
        );
        if response.stop_reason == "stop" {
            last_success = count;
        }
    }
    assert_eq!(last_success, 20);
}

#[test]
fn should_find_maximum_image_size_limit() {
    let model = get_model("mock", "test-model");
    let sizes = [32, 256, 2048];
    let mut last_success = 0;
    for size in sizes {
        let context = Context {
            system_prompt: None,
            messages: vec![Message::User(UserMessage {
                content: UserContent::Blocks(vec![
                    ContentBlock::Text {
                        text: "I am sending you an image.".to_string(),
                        text_signature: None,
                    },
                    ContentBlock::Image {
                        data: image_data(size),
                        mime_type: "image/png".to_string(),
                    },
                ]),
                timestamp: now_millis(),
            })],
            tools: None,
        };
        let response = complete(
            &model,
            &context,
            StreamOptions {
                signal: None,
                reasoning_effort: None,
            },
        );
        if response.stop_reason == "stop" {
            last_success = size;
        }
    }
    assert_eq!(last_success, 2048);
}

#[test]
fn should_find_maximum_image_dimension_limit() {
    let model = get_model("mock", "test-model");
    let context = Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage {
            content: UserContent::Blocks(vec![
                ContentBlock::Text {
                    text: "I am sending you a large dimension image.".to_string(),
                    text_signature: None,
                },
                ContentBlock::Image {
                    data: image_data(512),
                    mime_type: "image/png".to_string(),
                },
            ]),
            timestamp: now_millis(),
        })],
        tools: None,
    };

    let response = complete(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    assert_eq!(response.stop_reason, "stop");
    assert!(!response.content.is_empty());
}

fn multi_image_blocks(count: usize, label: &str) -> Vec<ContentBlock> {
    let mut blocks = Vec::with_capacity(count + 1);
    blocks.push(ContentBlock::Text {
        text: format!("Sending {count} images ({label})."),
        text_signature: None,
    });
    for _ in 0..count {
        blocks.push(ContentBlock::Image {
            data: image_data(16),
            mime_type: "image/png".to_string(),
        });
    }
    blocks
}

fn image_data(size: usize) -> String {
    "A".repeat(size)
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
