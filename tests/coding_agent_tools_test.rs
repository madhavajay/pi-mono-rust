use pi::coding_agent::tools::{
    BashTool, BashToolArgs, EditTool, EditToolArgs, FindTool, FindToolArgs, GrepTool, GrepToolArgs,
    LsTool, LsToolArgs, ReadTool, ReadToolArgs, ToolResult, WriteTool, WriteToolArgs,
};
use pi::ContentBlock;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// Source: packages/coding-agent/test/tools.test.ts

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let mut path = std::env::temp_dir();
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        path.push(format!("{prefix}-{since_epoch}-{}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn get_text_output(result: &ToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<String>>()
        .join("\n")
}

fn write_lines(path: &Path, lines: &[String]) {
    fs::write(path, lines.join("\n")).expect("write file");
}

#[test]
fn should_read_file_contents_that_fit_within_limits() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("test.txt");
    let content = "Hello, world!\nLine 2\nLine 3";
    fs::write(&test_file, content).expect("write file");

    let tool = ReadTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-1",
            ReadToolArgs {
                path: test_file.to_string_lossy().to_string(),
                offset: None,
                limit: None,
            },
        )
        .expect("read tool");

    let output = get_text_output(&result);
    assert_eq!(output, content);
    assert!(!output.contains("Use offset="));
    assert!(result.details.is_none());
}

#[test]
fn should_handle_non_existent_files() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("nonexistent.txt");

    let tool = ReadTool::new(&temp.path);
    let err = tool
        .execute(
            "test-call-2",
            ReadToolArgs {
                path: test_file.to_string_lossy().to_string(),
                offset: None,
                limit: None,
            },
        )
        .expect_err("expected error");
    assert!(err.to_lowercase().contains("not found"));
}

#[test]
fn should_truncate_files_exceeding_line_limit() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("large.txt");
    let lines: Vec<String> = (0..2500).map(|i| format!("Line {}", i + 1)).collect();
    write_lines(&test_file, &lines);

    let tool = ReadTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-3",
            ReadToolArgs {
                path: test_file.to_string_lossy().to_string(),
                offset: None,
                limit: None,
            },
        )
        .expect("read tool");
    let output = get_text_output(&result);

    assert!(output.contains("Line 1"));
    assert!(output.contains("Line 2000"));
    assert!(!output.contains("Line 2001"));
    assert!(output.contains("[Showing lines 1-2000 of 2500. Use offset=2001 to continue]"));
}

#[test]
fn should_truncate_when_byte_limit_exceeded() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("large-bytes.txt");
    let lines: Vec<String> = (0..500)
        .map(|i| format!("Line {}: {}", i + 1, "x".repeat(200)))
        .collect();
    write_lines(&test_file, &lines);

    let tool = ReadTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-4",
            ReadToolArgs {
                path: test_file.to_string_lossy().to_string(),
                offset: None,
                limit: None,
            },
        )
        .expect("read tool");
    let output = get_text_output(&result);

    assert!(output.contains("Line 1:"));
    assert!(output.contains("[Showing lines 1-"));
    assert!(output.contains("of 500"));
    assert!(output.contains("limit"));
    assert!(output.contains("Use offset="));
}

#[test]
fn should_handle_offset_parameter() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("offset-test.txt");
    let lines: Vec<String> = (0..100).map(|i| format!("Line {}", i + 1)).collect();
    write_lines(&test_file, &lines);

    let tool = ReadTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-5",
            ReadToolArgs {
                path: test_file.to_string_lossy().to_string(),
                offset: Some(51),
                limit: None,
            },
        )
        .expect("read tool");
    let output = get_text_output(&result);

    assert!(!output.contains("Line 50"));
    assert!(output.contains("Line 51"));
    assert!(output.contains("Line 100"));
    assert!(!output.contains("Use offset="));
}

#[test]
fn should_handle_limit_parameter() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("limit-test.txt");
    let lines: Vec<String> = (0..100).map(|i| format!("Line {}", i + 1)).collect();
    write_lines(&test_file, &lines);

    let tool = ReadTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-6",
            ReadToolArgs {
                path: test_file.to_string_lossy().to_string(),
                offset: None,
                limit: Some(10),
            },
        )
        .expect("read tool");
    let output = get_text_output(&result);

    assert!(output.contains("Line 1"));
    assert!(output.contains("Line 10"));
    assert!(!output.contains("Line 11"));
    assert!(output.contains("[90 more lines in file. Use offset=11 to continue]"));
}

