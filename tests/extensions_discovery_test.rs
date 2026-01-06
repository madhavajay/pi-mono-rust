use pi::coding_agent::discover_extension_paths;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let mut path = env::temp_dir();
        path.push(format!("pi-ext-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn collect_names(paths: &[PathBuf]) -> Vec<String> {
    let mut names = paths
        .iter()
        .filter_map(|path| path.file_name())
        .map(|name| name.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn discovers_direct_ts_files_in_extensions_dir() {
    let temp = TempDir::new();
    let extensions_dir = temp.path.join("extensions");
    fs::create_dir_all(&extensions_dir).unwrap();
    write_file(&extensions_dir.join("foo.ts"), "export default {}");
    write_file(&extensions_dir.join("bar.ts"), "export default {}");

    let result = discover_extension_paths(&[], &temp.path, &temp.path);

    assert_eq!(collect_names(&result), vec!["bar.ts", "foo.ts"]);
}

#[test]
fn discovers_direct_js_files_in_extensions_dir() {
    let temp = TempDir::new();
    let extensions_dir = temp.path.join("extensions");
    fs::create_dir_all(&extensions_dir).unwrap();
    write_file(&extensions_dir.join("foo.js"), "module.exports = {}");

    let result = discover_extension_paths(&[], &temp.path, &temp.path);

    assert_eq!(collect_names(&result), vec!["foo.js"]);
}

#[test]
fn discovers_subdirectory_with_index_ts() {
    let temp = TempDir::new();
    let subdir = temp.path.join("extensions").join("my-extension");
    write_file(&subdir.join("index.ts"), "export default {}");

    let result = discover_extension_paths(&[], &temp.path, &temp.path);

    assert_eq!(result.len(), 1);
    let path = result[0].to_string_lossy();
    assert!(path.contains("my-extension"));
    assert!(path.contains("index.ts"));
}

#[test]
fn prefers_index_ts_over_index_js() {
    let temp = TempDir::new();
    let subdir = temp.path.join("extensions").join("my-extension");
    write_file(&subdir.join("index.ts"), "export default {}");
    write_file(&subdir.join("index.js"), "module.exports = {}");

    let result = discover_extension_paths(&[], &temp.path, &temp.path);

    assert_eq!(result.len(), 1);
    assert!(result[0].to_string_lossy().contains("index.ts"));
}

#[test]
fn discovers_subdirectory_with_package_json_pi_field() {
    let temp = TempDir::new();
    let subdir = temp.path.join("extensions").join("my-package");
    let src_dir = subdir.join("src");
    write_file(&src_dir.join("main.ts"), "export default {}");
    write_file(
        &subdir.join("package.json"),
        r#"{"name":"my-package","pi":{"extensions":["./src/main.ts"]}}"#,
    );

    let result = discover_extension_paths(&[], &temp.path, &temp.path);

    assert_eq!(result.len(), 1);
    let path = result[0].to_string_lossy();
    assert!(path.contains("src"));
    assert!(path.contains("main.ts"));
}

#[test]
fn package_json_takes_precedence_over_index() {
    let temp = TempDir::new();
    let subdir = temp.path.join("extensions").join("my-package");
    write_file(&subdir.join("index.ts"), "export default {}");
    write_file(&subdir.join("custom.ts"), "export default {}");
    write_file(
        &subdir.join("package.json"),
        r#"{"name":"my-package","pi":{"extensions":["./custom.ts"]}}"#,
    );

    let result = discover_extension_paths(&[], &temp.path, &temp.path);

    assert_eq!(result.len(), 1);
    assert!(result[0].to_string_lossy().contains("custom.ts"));
}

#[test]
fn ignores_subdirectory_without_entries() {
    let temp = TempDir::new();
    let subdir = temp.path.join("extensions").join("not-an-extension");
    write_file(&subdir.join("helper.ts"), "export default {}");
    write_file(&subdir.join("utils.ts"), "export default {}");

    let result = discover_extension_paths(&[], &temp.path, &temp.path);

    assert!(result.is_empty());
}

#[test]
fn does_not_recurse_beyond_one_level() {
    let temp = TempDir::new();
    let nested = temp
        .path
        .join("extensions")
        .join("container")
        .join("nested");
    write_file(&nested.join("index.ts"), "export default {}");

    let result = discover_extension_paths(&[], &temp.path, &temp.path);

    assert!(result.is_empty());
}

#[test]
fn resolves_configured_paths_as_files_or_directories() {
    let temp = TempDir::new();
    let extensions_dir = temp.path.join("extensions");
    fs::create_dir_all(&extensions_dir).unwrap();
    write_file(&extensions_dir.join("global.ts"), "export default {}");

    let configured_dir = temp.path.join("custom-pack");
    write_file(&configured_dir.join("index.ts"), "export default {}");

    let configured_file = temp.path.join("custom.ts");
    write_file(&configured_file, "export default {}");

    let configured = vec![
        configured_dir.to_string_lossy().to_string(),
        configured_file.to_string_lossy().to_string(),
    ];

    let result = discover_extension_paths(&configured, &temp.path, &temp.path);
    let names = collect_names(&result);

    assert!(names.contains(&"global.ts".to_string()));
    assert!(names.contains(&"index.ts".to_string()));
    assert!(names.contains(&"custom.ts".to_string()));
}
