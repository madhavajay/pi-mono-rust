pub mod autocomplete;
pub mod components;
pub mod keys;
pub mod terminal_image;
pub mod utils;

pub use autocomplete::{
    AutocompleteItem, AutocompleteSuggestions, CombinedAutocompleteProvider, SlashCommand,
};
pub use components::{
    bool_values, double_escape_action_values, queue_mode_values, thinking_level_values, Component,
    Container, DefaultTextStyle, Editor, EditorTheme, Expandable, ExpandableText, FilterMode,
    Image, ImageOptions, ImageTheme, LoginDialogComponent, LoginDialogResult, LoginDialogState,
    Markdown, MarkdownTheme, ModelItem, ModelSelectorComponent, ModelSelectorResult,
    OAuthSelectorComponent, OAuthSelectorMode, OAuthSelectorResult, SelectList, SelectListTheme,
    SessionList, SessionSelectorComponent, SettingItem, SettingValue, SettingsSelectorComponent,
    SettingsSelectorResult, Spacer, Text, ToolPreviewConfig, TreeList, TreeSelectorComponent,
    TruncatedText,
};
pub use keys::{is_kitty_protocol_active, matches_key, parse_key, set_kitty_protocol_active};
pub use terminal_image::{
    calculate_image_rows, encode_iterm2, encode_kitty, get_capabilities, get_cell_dimensions,
    get_gif_dimensions, get_image_dimensions, get_jpeg_dimensions, get_png_dimensions,
    get_webp_dimensions, image_fallback, render_image, set_cell_dimensions, CellDimensions,
    ImageDimensions, ImageProtocol, ImageRenderOptions, ImageRenderResult, TerminalCapabilities,
};
pub use utils::{
    apply_background_to_line, is_punctuation_char, is_whitespace_char, truncate_to_width,
    visible_width, wrap_text_with_ansi,
};
