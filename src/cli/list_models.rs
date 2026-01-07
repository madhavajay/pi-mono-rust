use crate::coding_agent::fuzzy::fuzzy_filter;
use crate::coding_agent::model_registry::{Model, ModelRegistry};

struct ModelRow {
    provider: String,
    model: String,
    context: String,
    max_out: String,
    thinking: String,
    images: String,
}

struct ColumnWidths {
    provider: usize,
    model: usize,
    context: usize,
    max_out: usize,
    thinking: usize,
    images: usize,
}

pub fn list_models(model_registry: &ModelRegistry, search_pattern: Option<&str>) {
    let mut models = model_registry.get_available();

    if models.is_empty() {
        println!("No models available. Set API keys in environment variables.");
        return;
    }

    if let Some(pattern) = search_pattern {
        let annotated = models
            .into_iter()
            .map(|model| {
                let text = format!("{} {}", model.provider, model.id);
                (model, text)
            })
            .collect::<Vec<_>>();
        let filtered = fuzzy_filter(&annotated, pattern, |entry| entry.1.as_str());
        models = filtered.into_iter().map(|(model, _)| model).collect();
        if models.is_empty() {
            println!("No models matching \"{pattern}\"");
            return;
        }
    }

    models.sort_by(|a, b| {
        let provider_cmp = a.provider.cmp(&b.provider);
        if provider_cmp != std::cmp::Ordering::Equal {
            return provider_cmp;
        }
        a.id.cmp(&b.id)
    });

    let rows = models.iter().map(model_row).collect::<Vec<_>>();
    let widths = calculate_widths(&rows);

    print_header(&widths);
    for row in rows {
        print_row(&row, &widths);
    }
}

fn model_row(model: &Model) -> ModelRow {
    ModelRow {
        provider: model.provider.clone(),
        model: model.id.clone(),
        context: format_token_count(model.context_window),
        max_out: format_token_count(model.max_tokens),
        thinking: if model.reasoning {
            "yes".to_string()
        } else {
            "no".to_string()
        },
        images: if model.input.iter().any(|entry| entry == "image") {
            "yes".to_string()
        } else {
            "no".to_string()
        },
    }
}

fn calculate_widths(rows: &[ModelRow]) -> ColumnWidths {
    let mut widths = ColumnWidths {
        provider: "provider".len(),
        model: "model".len(),
        context: "context".len(),
        max_out: "max-out".len(),
        thinking: "thinking".len(),
        images: "images".len(),
    };

    for row in rows {
        widths.provider = widths.provider.max(row.provider.len());
        widths.model = widths.model.max(row.model.len());
        widths.context = widths.context.max(row.context.len());
        widths.max_out = widths.max_out.max(row.max_out.len());
        widths.thinking = widths.thinking.max(row.thinking.len());
        widths.images = widths.images.max(row.images.len());
    }

    widths
}

fn print_header(widths: &ColumnWidths) {
    let line = [
        pad("provider", widths.provider),
        pad("model", widths.model),
        pad("context", widths.context),
        pad("max-out", widths.max_out),
        pad("thinking", widths.thinking),
        pad("images", widths.images),
    ]
    .join("  ");
    println!("{line}");
}

fn print_row(row: &ModelRow, widths: &ColumnWidths) {
    let line = [
        pad(&row.provider, widths.provider),
        pad(&row.model, widths.model),
        pad(&row.context, widths.context),
        pad(&row.max_out, widths.max_out),
        pad(&row.thinking, widths.thinking),
        pad(&row.images, widths.images),
    ]
    .join("  ");
    println!("{line}");
}

fn pad(value: &str, width: usize) -> String {
    format!("{value:width$}", width = width)
}

fn format_token_count(count: i64) -> String {
    if count >= 1_000_000 {
        let millions = count as f64 / 1_000_000.0;
        if (millions.fract() - 0.0).abs() < f64::EPSILON {
            format!("{millions:.0}M")
        } else {
            format!("{millions:.1}M")
        }
    } else if count >= 1_000 {
        let thousands = count as f64 / 1_000.0;
        if (thousands.fract() - 0.0).abs() < f64::EPSILON {
            format!("{thousands:.0}K")
        } else {
            format!("{thousands:.1}K")
        }
    } else {
        count.to_string()
    }
}
