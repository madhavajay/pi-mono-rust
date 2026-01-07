use pi::coding_agent::{
    expand_prompt_template, load_prompt_templates, LoadPromptTemplatesOptions, PromptTemplate,
};
use std::fs;
use std::path::Path;
use uuid::Uuid;

fn write_prompt(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

#[test]
fn expands_prompt_templates_with_arguments() {
    let templates = vec![PromptTemplate {
        name: "component".to_string(),
        description: "Create component (user)".to_string(),
        content: "Create a component named $1 with features: $ARGUMENTS".to_string(),
        source: "(user)".to_string(),
    }];

    let expanded = expand_prompt_template(
        "/component Button \"onClick handler\" \"disabled support\"",
        &templates,
    );
    assert_eq!(
        expanded,
        "Create a component named Button with features: Button onClick handler disabled support"
    );
}

#[test]
fn expand_prompt_template_returns_original_when_missing() {
    let templates = vec![PromptTemplate {
        name: "exists".to_string(),
        description: "Exists (user)".to_string(),
        content: "Hello".to_string(),
        source: "(user)".to_string(),
    }];

    let expanded = expand_prompt_template("/missing arg1 arg2", &templates);
    assert_eq!(expanded, "/missing arg1 arg2");
}

#[test]
fn load_prompt_templates_reads_user_and_project_prompts() {
    let root = std::env::temp_dir().join(format!("pi-prompts-{}", Uuid::new_v4()));
    let agent_dir = root.join("agent");
    let project_dir = root.join("project");

    write_prompt(
        &agent_dir.join("prompts").join("global.md"),
        "---\ndescription: Global prompt\n---\nGlobal content",
    );
    write_prompt(
        &project_dir.join(".pi").join("prompts").join("local.md"),
        "Local prompt description\nMore details",
    );

    let templates = load_prompt_templates(LoadPromptTemplatesOptions {
        cwd: Some(project_dir.clone()),
        agent_dir: Some(agent_dir.clone()),
    });

    assert!(templates.iter().any(|template| {
        template.name == "global"
            && template.description == "Global prompt (user)"
            && template.source == "(user)"
            && template.content == "Global content"
    }));

    assert!(templates.iter().any(|template| {
        template.name == "local"
            && template.description == "Local prompt description (project)"
            && template.source == "(project)"
    }));

    let _ = fs::remove_dir_all(root);
}
