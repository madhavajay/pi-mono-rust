use super::Component;
use std::any::Any;

pub struct Spacer {
    height: usize,
}

impl Spacer {
    pub fn new(height: usize) -> Self {
        Self { height }
    }
}

impl Component for Spacer {
    fn render(&self, _width: usize) -> Vec<String> {
        vec![String::new(); self.height]
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