#[test]
fn should_handle_offset_limit_together() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("offset-limit-test.txt");
    let lines: Vec<String> = (0..100).map(|i| format!("Line {}", i + 1)).collect();
    write_lines(&test_file, &lines);

    let tool = ReadTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-7",
            ReadToolArgs {
                path: test_file.to_string_lossy().to_string(),
                offset: Some(41),
                limit: Some(20),
            },
        )
        .expect("read tool");
    let output = get_text_output(&result);

    assert!(!output.contains("Line 40"));
    assert!(output.contains("Line 41"));
    assert!(output.contains("Line 60"));
    assert!(!output.contains("Line 61"));
    assert!(output.contains("[40 more lines in file. Use offset=61 to continue]"));
}

#[test]
fn should_show_error_when_offset_is_beyond_file_length() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("short.txt");
    fs::write(&test_file, "Line 1\nLine 2\nLine 3").expect("write file");

    let tool = ReadTool::new(&temp.path);
    let err = tool
        .execute(
            "test-call-8",
            ReadToolArgs {
                path: test_file.to_string_lossy().to_string(),
                offset: Some(100),
                limit: None,
            },
        )
        .expect_err("expected error");
    assert!(err.contains("Offset 100 is beyond end of file (3 lines total)"));
}

#[test]
fn should_include_truncation_details_when_truncated() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("large-file.txt");
    let lines: Vec<String> = (0..2500).map(|i| format!("Line {}", i + 1)).collect();
    write_lines(&test_file, &lines);

    let tool = ReadTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-9",
            ReadToolArgs {
                path: test_file.to_string_lossy().to_string(),
                offset: None,
                limit: None,
            },
        )
        .expect("read tool");

    let details = result.details.expect("details");
    let truncation = details.get("truncation").expect("truncation");
    assert_eq!(
        truncation.get("truncated").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        truncation.get("truncatedBy").and_then(|v| v.as_str()),
        Some("lines")
    );
    assert_eq!(
        truncation.get("totalLines").and_then(|v| v.as_i64()),
        Some(2500)
    );
    assert_eq!(
        truncation.get("outputLines").and_then(|v| v.as_i64()),
        Some(2000)
    );
}

#[test]
fn should_detect_image_mime_type_from_file_magic_not_extension() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("image.txt");
    let png_bytes: [u8; 68] = [
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4,
        0, 0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 252, 255, 31, 0, 3, 3,
        2, 0, 239, 151, 217, 157, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    fs::write(&test_file, png_bytes).expect("write image");

    let tool = ReadTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-img-1",
            ReadToolArgs {
                path: test_file.to_string_lossy().to_string(),
                offset: None,
                limit: None,
            },
        )
        .expect("read tool");

    let output = get_text_output(&result);
    assert!(output.contains("Read image file [image/png]"));

    let mut image_block = None;
    for block in &result.content {
        if let ContentBlock::Image { data, mime_type } = block {
            image_block = Some((data, mime_type));
        }
    }

    let (data, mime_type) = image_block.expect("image block");
    assert_eq!(mime_type, "image/png");
    assert!(!data.is_empty());
}

#[test]
fn should_treat_files_with_image_extension_but_non_image_content_as_text() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("not-an-image.png");
    fs::write(&test_file, "definitely not a png").expect("write file");

    let tool = ReadTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-img-2",
            ReadToolArgs {
                path: test_file.to_string_lossy().to_string(),
                offset: None,
                limit: None,
            },
        )
        .expect("read tool");
    let output = get_text_output(&result);

    assert!(output.contains("definitely not a png"));
    assert!(!result
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::Image { .. })));
}

#[test]
fn should_write_file_contents() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("write-test.txt");
    let content = "Test content";

    let tool = WriteTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-3",
            WriteToolArgs {
                path: test_file.to_string_lossy().to_string(),
                content: content.to_string(),
            },
        )
        .expect("write tool");

    let output = get_text_output(&result);
    assert!(output.contains("Successfully wrote"));
    assert!(output.contains(test_file.to_string_lossy().as_ref()));
    assert!(result.details.is_none());
}

#[test]
fn should_create_parent_directories() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("nested/dir/test.txt");
    let content = "Nested content";

    let tool = WriteTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-4",
            WriteToolArgs {
                path: test_file.to_string_lossy().to_string(),
                content: content.to_string(),
            },
        )
        .expect("write tool");

    let output = get_text_output(&result);
    assert!(output.contains("Successfully wrote"));
}

#[test]
fn should_replace_text_in_file() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("edit-test.txt");
    fs::write(&test_file, "Hello, world!").expect("write file");

    let tool = EditTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-5",
            EditToolArgs {
                path: test_file.to_string_lossy().to_string(),
                old_text: "world".to_string(),
                new_text: "testing".to_string(),
            },
        )
        .expect("edit tool");

    let output = get_text_output(&result);
    assert!(output.contains("Successfully replaced"));
    let details = result.details.expect("details");
    let diff = details.get("diff").and_then(|v| v.as_str()).unwrap_or("");
    assert!(diff.contains("testing"));
}

