use pi::coding_agent::{
    format_skills_for_prompt, load_skills, load_skills_from_dir, LoadSkillsFromDirOptions,
    LoadSkillsOptions, Skill,
};
use std::env;
use std::path::PathBuf;

// Source: packages/coding-agent/test/skills.test.ts

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let monorepo_root = manifest_dir.join("pi-mono");
    if monorepo_root.join("packages").exists() {
        return monorepo_root;
    }
    manifest_dir.parent().unwrap_or(&manifest_dir).to_path_buf()
}

fn fixtures_dir() -> PathBuf {
    repo_root()
        .join("packages")
        .join("coding-agent")
        .join("test")
        .join("fixtures")
        .join("skills")
}

fn collision_fixtures_dir() -> PathBuf {
    repo_root()
        .join("packages")
        .join("coding-agent")
        .join("test")
        .join("fixtures")
        .join("skills-collision")
}

#[test]
fn should_load_a_valid_skill() {
    let result = load_skills_from_dir(LoadSkillsFromDirOptions {
        dir: fixtures_dir().join("valid-skill"),
        source: "test".to_string(),
    });

    assert_eq!(result.skills.len(), 1);
    assert_eq!(result.skills[0].name, "valid-skill");
    assert_eq!(
        result.skills[0].description,
        "A valid skill for testing purposes."
    );
    assert_eq!(result.skills[0].source, "test");
    assert!(result.warnings.is_empty());
}

#[test]
fn should_warn_when_name_doesn_t_match_parent_directory() {
    let result = load_skills_from_dir(LoadSkillsFromDirOptions {
        dir: fixtures_dir().join("name-mismatch"),
        source: "test".to_string(),
    });

    assert_eq!(result.skills.len(), 1);
    assert_eq!(result.skills[0].name, "different-name");
    assert!(result
        .warnings
        .iter()
        .any(|w| w.message.contains("does not match parent directory")));
}

#[test]
fn should_warn_when_name_contains_invalid_characters() {
    let result = load_skills_from_dir(LoadSkillsFromDirOptions {
        dir: fixtures_dir().join("invalid-name-chars"),
        source: "test".to_string(),
    });

    assert_eq!(result.skills.len(), 1);
    assert!(result
        .warnings
        .iter()
        .any(|w| w.message.contains("invalid characters")));
}

#[test]
fn should_warn_when_name_exceeds_64_characters() {
    let result = load_skills_from_dir(LoadSkillsFromDirOptions {
        dir: fixtures_dir().join("long-name"),
        source: "test".to_string(),
    });

    assert_eq!(result.skills.len(), 1);
    assert!(result
        .warnings
        .iter()
        .any(|w| w.message.contains("exceeds 64 characters")));
}

#[test]
fn should_warn_and_skip_skill_when_description_is_missing() {
    let result = load_skills_from_dir(LoadSkillsFromDirOptions {
        dir: fixtures_dir().join("missing-description"),
        source: "test".to_string(),
    });

    assert!(result.skills.is_empty());
    assert!(result
        .warnings
        .iter()
        .any(|w| w.message.contains("description is required")));
}

#[test]
fn should_warn_when_unknown_frontmatter_fields_are_present() {
    let result = load_skills_from_dir(LoadSkillsFromDirOptions {
        dir: fixtures_dir().join("unknown-field"),
        source: "test".to_string(),
    });

    assert_eq!(result.skills.len(), 1);
    assert!(result
        .warnings
        .iter()
        .any(|w| w.message.contains("unknown frontmatter field \"author\"")));
    assert!(result
        .warnings
        .iter()
        .any(|w| w.message.contains("unknown frontmatter field \"version\"")));
}

#[test]
fn should_load_nested_skills_recursively() {
    let result = load_skills_from_dir(LoadSkillsFromDirOptions {
        dir: fixtures_dir().join("nested"),
        source: "test".to_string(),
    });

    assert_eq!(result.skills.len(), 1);
    assert_eq!(result.skills[0].name, "child-skill");
    assert!(result.warnings.is_empty());
}

