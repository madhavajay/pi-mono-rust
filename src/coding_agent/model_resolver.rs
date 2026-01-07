use crate::coding_agent::model_registry::Model;

#[derive(Clone, Debug, PartialEq)]
pub struct ScopedModel {
    pub model: Model,
    pub thinking_level: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedModelResult {
    pub model: Option<Model>,
    pub thinking_level: String,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InitialModelResult {
    pub model: Option<Model>,
    pub thinking_level: String,
    pub fallback_message: Option<String>,
}

pub fn parse_model_pattern(pattern: &str, available_models: &[Model]) -> ParsedModelResult {
    if let Some(model) = try_match_model(pattern, available_models) {
        return ParsedModelResult {
            model: Some(model),
            thinking_level: "off".to_string(),
            warning: None,
        };
    }

    if let Some(last_colon) = pattern.rfind(':') {
        let prefix = &pattern[..last_colon];
        let suffix = &pattern[last_colon + 1..];
        if is_valid_thinking_level(suffix) {
            let result = parse_model_pattern(prefix, available_models);
            if result.model.is_some() && result.warning.is_none() {
                return ParsedModelResult {
                    model: result.model,
                    thinking_level: suffix.to_string(),
                    warning: None,
                };
            }
            return result;
        }

        let result = parse_model_pattern(prefix, available_models);
        if result.model.is_some() {
            return ParsedModelResult {
                model: result.model,
                thinking_level: "off".to_string(),
                warning: Some(format!(
                    "Invalid thinking level \"{}\" in pattern \"{}\". Using \"off\" instead.",
                    suffix, pattern
                )),
            };
        }
        return result;
    }

    ParsedModelResult {
        model: None,
        thinking_level: "off".to_string(),
        warning: None,
    }
}

pub fn resolve_model_scope(patterns: &[String], available_models: &[Model]) -> Vec<ScopedModel> {
    let mut scoped = Vec::new();
    for pattern in patterns {
        if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
            let (glob_pattern, thinking_level) = parse_glob_pattern(pattern);
            let matched = available_models
                .iter()
                .filter(|model| {
                    let full_id = format!("{}/{}", model.provider, model.id);
                    glob_match(&full_id, &glob_pattern) || glob_match(&model.id, &glob_pattern)
                })
                .cloned()
                .collect::<Vec<_>>();
            for model in matched {
                if !scoped
                    .iter()
                    .any(|item: &ScopedModel| models_equal(&item.model, &model))
                {
                    scoped.push(ScopedModel {
                        model,
                        thinking_level: thinking_level.clone(),
                    });
                }
            }
            continue;
        }

        let result = parse_model_pattern(pattern, available_models);
        if let Some(model) = result.model {
            if !scoped
                .iter()
                .any(|item: &ScopedModel| models_equal(&item.model, &model))
            {
                scoped.push(ScopedModel {
                    model,
                    thinking_level: result.thinking_level,
                });
            }
        }
    }
    scoped
}

fn parse_glob_pattern(pattern: &str) -> (String, String) {
    if let Some(idx) = pattern.rfind(':') {
        let prefix = &pattern[..idx];
        let suffix = &pattern[idx + 1..];
        if is_valid_thinking_level(suffix) {
            return (prefix.to_string(), suffix.to_string());
        }
    }
    (pattern.to_string(), "off".to_string())
}

fn try_match_model(pattern: &str, available_models: &[Model]) -> Option<Model> {
    if let Some((provider, model_id)) = split_provider_model(pattern) {
        if let Some(found) = available_models.iter().find(|model| {
            model.provider.eq_ignore_ascii_case(provider) && model.id.eq_ignore_ascii_case(model_id)
        }) {
            return Some(found.clone());
        }
    }

    if let Some(found) = available_models
        .iter()
        .find(|model| model.id.eq_ignore_ascii_case(pattern))
    {
        return Some(found.clone());
    }

    let matches: Vec<Model> = available_models
        .iter()
        .filter(|model| {
            model.id.to_lowercase().contains(&pattern.to_lowercase())
                || model.name.to_lowercase().contains(&pattern.to_lowercase())
        })
        .cloned()
        .collect();

    if matches.is_empty() {
        return None;
    }

    let mut aliases: Vec<Model> = matches
        .iter()
        .filter(|&model| is_alias(&model.id))
        .cloned()
        .collect();
    if !aliases.is_empty() {
        aliases.sort_by(|a, b| b.id.cmp(&a.id));
        return aliases.into_iter().next();
    }

    let mut dated = matches;
    dated.sort_by(|a, b| b.id.cmp(&a.id));
    dated.into_iter().next()
}

fn split_provider_model(pattern: &str) -> Option<(&str, &str)> {
    let idx = pattern.find('/')?;
    let provider = &pattern[..idx];
    let model_id = &pattern[idx + 1..];
    if provider.is_empty() || model_id.is_empty() {
        None
    } else {
        Some((provider, model_id))
    }
}

fn is_alias(model_id: &str) -> bool {
    if model_id.ends_with("-latest") {
        return true;
    }
    let date_pattern = model_id.rsplit_once('-').map(|(_, suffix)| suffix);
    match date_pattern {
        Some(suffix) => !suffix.chars().all(|ch| ch.is_ascii_digit()) || suffix.len() != 8,
        None => true,
    }
}

fn is_valid_thinking_level(level: &str) -> bool {
    matches!(
        level,
        "off" | "minimal" | "low" | "medium" | "high" | "xhigh"
    )
}

fn models_equal(a: &Model, b: &Model) -> bool {
    a.provider == b.provider && a.id == b.id
}

fn glob_match(value: &str, pattern: &str) -> bool {
    glob::Pattern::new(pattern)
        .map(|pattern| pattern.matches(value))
        .unwrap_or(false)
}
