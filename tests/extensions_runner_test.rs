use pi::coding_agent::{ExtensionHost, ExtensionRunner};
use serde_json::Value;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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

fn spawn_runner(paths: Vec<PathBuf>, cwd: &Path) -> ExtensionRunner {
    let (host, manifest) = ExtensionHost::spawn(&paths, cwd).expect("spawn extension host");
    ExtensionRunner::new(host, manifest)
}

#[test]
fn warns_when_extension_shortcut_conflicts_with_built_in() {
    let temp = TempDir::new("pi-runner-test");
    let extensions_dir = temp.path().join("extensions");
    fs::create_dir_all(&extensions_dir).expect("create extensions dir");
    let extension_path = write_extension(
        &extensions_dir,
        "conflict.js",
        r#"
        module.exports = function(pi) {
            pi.registerShortcut("ctrl+c", {
                description: "Conflicts with built-in",
                handler: async () => {},
            });
        };
        "#,
    );

    let mut runner = spawn_runner(vec![extension_path], temp.path());
    let shortcuts = runner.get_shortcuts();

    assert!(!shortcuts.contains_key("ctrl+c"));
    assert!(runner
        .warnings()
        .iter()
        .any(|warning| warning.contains("conflicts with built-in")));
}

#[test]
fn warns_when_two_extensions_register_same_shortcut() {
    let temp = TempDir::new("pi-runner-test");
    let extensions_dir = temp.path().join("extensions");
    fs::create_dir_all(&extensions_dir).expect("create extensions dir");
    let ext1 = write_extension(
        &extensions_dir,
        "ext1.js",
        r#"
        module.exports = function(pi) {
            pi.registerShortcut("ctrl+shift+x", {
                description: "First extension",
                handler: async () => {},
            });
        };
        "#,
    );
    let ext2 = write_extension(
        &extensions_dir,
        "ext2.js",
        r#"
        module.exports = function(pi) {
            pi.registerShortcut("ctrl+shift+x", {
                description: "Second extension",
                handler: async () => {},
            });
        };
        "#,
    );

    let mut runner = spawn_runner(vec![ext1.clone(), ext2.clone()], temp.path());
    let shortcuts = runner.get_shortcuts();

    let shortcut = shortcuts.get("ctrl+shift+x").expect("shortcut exists");
    assert_eq!(shortcut.extension_path, ext2.to_string_lossy().to_string());
    assert!(runner
        .warnings()
        .iter()
        .any(|warning| warning.contains("shortcut conflict")));
}

#[test]
fn collects_tools_from_multiple_extensions() {
    let temp = TempDir::new("pi-runner-test");
    let extensions_dir = temp.path().join("extensions");
    fs::create_dir_all(&extensions_dir).expect("create extensions dir");
    let tool_a = r#"
        module.exports = function(pi) {
            pi.registerTool({
                name: "tool_a",
                label: "tool_a",
                description: "Test tool",
            });
        };
    "#;
    let tool_b = r#"
        module.exports = function(pi) {
            pi.registerTool({
                name: "tool_b",
                label: "tool_b",
                description: "Test tool",
            });
        };
    "#;
    let ext1 = write_extension(&extensions_dir, "tool-a.js", tool_a);
    let ext2 = write_extension(&extensions_dir, "tool-b.js", tool_b);

    let runner = spawn_runner(vec![ext1, ext2], temp.path());
    let mut tools = runner.get_all_registered_tools();
    tools.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].name, "tool_a");
    assert_eq!(tools[1].name, "tool_b");
}

#[test]
fn collects_commands_from_multiple_extensions() {
    let temp = TempDir::new("pi-runner-test");
    let extensions_dir = temp.path().join("extensions");
    fs::create_dir_all(&extensions_dir).expect("create extensions dir");
    let cmd_a = r#"
        module.exports = function(pi) {
            pi.registerCommand("cmd-a", { description: "Test command" });
        };
    "#;
    let cmd_b = r#"
        module.exports = function(pi) {
            pi.registerCommand("cmd-b", { description: "Test command" });
        };
    "#;
    let ext1 = write_extension(&extensions_dir, "cmd-a.js", cmd_a);
    let ext2 = write_extension(&extensions_dir, "cmd-b.js", cmd_b);

    let runner = spawn_runner(vec![ext1, ext2], temp.path());
    let mut commands = runner.get_registered_commands();
    commands.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].name, "cmd-a");
    assert_eq!(commands[1].name, "cmd-b");
}

