use pi::coding_agent::{fuzzy_filter, fuzzy_match};

// Source: packages/coding-agent/test/fuzzy.test.ts

#[test]
fn empty_query_matches_everything_with_score_0() {
    let result = fuzzy_match("", "anything");
    assert!(result.matches);
    assert_eq!(result.score, 0.0);
}

#[test]
fn query_longer_than_text_does_not_match() {
    let result = fuzzy_match("longquery", "short");
    assert!(!result.matches);
}

#[test]
fn exact_match_has_good_score() {
    let result = fuzzy_match("test", "test");
    assert!(result.matches);
    assert!(result.score < 0.0);
}

#[test]
fn characters_must_appear_in_order() {
    let in_order = fuzzy_match("abc", "aXbXc");
    assert!(in_order.matches);

    let out_of_order = fuzzy_match("abc", "cba");
    assert!(!out_of_order.matches);
}

#[test]
fn case_insensitive_matching() {
    let result = fuzzy_match("ABC", "abc");
    assert!(result.matches);

    let result2 = fuzzy_match("abc", "ABC");
    assert!(result2.matches);
}

#[test]
fn consecutive_matches_score_better_than_scattered_matches() {
    let consecutive = fuzzy_match("foo", "foobar");
    let scattered = fuzzy_match("foo", "f_o_o_bar");

    assert!(consecutive.matches);
    assert!(scattered.matches);
    assert!(consecutive.score < scattered.score);
}

#[test]
fn word_boundary_matches_score_better() {
    let at_boundary = fuzzy_match("fb", "foo-bar");
    let not_at_boundary = fuzzy_match("fb", "afbx");

    assert!(at_boundary.matches);
    assert!(not_at_boundary.matches);
    assert!(at_boundary.score < not_at_boundary.score);
}

#[test]
fn empty_query_returns_all_items_unchanged() {
    let items = vec![
        "apple".to_string(),
        "banana".to_string(),
        "cherry".to_string(),
    ];
    let result = fuzzy_filter(&items, "", |x| x.as_str());
    assert_eq!(result, items);
}

#[test]
fn filters_out_non_matching_items() {
    let items = vec![
        "apple".to_string(),
        "banana".to_string(),
        "cherry".to_string(),
    ];
    let result = fuzzy_filter(&items, "an", |x| x.as_str());
    assert!(result.contains(&"banana".to_string()));
    assert!(!result.contains(&"apple".to_string()));
    assert!(!result.contains(&"cherry".to_string()));
}

#[test]
fn sorts_results_by_match_quality() {
    let items = vec![
        "a_p_p".to_string(),
        "app".to_string(),
        "application".to_string(),
    ];
    let result = fuzzy_filter(&items, "app", |x| x.as_str());
    assert_eq!(result.first().map(String::as_str), Some("app"));
}

#[test]
fn works_with_custom_gettext_function() {
    #[derive(Clone, Debug, PartialEq)]
    struct Item {
        name: String,
        id: i32,
    }

    let items = vec![
        Item {
            name: "foo".to_string(),
            id: 1,
        },
        Item {
            name: "bar".to_string(),
            id: 2,
        },
        Item {
            name: "foobar".to_string(),
            id: 3,
        },
    ];

    let result = fuzzy_filter(&items, "foo", |item| item.name.as_str());
    let names: Vec<String> = result.iter().map(|item| item.name.clone()).collect();
    assert_eq!(result.len(), 2);
    assert!(names.contains(&"foo".to_string()));
    assert!(names.contains(&"foobar".to_string()));
}
