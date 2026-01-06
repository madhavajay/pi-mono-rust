use pi::coding_agent::InteractiveMode;
use pi::tui::{Component, Container};
use std::any::Any;

// Source: packages/coding-agent/test/interactive-mode-status.test.ts

fn render_last_line(container: &Container, width: usize) -> String {
    let last = container.children.last();
    if let Some(component) = last {
        return component.render(width).join("\n");
    }
    String::new()
}

struct DummyComponent;

impl Component for DummyComponent {
    fn render(&self, _width: usize) -> Vec<String> {
        vec!["OTHER".to_string()]
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[test]
fn coalesces_immediately_sequential_status_messages() {
    let mut mode = InteractiveMode::new();

    mode.show_status("STATUS_ONE");
    assert_eq!(mode.chat_container.children.len(), 2);
    assert!(render_last_line(&mode.chat_container, 120).contains("STATUS_ONE"));

    mode.show_status("STATUS_TWO");
    assert_eq!(mode.chat_container.children.len(), 2);
    let last_line = render_last_line(&mode.chat_container, 120);
    assert!(last_line.contains("STATUS_TWO"));
    assert!(!last_line.contains("STATUS_ONE"));
}

#[test]
fn appends_a_new_status_line_if_something_else_was_added_in_between() {
    let mut mode = InteractiveMode::new();

    mode.show_status("STATUS_ONE");
    assert_eq!(mode.chat_container.children.len(), 2);

    mode.chat_container.add_child(DummyComponent);
    assert_eq!(mode.chat_container.children.len(), 3);

    mode.show_status("STATUS_TWO");
    assert_eq!(mode.chat_container.children.len(), 5);
    assert!(render_last_line(&mode.chat_container, 120).contains("STATUS_TWO"));
}
