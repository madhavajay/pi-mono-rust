use super::Component;

pub struct Container {
    pub children: Vec<Box<dyn Component>>,
}

impl Container {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }

    pub fn add_child<C: Component + 'static>(&mut self, child: C) {
        self.children.push(Box::new(child));
    }

    pub fn clear(&mut self) {
        self.children.clear();
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}