#[test]
fn should_fail_if_text_not_found() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("edit-test.txt");
    fs::write(&test_file, "Hello, world!").expect("write file");

    let tool = EditTool::new(&temp.path);
    let err = tool
        .execute(
            "test-call-6",
            EditToolArgs {
                path: test_file.to_string_lossy().to_string(),
                old_text: "nonexistent".to_string(),
                new_text: "testing".to_string(),
            },
        )
        .expect_err("expected error");

    assert!(err.contains("Could not find the exact text"));
}

#[test]
fn should_fail_if_text_appears_multiple_times() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("edit-test.txt");
    fs::write(&test_file, "foo foo foo").expect("write file");

    let tool = EditTool::new(&temp.path);
    let err = tool
        .execute(
            "test-call-7",
            EditToolArgs {
                path: test_file.to_string_lossy().to_string(),
                old_text: "foo".to_string(),
                new_text: "bar".to_string(),
            },
        )
        .expect_err("expected error");

    assert!(err.contains("Found 3 occurrences"));
}

#[test]
fn should_execute_simple_commands() {
    let temp = TempDir::new("coding-agent-test");
    let tool = BashTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-8",
            BashToolArgs {
                command: "echo 'test output'".to_string(),
                timeout: None,
            },
        )
        .expect("bash tool");

    let output = get_text_output(&result);
    assert!(output.contains("test output"));
    assert!(result.details.is_none());
}

#[test]
fn should_handle_command_errors() {
    let temp = TempDir::new("coding-agent-test");
    let tool = BashTool::new(&temp.path);
    let err = tool
        .execute(
            "test-call-9",
            BashToolArgs {
                command: "exit 1".to_string(),
                timeout: None,
            },
        )
        .expect_err("expected error");

    assert!(err.contains("Command failed") || err.contains("code 1"));
}

#[test]
fn should_respect_timeout() {
    let temp = TempDir::new("coding-agent-test");
    let tool = BashTool::new(&temp.path);
    let err = tool
        .execute(
            "test-call-10",
            BashToolArgs {
                command: "sleep 5".to_string(),
                timeout: Some(1),
            },
        )
        .expect_err("expected error");

    assert!(err.to_lowercase().contains("timed out"));
}

#[test]
fn should_include_filename_when_searching_a_single_file() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("example.txt");
    fs::write(&test_file, "first line\nmatch line\nlast line").expect("write file");

    let tool = GrepTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-11",
            GrepToolArgs {
                pattern: "match".to_string(),
                path: Some(test_file.to_string_lossy().to_string()),
                glob: None,
                ignore_case: None,
                literal: None,
                context: None,
                limit: None,
            },
        )
        .expect("grep tool");

    let output = get_text_output(&result);
    assert!(output.contains("example.txt:2: match line"));
}

#[test]
fn should_respect_global_limit_and_include_context_lines() {
    let temp = TempDir::new("coding-agent-test");
    let test_file = temp.join("context.txt");
    let content = [
        "before",
        "match one",
        "after",
        "middle",
        "match two",
        "after two",
    ]
    .join("\n");
    fs::write(&test_file, content).expect("write file");

    let tool = GrepTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-12",
            GrepToolArgs {
                pattern: "match".to_string(),
                path: Some(test_file.to_string_lossy().to_string()),
                glob: None,
                ignore_case: None,
                literal: None,
                context: Some(1),
                limit: Some(1),
            },
        )
        .expect("grep tool");

    let output = get_text_output(&result);
    assert!(output.contains("context.txt-1- before"));
    assert!(output.contains("context.txt:2: match one"));
    assert!(output.contains("context.txt-3- after"));
    assert!(output.contains("[1 matches limit reached. Use limit=2 for more, or refine pattern]"));
    assert!(!output.contains("match two"));
}

#[test]
fn should_include_hidden_files_that_are_not_gitignored() {
    let temp = TempDir::new("coding-agent-test");
    let hidden_dir = temp.join(".secret");
    fs::create_dir_all(&hidden_dir).expect("mkdir");
    fs::write(hidden_dir.join("hidden.txt"), "hidden").expect("write file");
    fs::write(temp.join("visible.txt"), "visible").expect("write file");

    let tool = FindTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-13",
            FindToolArgs {
                pattern: "**/*.txt".to_string(),
                path: Some(temp.path.to_string_lossy().to_string()),
                limit: None,
            },
        )
        .expect("find tool");

    let output_lines: Vec<String> = get_text_output(&result)
        .split('\n')
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    assert!(output_lines.contains(&"visible.txt".to_string()));
    assert!(output_lines.contains(&".secret/hidden.txt".to_string()));
}