#[test]
fn should_skip_files_without_frontmatter() {
    let result = load_skills_from_dir(LoadSkillsFromDirOptions {
        dir: fixtures_dir().join("no-frontmatter"),
        source: "test".to_string(),
    });

    assert!(result.skills.is_empty());
    assert!(result
        .warnings
        .iter()
        .any(|w| w.message.contains("description is required")));
}

#[test]
fn should_warn_when_name_contains_consecutive_hyphens() {
    let result = load_skills_from_dir(LoadSkillsFromDirOptions {
        dir: fixtures_dir().join("consecutive-hyphens"),
        source: "test".to_string(),
    });

    assert_eq!(result.skills.len(), 1);
    assert!(result
        .warnings
        .iter()
        .any(|w| w.message.contains("consecutive hyphens")));
}

#[test]
fn should_load_all_skills_from_fixture_directory() {
    let result = load_skills_from_dir(LoadSkillsFromDirOptions {
        dir: fixtures_dir(),
        source: "test".to_string(),
    });

    assert!(result.skills.len() >= 6);
}

#[test]
fn should_return_empty_for_non_existent_directory() {
    let result = load_skills_from_dir(LoadSkillsFromDirOptions {
        dir: PathBuf::from("/non/existent/path"),
        source: "test".to_string(),
    });

    assert!(result.skills.is_empty());
    assert!(result.warnings.is_empty());
}

#[test]
fn should_use_parent_directory_name_when_name_not_in_frontmatter() {
    let result = load_skills_from_dir(LoadSkillsFromDirOptions {
        dir: fixtures_dir().join("valid-skill"),
        source: "test".to_string(),
    });

    assert_eq!(result.skills.len(), 1);
    assert_eq!(result.skills[0].name, "valid-skill");
}

#[test]
fn should_return_empty_string_for_no_skills() {
    let result = format_skills_for_prompt(&[]);
    assert_eq!(result, "");
}

#[test]
fn should_format_skills_as_xml() {
    let skills = vec![Skill {
        name: "test-skill".to_string(),
        description: "A test skill.".to_string(),
        file_path: "/path/to/skill/SKILL.md".to_string(),
        base_dir: "/path/to/skill".to_string(),
        source: "test".to_string(),
    }];

    let result = format_skills_for_prompt(&skills);

    assert!(result.contains("<available_skills>"));
    assert!(result.contains("</available_skills>"));
    assert!(result.contains("<skill>"));
    assert!(result.contains("<name>test-skill</name>"));
    assert!(result.contains("<description>A test skill.</description>"));
    assert!(result.contains("<location>/path/to/skill/SKILL.md</location>"));
}

#[test]
fn should_include_intro_text_before_xml() {
    let skills = vec![Skill {
        name: "test-skill".to_string(),
        description: "A test skill.".to_string(),
        file_path: "/path/to/skill/SKILL.md".to_string(),
        base_dir: "/path/to/skill".to_string(),
        source: "test".to_string(),
    }];

    let result = format_skills_for_prompt(&skills);
    let xml_start = result.find("<available_skills>").unwrap_or(0);
    let intro_text = &result[..xml_start];

    assert!(intro_text.contains("The following skills provide specialized instructions"));
    assert!(intro_text.contains("Use the read tool to load a skill's file"));
}

#[test]
fn should_escape_xml_special_characters() {
    let skills = vec![Skill {
        name: "test-skill".to_string(),
        description: "A skill with <special> & \"characters\".".to_string(),
        file_path: "/path/to/skill/SKILL.md".to_string(),
        base_dir: "/path/to/skill".to_string(),
        source: "test".to_string(),
    }];

    let result = format_skills_for_prompt(&skills);

    assert!(result.contains("&lt;special&gt;"));
    assert!(result.contains("&amp;"));
    assert!(result.contains("&quot;characters&quot;"));
}

#[test]
fn should_format_multiple_skills() {
    let skills = vec![
        Skill {
            name: "skill-one".to_string(),
            description: "First skill.".to_string(),
            file_path: "/path/one/SKILL.md".to_string(),
            base_dir: "/path/one".to_string(),
            source: "test".to_string(),
        },
        Skill {
            name: "skill-two".to_string(),
            description: "Second skill.".to_string(),
            file_path: "/path/two/SKILL.md".to_string(),
            base_dir: "/path/two".to_string(),
            source: "test".to_string(),
        },
    ];

    let result = format_skills_for_prompt(&skills);

    assert!(result.contains("<name>skill-one</name>"));
    assert!(result.contains("<name>skill-two</name>"));
    assert_eq!(result.matches("<skill>").count(), 2);
}

