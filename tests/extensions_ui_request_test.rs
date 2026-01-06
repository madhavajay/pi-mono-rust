use pi::coding_agent::{ExtensionHost, ExtensionUiResponse};
use pi::core::messages::ContentBlock;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("{}-{}", prefix, uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_extension(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, contents).expect("write extension file");
    path
}

#[test]
fn extension_ui_requests_round_trip() {
    let temp = TempDir::new("pi-extension-ui");
    let extensions_dir = temp.path().join("extensions");
    fs::create_dir_all(&extensions_dir).expect("create extensions dir");
    let ext = write_extension(
        &extensions_dir,
        "ui-tool.js",
        r#"
        module.exports = function(pi) {
            pi.registerTool({
                name: "ask_ui",
                description: "Asks for input",
                parameters: {
                    type: "object",
                    properties: {},
                    additionalProperties: false,
                },
                async execute(_callId, _params, _unused, ctx) {
                    const answer = await ctx.ui.input("Title", "Placeholder");
                    return answer || "";
                },
            });
        };
        "#,
    );

    let (mut host, _manifest) =
        ExtensionHost::spawn(&[ext], temp.path()).expect("spawn extension host");
    host.set_ui_handler(|request| {
        if request.method == "input" {
            ExtensionUiResponse {
                value: Some("blue".to_string()),
                ..Default::default()
            }
        } else {
            ExtensionUiResponse {
                cancelled: Some(true),
                ..Default::default()
            }
        }
    });

    let result = host
        .call_tool("ask_ui", "call-1", &json!({}), &[])
        .expect("call extension tool");

    assert_eq!(
        result.content,
        vec![ContentBlock::Text {
            text: "blue".to_string(),
            text_signature: None,
        }]
    );
}
