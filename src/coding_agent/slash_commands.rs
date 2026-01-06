pub fn parse_command_args(args: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quote: Option<char> = None;

    for ch in args.chars() {
        if let Some(quote) = in_quote {
            if ch == quote {
                in_quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }

        if ch == '"' || ch == '\'' {
            in_quote = Some(ch);
            continue;
        }

        if ch == ' ' || ch == '\t' {
            if !current.is_empty() {
                result.push(current);
                current = String::new();
            }
            continue;
        }

        current.push(ch);
    }

    if !current.is_empty() {
        result.push(current);
    }

    result
}

pub fn substitute_args<S: AsRef<str>>(template: &str, args: &[S]) -> String {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            let mut digits = String::new();
            while let Some(next) = chars.peek() {
                if next.is_ascii_digit() {
                    digits.push(*next);
                    chars.next();
                } else {
                    break;
                }
            }

            if !digits.is_empty() {
                let index = digits.parse::<usize>().unwrap_or(0);
                if index > 0 {
                    if let Some(value) = args.get(index - 1) {
                        result.push_str(value.as_ref());
                    }
                }
                continue;
            }

            result.push('$');
            continue;
        }

        result.push(ch);
    }

    let joined = args
        .iter()
        .map(|value| value.as_ref())
        .collect::<Vec<&str>>()
        .join(" ");
    let result = result.replace("$ARGUMENTS", &joined);
    result.replace("$@", &joined)
}
