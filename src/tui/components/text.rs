use super::Component;
use std::any::Any;

pub struct Text {
    text: String,
}

impl Text {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }
}

impl Component for Text {
    fn render(&self, _width: usize) -> Vec<String> {
        if self.text.is_empty() {
            vec![String::new()]
        } else {
            self.text.split('\n').map(|line| line.to_string()).collect()
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
