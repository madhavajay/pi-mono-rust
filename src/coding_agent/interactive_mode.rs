use crate::tui::{Container, Spacer, Text};

pub struct InteractiveMode {
    pub chat_container: Container,
    last_status_spacer: Option<usize>,
    last_status_text: Option<usize>,
}

impl InteractiveMode {
    pub fn new() -> Self {
        Self {
            chat_container: Container::new(),
            last_status_spacer: None,
            last_status_text: None,
        }
    }

    pub fn show_status(&mut self, message: &str) {
        let len = self.chat_container.children.len();
        if len >= 2 {
            let last_index = len - 1;
            let second_last_index = len - 2;
            if self.last_status_text == Some(last_index)
                && self.last_status_spacer == Some(second_last_index)
            {
                if let Some(child) = self.chat_container.children.get_mut(last_index) {
                    if let Some(text) = child.as_any_mut().downcast_mut::<Text>() {
                        text.set_text(message);
                    }
                }
                return;
            }
        }

        self.chat_container.add_child(Spacer::new(1));
        self.chat_container.add_child(Text::new(message));
        let len = self.chat_container.children.len();
        self.last_status_spacer = Some(len - 2);
        self.last_status_text = Some(len - 1);
    }
}

impl Default for InteractiveMode {
    fn default() -> Self {
        Self::new()
    }
}
