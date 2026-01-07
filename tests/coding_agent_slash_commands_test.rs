use pi::coding_agent::{parse_command_args, substitute_args};

// Source: packages/coding-agent/test/slash-commands.test.ts

#[test]
fn should_replace_arguments_with_all_args_joined() {
    assert_eq!(
        substitute_args("Test: $ARGUMENTS", &["a", "b", "c"]),
        "Test: a b c"
    );
}

#[test]
fn should_replace_with_all_args_joined() {
    assert_eq!(substitute_args("Test: $@", &["a", "b", "c"]), "Test: a b c");
}

#[test]
fn should_replace_and_arguments_identically() {
    let args = vec!["foo", "bar", "baz"];
    assert_eq!(
        substitute_args("Test: $@", &args),
        substitute_args("Test: $ARGUMENTS", &args)
    );
}

#[test]
fn should_not_recursively_substitute_patterns_in_argument_values() {
    assert_eq!(
        substitute_args("$ARGUMENTS", &["$1", "$ARGUMENTS"]),
        "$1 $ARGUMENTS"
    );
    assert_eq!(substitute_args("$@", &["$100", "$1"]), "$100 $1");
    assert_eq!(substitute_args("$ARGUMENTS", &["$100", "$1"]), "$100 $1");
}

#[test]
fn should_support_mixed_1_2_and_arguments() {
    assert_eq!(
        substitute_args("$1: $ARGUMENTS", &["prefix", "a", "b"]),
        "prefix: prefix a b"
    );
}

#[test]
fn should_support_mixed_1_2_and() {
    assert_eq!(
        substitute_args("$1: $@", &["prefix", "a", "b"]),
        "prefix: prefix a b"
    );
}

#[test]
fn should_handle_empty_arguments_array_with_arguments() {
    let empty: Vec<&str> = Vec::new();
    assert_eq!(substitute_args("Test: $ARGUMENTS", &empty), "Test: ");
}

#[test]
fn should_handle_empty_arguments_array_with() {
    let empty: Vec<&str> = Vec::new();
    assert_eq!(substitute_args("Test: $@", &empty), "Test: ");
}

#[test]
fn should_handle_empty_arguments_array_with_1() {
    let empty: Vec<&str> = Vec::new();
    assert_eq!(substitute_args("Test: $1", &empty), "Test: ");
}

#[test]
fn should_handle_multiple_occurrences_of_arguments() {
    assert_eq!(
        substitute_args("$ARGUMENTS and $ARGUMENTS", &["a", "b"]),
        "a b and a b"
    );
}

#[test]
fn should_handle_multiple_occurrences_of() {
    assert_eq!(substitute_args("$@ and $@", &["a", "b"]), "a b and a b");
}

#[test]
fn should_handle_mixed_occurrences_of_and_arguments() {
    assert_eq!(
        substitute_args("$@ and $ARGUMENTS", &["a", "b"]),
        "a b and a b"
    );
}

#[test]
fn should_handle_special_characters_in_arguments() {
    assert_eq!(
        substitute_args("$1 $2: $ARGUMENTS", &["arg100", "@user"]),
        "arg100 @user: arg100 @user"
    );
}

#[test]
fn should_handle_out_of_range_numbered_placeholders() {
    assert_eq!(substitute_args("$1 $2 $3 $4 $5", &["a", "b"]), "a b   ");
}

#[test]
fn should_handle_unicode_characters() {
    assert_eq!(
        substitute_args("$ARGUMENTS", &["日本語", "🎉", "café"]),
        "日本語 🎉 café"
    );
}

#[test]
fn should_preserve_newlines_and_tabs_in_argument_values() {
    assert_eq!(
        substitute_args("$1 $2", &["line1\nline2", "tab\tthere"]),
        "line1\nline2 tab\tthere"
    );
}

#[test]
fn should_handle_consecutive_dollar_patterns() {
    assert_eq!(substitute_args("$1$2", &["a", "b"]), "ab");
}

#[test]
fn should_handle_quoted_arguments_with_spaces() {
    assert_eq!(
        substitute_args("$ARGUMENTS", &["first arg", "second arg"]),
        "first arg second arg"
    );
}

#[test]
fn should_handle_single_argument_with_arguments() {
    assert_eq!(substitute_args("Test: $ARGUMENTS", &["only"]), "Test: only");
}

#[test]
fn should_handle_single_argument_with() {
    assert_eq!(substitute_args("Test: $@", &["only"]), "Test: only");
}

#[test]
fn should_handle_0_zero_index() {
    assert_eq!(substitute_args("$0", &["a", "b"]), "");
}

#[test]
fn should_handle_decimal_number_in_pattern_only_integer_part_matches() {
    assert_eq!(substitute_args("$1.5", &["a"]), "a.5");
}

#[test]
fn should_handle_arguments_as_part_of_word() {
    assert_eq!(substitute_args("pre$ARGUMENTS", &["a", "b"]), "prea b");
}

#[test]
fn should_handle_as_part_of_word() {
    assert_eq!(substitute_args("pre$@", &["a", "b"]), "prea b");
}

#[test]
fn should_handle_empty_arguments_in_middle_of_list() {
    assert_eq!(substitute_args("$ARGUMENTS", &["a", "", "c"]), "a  c");
}

#[test]
fn should_handle_trailing_and_leading_spaces_in_arguments() {
    assert_eq!(
        substitute_args("$ARGUMENTS", &["  leading  ", "trailing  "]),
        "  leading   trailing  "
    );
}

#[test]
fn should_handle_argument_containing_pattern_partially() {
    assert_eq!(
        substitute_args("Prefix $ARGUMENTS suffix", &["ARGUMENTS"]),
        "Prefix ARGUMENTS suffix"
    );
}

