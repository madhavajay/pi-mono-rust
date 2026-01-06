#[derive(Clone, Debug, PartialEq)]
pub struct FuzzyMatch {
    pub matches: bool,
    pub score: f64,
}

pub fn fuzzy_match(query: &str, text: &str) -> FuzzyMatch {
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();

    if query_lower.is_empty() {
        return FuzzyMatch {
            matches: true,
            score: 0.0,
        };
    }

    let query_chars: Vec<char> = query_lower.chars().collect();
    let text_chars: Vec<char> = text_lower.chars().collect();

    if query_chars.len() > text_chars.len() {
        return FuzzyMatch {
            matches: false,
            score: 0.0,
        };
    }

    let mut query_index = 0usize;
    let mut score = 0.0f64;
    let mut last_match_index: isize = -1;
    let mut consecutive_matches = 0i32;

    for (i, ch) in text_chars.iter().enumerate() {
        if query_index >= query_chars.len() {
            break;
        }

        if *ch == query_chars[query_index] {
            let is_word_boundary = if i == 0 {
                true
            } else {
                is_boundary(text_chars[i - 1])
            };

            if last_match_index == (i as isize) - 1 {
                consecutive_matches += 1;
                score -= f64::from(consecutive_matches * 5);
            } else {
                consecutive_matches = 0;
                if last_match_index >= 0 {
                    let gap = i as isize - last_match_index - 1;
                    if gap > 0 {
                        score += (gap as f64) * 2.0;
                    }
                }
            }

            if is_word_boundary {
                score -= 10.0;
            }

            score += (i as f64) * 0.1;
            last_match_index = i as isize;
            query_index += 1;
        }
    }

    if query_index < query_chars.len() {
        return FuzzyMatch {
            matches: false,
            score: 0.0,
        };
    }

    FuzzyMatch {
        matches: true,
        score,
    }
}

pub fn fuzzy_filter<T>(items: &[T], query: &str, get_text: impl Fn(&T) -> &str) -> Vec<T>
where
    T: Clone,
{
    if query.trim().is_empty() {
        return items.to_vec();
    }

    let tokens: Vec<&str> = query.split_whitespace().filter(|t| !t.is_empty()).collect();
    if tokens.is_empty() {
        return items.to_vec();
    }

    let mut results: Vec<(T, f64)> = Vec::new();

    for item in items {
        let text = get_text(item);
        let mut total_score = 0.0f64;
        let mut all_match = true;

        for token in &tokens {
            let matched = fuzzy_match(token, text);
            if matched.matches {
                total_score += matched.score;
            } else {
                all_match = false;
                break;
            }
        }

        if all_match {
            results.push((item.clone(), total_score));
        }
    }

    results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    results.into_iter().map(|(item, _)| item).collect()
}

fn is_boundary(ch: char) -> bool {
    ch.is_whitespace() || ch == '-' || ch == '_' || ch == '.' || ch == '/'
}