#[test]
fn gets_command_by_name() {
    let temp = TempDir::new("pi-runner-test");
    let extensions_dir = temp.path().join("extensions");
    fs::create_dir_all(&extensions_dir).expect("create extensions dir");
    let cmd = r#"
        module.exports = function(pi) {
            pi.registerCommand("my-cmd", { description: "My command" });
        };
    "#;
    let ext = write_extension(&extensions_dir, "cmd.js", cmd);

    let runner = spawn_runner(vec![ext], temp.path());
    let command = runner.get_command("my-cmd").expect("command exists");
    assert_eq!(command.name, "my-cmd");
    assert_eq!(command.description.as_deref(), Some("My command"));
    assert!(runner.get_command("not-exists").is_none());
}

#[test]
fn calls_error_listeners_when_handler_throws() {
    let temp = TempDir::new("pi-runner-test");
    let extensions_dir = temp.path().join("extensions");
    fs::create_dir_all(&extensions_dir).expect("create extensions dir");
    let ext = r#"
        module.exports = function(pi) {
            pi.on("context", async () => {
                throw new Error("Handler error!");
            });
        };
    "#;
    let ext_path = write_extension(&extensions_dir, "throws.js", ext);

    let mut runner = spawn_runner(vec![ext_path], temp.path());
    let errors = Rc::new(RefCell::new(Vec::new()));
    let errors_ref = Rc::clone(&errors);
    runner.on_error(move |err| errors_ref.borrow_mut().push(err.clone()));

    runner.emit_context(&[]).expect("emit context");
    let errors = errors.borrow();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].event.as_deref(), Some("context"));
    assert!(errors[0].error.contains("Handler error!"));
}

#[test]
fn gets_message_renderer_by_type() {
    let temp = TempDir::new("pi-runner-test");
    let extensions_dir = temp.path().join("extensions");
    fs::create_dir_all(&extensions_dir).expect("create extensions dir");
    let ext = r#"
        module.exports = function(pi) {
            pi.registerMessageRenderer("my-type");
        };
    "#;
    let ext_path = write_extension(&extensions_dir, "renderer.js", ext);

    let runner = spawn_runner(vec![ext_path], temp.path());
    let renderer = runner.get_message_renderer("my-type");
    assert!(renderer.is_some());
    assert!(runner.get_message_renderer("not-exists").is_none());
}

#[test]
fn collects_flags_from_extensions() {
    let temp = TempDir::new("pi-runner-test");
    let extensions_dir = temp.path().join("extensions");
    fs::create_dir_all(&extensions_dir).expect("create extensions dir");
    let ext = r#"
        module.exports = function(pi) {
            pi.registerFlag("--my-flag", {
                description: "My flag",
                type: "boolean",
            });
        };
    "#;
    let ext_path = write_extension(&extensions_dir, "with-flag.js", ext);

    let runner = spawn_runner(vec![ext_path], temp.path());
    let flags = runner.get_flags();
    assert!(flags.contains_key("--my-flag"));
}

#[test]
fn can_set_flag_values() {
    let temp = TempDir::new("pi-runner-test");
    let extensions_dir = temp.path().join("extensions");
    fs::create_dir_all(&extensions_dir).expect("create extensions dir");
    let ext = r#"
        module.exports = function(pi) {
            pi.registerFlag("--test-flag", {
                description: "Test flag",
                type: "boolean",
            });
            pi.on("context", async () => {
                if (!pi.getFlag("--test-flag")) {
                    throw new Error("Flag not set");
                }
            });
        };
    "#;
    let ext_path = write_extension(&extensions_dir, "flag.js", ext);

    let mut runner = spawn_runner(vec![ext_path], temp.path());
    runner
        .set_flag_value("--test-flag", Value::Bool(true))
        .expect("set flag value");

    let errors = Rc::new(RefCell::new(Vec::new()));
    let errors_ref = Rc::clone(&errors);
    runner.on_error(move |err| errors_ref.borrow_mut().push(err.clone()));
    runner.emit_context(&[]).expect("emit context");
    assert!(errors.borrow().is_empty());
}

#[test]
fn returns_true_when_handlers_exist() {
    let temp = TempDir::new("pi-runner-test");
    let extensions_dir = temp.path().join("extensions");
    fs::create_dir_all(&extensions_dir).expect("create extensions dir");
    let ext = r#"
        module.exports = function(pi) {
            pi.on("tool_call", async () => undefined);
        };
    "#;
    let ext_path = write_extension(&extensions_dir, "handler.js", ext);

    let runner = spawn_runner(vec![ext_path], temp.path());
    assert!(runner.has_handlers("tool_call"));
    assert!(!runner.has_handlers("agent_end"));
}
