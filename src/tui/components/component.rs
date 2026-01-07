use std::any::Any;

pub trait Component {
    fn render(&self, width: usize) -> Vec<String>;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
