use pi::coding_agent::ExtensionHost;
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
fn executes_extension_tool() {
    let temp = TempDir::new("pi-extension-tool");
    let extensions_dir = temp.path().join("extensions");
    fs::create_dir_all(&extensions_dir).expect("create extensions dir");
    let ext = write_extension(
        &extensions_dir,
        "tool.js",
        r#"
        module.exports = function(pi) {
            pi.registerTool({
                name: "hello_tool",
                description: "Greets someone",
                parameters: {
                    type: "object",
                    properties: { name: { type: "string" } },
                    required: ["name"],
                    additionalProperties: false,
                },
                async execute(_callId, params) {
                    return {
                        content: [{ type: "text", text: `Hello ${params.name}` }],
                        details: { greeted: params.name },
                    };
                },
            });
        };
        "#,
    );

    let (mut host, _manifest) =
        ExtensionHost::spawn(&[ext], temp.path()).expect("spawn extension host");
    let result = host
        .call_tool("hello_tool", "call-1", &json!({ "name": "Rust" }), &[])
        .expect("call extension tool");

    assert_eq!(
        result.content,
        vec![ContentBlock::Text {
            text: "Hello Rust".to_string(),
            text_signature: None,
        }]
    );
    assert_eq!(result.details, Some(json!({ "greeted": "Rust" })));
}
