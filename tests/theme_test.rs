use pi::coding_agent::{available_themes, load_theme, ThemeBg, ThemeColor};

#[test]
fn loads_builtin_theme_and_formats_colors() {
    std::env::set_var("COLORTERM", "truecolor");
    let theme = load_theme(Some("dark")).expect("dark theme loads");

    let rendered = theme.fg(ThemeColor::Accent, "X");
    assert!(rendered.contains("X"));
    assert!(rendered.contains("\x1b["));

    let bg_rendered = theme.bg(ThemeBg::UserMessageBg, "Y");
    assert!(bg_rendered.contains("Y"));
    assert!(bg_rendered.contains("\x1b["));
}

#[test]
fn available_themes_includes_builtins() {
    let themes = available_themes();
    assert!(themes.contains(&"dark".to_string()));
    assert!(themes.contains(&"light".to_string()));
}