#[test]
fn should_load_from_customdirectories_only_when_built_ins_disabled() {
    let mut options = LoadSkillsOptions::new();
    options.enable_codex_user = false;
    options.enable_claude_user = false;
    options.enable_claude_project = false;
    options.enable_pi_user = false;
    options.enable_pi_project = false;
    options.custom_directories = vec![fixtures_dir().to_string_lossy().to_string()];

    let result = load_skills(options);
    assert!(!result.skills.is_empty());
    assert!(result.skills.iter().all(|s| s.source == "custom"));
}

#[test]
fn should_filter_out_ignoredskills() {
    let mut options = LoadSkillsOptions::new();
    options.enable_codex_user = false;
    options.enable_claude_user = false;
    options.enable_claude_project = false;
    options.enable_pi_user = false;
    options.enable_pi_project = false;
    options.custom_directories = vec![fixtures_dir()
        .join("valid-skill")
        .to_string_lossy()
        .to_string()];
    options.ignored_skills = vec!["valid-skill".to_string()];

    let result = load_skills(options);
    assert!(result.skills.is_empty());
}

#[test]
fn should_support_glob_patterns_in_ignoredskills() {
    let mut options = LoadSkillsOptions::new();
    options.enable_codex_user = false;
    options.enable_claude_user = false;
    options.enable_claude_project = false;
    options.enable_pi_user = false;
    options.enable_pi_project = false;
    options.custom_directories = vec![fixtures_dir().to_string_lossy().to_string()];
    options.ignored_skills = vec!["valid-*".to_string()];

    let result = load_skills(options);
    assert!(result.skills.iter().all(|s| !s.name.starts_with("valid-")));
}

#[test]
fn should_have_ignoredskills_take_precedence_over_includeskills() {
    let mut options = LoadSkillsOptions::new();
    options.enable_codex_user = false;
    options.enable_claude_user = false;
    options.enable_claude_project = false;
    options.enable_pi_user = false;
    options.enable_pi_project = false;
    options.custom_directories = vec![fixtures_dir().to_string_lossy().to_string()];
    options.include_skills = vec!["valid-*".to_string()];
    options.ignored_skills = vec!["valid-skill".to_string()];

    let result = load_skills(options);
    assert!(result.skills.iter().all(|s| s.name != "valid-skill"));
}

#[test]
fn should_expand_in_customdirectories() {
    let home = env::var("HOME").unwrap_or_default();
    let home_skills_dir = PathBuf::from(&home)
        .join(".pi")
        .join("agent")
        .join("skills");

    let mut with_tilde = LoadSkillsOptions::new();
    with_tilde.enable_codex_user = false;
    with_tilde.enable_claude_user = false;
    with_tilde.enable_claude_project = false;
    with_tilde.enable_pi_user = false;
    with_tilde.enable_pi_project = false;
    with_tilde.custom_directories = vec!["~/.pi/agent/skills".to_string()];

    let mut without_tilde = LoadSkillsOptions::new();
    without_tilde.enable_codex_user = false;
    without_tilde.enable_claude_user = false;
    without_tilde.enable_claude_project = false;
    without_tilde.enable_pi_user = false;
    without_tilde.enable_pi_project = false;
    without_tilde.custom_directories = vec![home_skills_dir.to_string_lossy().to_string()];

    let result_with_tilde = load_skills(with_tilde);
    let result_without_tilde = load_skills(without_tilde);

    assert_eq!(
        result_with_tilde.skills.len(),
        result_without_tilde.skills.len()
    );
}

#[test]
fn should_return_empty_when_all_sources_disabled_and_no_custom_dirs() {
    let mut options = LoadSkillsOptions::new();
    options.enable_codex_user = false;
    options.enable_claude_user = false;
    options.enable_claude_project = false;
    options.enable_pi_user = false;
    options.enable_pi_project = false;

    let result = load_skills(options);
    assert!(result.skills.is_empty());
}