#[test]
fn should_handle_non_matching_patterns() {
    assert_eq!(substitute_args("$A $$ $ $ARGS", &["a"]), "$A $$ $ $ARGS");
}

#[test]
fn should_handle_case_variations_case_sensitive() {
    assert_eq!(
        substitute_args("$arguments $Arguments $ARGUMENTS", &["a", "b"]),
        "$arguments $Arguments a b"
    );
}

#[test]
fn should_handle_both_syntaxes_in_same_command_with_same_result() {
    let args = vec!["x", "y", "z"];
    let result1 = substitute_args("$@ and $ARGUMENTS", &args);
    let result2 = substitute_args("$ARGUMENTS and $@", &args);
    assert_eq!(result1, result2);
    assert_eq!(result1, "x y z and x y z");
}

#[test]
fn should_handle_very_long_argument_lists() {
    let args = (0..100).map(|i| format!("arg{i}")).collect::<Vec<_>>();
    let result = substitute_args("$ARGUMENTS", &args);
    assert_eq!(result, args.join(" "));
}

#[test]
fn should_handle_numbered_placeholders_with_single_digit() {
    assert_eq!(substitute_args("$1 $2 $3", &["a", "b", "c"]), "a b c");
}

#[test]
fn should_handle_numbered_placeholders_with_multiple_digits() {
    let args = (0..15).map(|i| format!("val{i}")).collect::<Vec<_>>();
    assert_eq!(substitute_args("$10 $12 $15", &args), "val9 val11 val14");
}

#[test]
fn should_handle_escaped_dollar_signs_literal_backslash_preserved() {
    let empty: Vec<&str> = Vec::new();
    assert_eq!(substitute_args("Price: \\$100", &empty), "Price: \\");
}

#[test]
fn should_handle_mixed_numbered_and_wildcard_placeholders() {
    assert_eq!(
        substitute_args("$1: $@ ($ARGUMENTS)", &["first", "second", "third"]),
        "first: first second third (first second third)"
    );
}

#[test]
fn should_handle_command_with_no_placeholders() {
    assert_eq!(
        substitute_args("Just plain text", &["a", "b"]),
        "Just plain text"
    );
}

#[test]
fn should_handle_command_with_only_placeholders() {
    assert_eq!(substitute_args("$1 $2 $@", &["a", "b", "c"]), "a b a b c");
}

#[test]
fn should_parse_simple_space_separated_arguments() {
    assert_eq!(
        parse_command_args("a b c"),
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
}

#[test]
fn should_parse_quoted_arguments_with_spaces() {
    assert_eq!(
        parse_command_args("\"first arg\" second"),
        vec!["first arg".to_string(), "second".to_string()]
    );
}

#[test]
fn should_parse_single_quoted_arguments() {
    assert_eq!(
        parse_command_args("'first arg' second"),
        vec!["first arg".to_string(), "second".to_string()]
    );
}

#[test]
fn should_parse_mixed_quote_styles() {
    assert_eq!(
        parse_command_args("\"double\" 'single' \"double again\""),
        vec![
            "double".to_string(),
            "single".to_string(),
            "double again".to_string()
        ]
    );
}

#[test]
fn should_handle_empty_string() {
    assert_eq!(parse_command_args(""), Vec::<String>::new());
}

#[test]
fn should_handle_extra_spaces() {
    assert_eq!(
        parse_command_args("a  b   c"),
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
}

#[test]
fn should_handle_tabs_as_separators() {
    assert_eq!(
        parse_command_args("a\tb\tc"),
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
}

#[test]
fn should_handle_quoted_empty_string() {
    assert_eq!(parse_command_args("\"\" \" \""), vec![" ".to_string()]);
}

#[test]
fn should_handle_arguments_with_special_characters() {
    assert_eq!(
        parse_command_args("$100 @user #tag"),
        vec!["$100".to_string(), "@user".to_string(), "#tag".to_string()]
    );
}

#[test]
fn should_handle_newlines_in_arguments() {
    assert_eq!(
        parse_command_args("\"line1\nline2\" second"),
        vec!["line1\nline2".to_string(), "second".to_string()]
    );
}

#[test]
fn should_handle_escaped_quotes_inside_quoted_strings() {
    assert_eq!(
        parse_command_args("\"quoted \\\"text\\\"\""),
        vec!["quoted \\text\\".to_string()]
    );
}

#[test]
fn should_handle_trailing_spaces() {
    assert_eq!(
        parse_command_args("a b c   "),
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
}

#[test]
fn should_handle_leading_spaces() {
    assert_eq!(
        parse_command_args("   a b c"),
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
}

#[test]
fn should_parse_and_substitute_together_correctly() {
    let input = "Button \"onClick handler\" \"disabled support\"";
    let args = parse_command_args(input);
    let template = "Create component $1 with features: $ARGUMENTS";
    let result = substitute_args(template, &args);
    assert_eq!(
        result,
        "Create component Button with features: Button onClick handler disabled support"
    );
}

#[test]
fn should_handle_the_example_from_readme() {
    let input = "Button \"onClick handler\" \"disabled support\"";
    let args = parse_command_args(input);
    let template = "Create a React component named $1 with features: $ARGUMENTS";
    let result = substitute_args(template, &args);
    assert_eq!(
        result,
        "Create a React component named Button with features: Button onClick handler disabled support"
    );
}

#[test]
fn should_produce_same_result_with_and_arguments() {
    let args = parse_command_args("feature1 feature2 feature3");
    let template1 = "Implement: $@";
    let template2 = "Implement: $ARGUMENTS";
    assert_eq!(
        substitute_args(template1, &args),
        substitute_args(template2, &args)
    );
}