#[test]
fn should_respect_gitignore() {
    let temp = TempDir::new("coding-agent-test");
    fs::write(temp.join(".gitignore"), "ignored.txt\n").expect("write gitignore");
    fs::write(temp.join("ignored.txt"), "ignored").expect("write file");
    fs::write(temp.join("kept.txt"), "kept").expect("write file");

    let tool = FindTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-14",
            FindToolArgs {
                pattern: "**/*.txt".to_string(),
                path: Some(temp.path.to_string_lossy().to_string()),
                limit: None,
            },
        )
        .expect("find tool");

    let output = get_text_output(&result);
    assert!(output.contains("kept.txt"));
    assert!(!output.contains("ignored.txt"));
}

#[test]
fn should_list_dotfiles_and_directories() {
    let temp = TempDir::new("coding-agent-test");
    fs::write(temp.join(".hidden-file"), "secret").expect("write file");
    fs::create_dir_all(temp.join(".hidden-dir")).expect("mkdir");

    let tool = LsTool::new(&temp.path);
    let result = tool
        .execute(
            "test-call-15",
            LsToolArgs {
                path: Some(temp.path.to_string_lossy().to_string()),
                limit: None,
            },
        )
        .expect("ls tool");

    let output = get_text_output(&result);
    assert!(output.contains(".hidden-file"));
    assert!(output.contains(".hidden-dir/"));
}

#[test]
fn should_match_lf_oldtext_against_crlf_file_content() {
    let temp = TempDir::new("coding-agent-crlf-test");
    let test_file = temp.join("crlf-test.txt");
    fs::write(&test_file, "line one\r\nline two\r\nline three\r\n").expect("write file");

    let tool = EditTool::new(&temp.path);
    let result = tool
        .execute(
            "test-crlf-1",
            EditToolArgs {
                path: test_file.to_string_lossy().to_string(),
                old_text: "line two\n".to_string(),
                new_text: "replaced line\n".to_string(),
            },
        )
        .expect("edit tool");

    let output = get_text_output(&result);
    assert!(output.contains("Successfully replaced"));
}

#[test]
fn should_preserve_crlf_line_endings_after_edit() {
    let temp = TempDir::new("coding-agent-crlf-test");
    let test_file = temp.join("crlf-preserve.txt");
    fs::write(&test_file, "first\r\nsecond\r\nthird\r\n").expect("write file");

    let tool = EditTool::new(&temp.path);
    tool.execute(
        "test-crlf-2",
        EditToolArgs {
            path: test_file.to_string_lossy().to_string(),
            old_text: "second\n".to_string(),
            new_text: "REPLACED\n".to_string(),
        },
    )
    .expect("edit tool");

    let content = fs::read_to_string(&test_file).expect("read file");
    assert_eq!(content, "first\r\nREPLACED\r\nthird\r\n");
}

#[test]
fn should_preserve_lf_line_endings_for_lf_files() {
    let temp = TempDir::new("coding-agent-crlf-test");
    let test_file = temp.join("lf-preserve.txt");
    fs::write(&test_file, "first\nsecond\nthird\n").expect("write file");

    let tool = EditTool::new(&temp.path);
    tool.execute(
        "test-lf-1",
        EditToolArgs {
            path: test_file.to_string_lossy().to_string(),
            old_text: "second\n".to_string(),
            new_text: "REPLACED\n".to_string(),
        },
    )
    .expect("edit tool");

    let content = fs::read_to_string(&test_file).expect("read file");
    assert_eq!(content, "first\nREPLACED\nthird\n");
}

#[test]
fn should_detect_duplicates_across_crlf_lf_variants() {
    let temp = TempDir::new("coding-agent-crlf-test");
    let test_file = temp.join("mixed-endings.txt");
    fs::write(&test_file, "hello\r\nworld\r\n---\r\nhello\nworld\n").expect("write file");

    let tool = EditTool::new(&temp.path);
    let err = tool
        .execute(
            "test-crlf-dup",
            EditToolArgs {
                path: test_file.to_string_lossy().to_string(),
                old_text: "hello\nworld\n".to_string(),
                new_text: "replaced\n".to_string(),
            },
        )
        .expect_err("expected error");

    assert!(err.contains("Found 2 occurrences"));
}

#[test]
fn should_preserve_utf_8_bom_after_edit() {
    let temp = TempDir::new("coding-agent-crlf-test");
    let test_file = temp.join("bom-test.txt");
    fs::write(&test_file, "\u{feff}first\r\nsecond\r\nthird\r\n").expect("write file");

    let tool = EditTool::new(&temp.path);
    tool.execute(
        "test-bom",
        EditToolArgs {
            path: test_file.to_string_lossy().to_string(),
            old_text: "second\n".to_string(),
            new_text: "REPLACED\n".to_string(),
        },
    )
    .expect("edit tool");

    let content = fs::read_to_string(&test_file).expect("read file");
    assert_eq!(content, "\u{feff}first\r\nREPLACED\r\nthird\r\n");
}