#[test]
fn should_filter_skills_with_includeskills_glob_patterns() {
    let mut all_options = LoadSkillsOptions::new();
    all_options.enable_codex_user = false;
    all_options.enable_claude_user = false;
    all_options.enable_claude_project = false;
    all_options.enable_pi_user = false;
    all_options.enable_pi_project = false;
    all_options.custom_directories = vec![fixtures_dir().to_string_lossy().to_string()];

    let all_skills = load_skills(all_options);
    assert!(!all_skills.skills.is_empty());

    let mut filtered_options = LoadSkillsOptions::new();
    filtered_options.enable_codex_user = false;
    filtered_options.enable_claude_user = false;
    filtered_options.enable_claude_project = false;
    filtered_options.enable_pi_user = false;
    filtered_options.enable_pi_project = false;
    filtered_options.custom_directories = vec![fixtures_dir().to_string_lossy().to_string()];
    filtered_options.include_skills = vec!["valid-skill".to_string()];

    let filtered = load_skills(filtered_options);
    assert_eq!(filtered.skills.len(), 1);
    assert_eq!(filtered.skills[0].name, "valid-skill");
}

#[test]
fn should_support_glob_patterns_in_includeskills() {
    let mut options = LoadSkillsOptions::new();
    options.enable_codex_user = false;
    options.enable_claude_user = false;
    options.enable_claude_project = false;
    options.enable_pi_user = false;
    options.enable_pi_project = false;
    options.custom_directories = vec![fixtures_dir().to_string_lossy().to_string()];
    options.include_skills = vec!["valid-*".to_string()];

    let result = load_skills(options);
    assert!(!result.skills.is_empty());
    assert!(result.skills.iter().all(|s| s.name.starts_with("valid-")));
}

#[test]
fn should_return_all_skills_when_includeskills_is_empty() {
    let mut with_empty = LoadSkillsOptions::new();
    with_empty.enable_codex_user = false;
    with_empty.enable_claude_user = false;
    with_empty.enable_claude_project = false;
    with_empty.enable_pi_user = false;
    with_empty.enable_pi_project = false;
    with_empty.custom_directories = vec![fixtures_dir().to_string_lossy().to_string()];
    with_empty.include_skills = Vec::new();

    let mut without_option = LoadSkillsOptions::new();
    without_option.enable_codex_user = false;
    without_option.enable_claude_user = false;
    without_option.enable_claude_project = false;
    without_option.enable_pi_user = false;
    without_option.enable_pi_project = false;
    without_option.custom_directories = vec![fixtures_dir().to_string_lossy().to_string()];

    let with_empty_result = load_skills(with_empty);
    let without_option_result = load_skills(without_option);

    assert_eq!(
        with_empty_result.skills.len(),
        without_option_result.skills.len()
    );
}

#[test]
fn should_detect_name_collisions_and_keep_first_skill() {
    let first = load_skills_from_dir(LoadSkillsFromDirOptions {
        dir: collision_fixtures_dir().join("first"),
        source: "first".to_string(),
    });
    let second = load_skills_from_dir(LoadSkillsFromDirOptions {
        dir: collision_fixtures_dir().join("second"),
        source: "second".to_string(),
    });

    let mut skill_map: std::collections::HashMap<String, Skill> = std::collections::HashMap::new();
    let mut collision_warnings: Vec<String> = Vec::new();

    for skill in first.skills {
        skill_map.insert(skill.name.clone(), skill);
    }

    for skill in second.skills {
        if let Some(existing) = skill_map.get(&skill.name) {
            collision_warnings.push(format!(
                "name collision: \"{}\" already loaded from {}",
                skill.name, existing.file_path
            ));
        } else {
            skill_map.insert(skill.name.clone(), skill);
        }
    }

    assert_eq!(skill_map.len(), 1);
    assert_eq!(skill_map.get("calendar").unwrap().source, "first");
    assert_eq!(collision_warnings.len(), 1);
    assert!(collision_warnings[0].contains("name collision"));
}
