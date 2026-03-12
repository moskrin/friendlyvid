use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub selected_clip: Option<Uuid>,
    pub selected_transition: Option<Uuid>,
    pub selected_text: Option<Uuid>,
}

impl Selection {
    pub fn clear(&mut self) {
        self.selected_clip = None;
        self.selected_transition = None;
        self.selected_text = None;
    }

    pub fn select_clip(&mut self, id: Uuid) {
        self.clear();
        self.selected_clip = Some(id);
    }

    #[allow(dead_code)]
    pub fn has_selection(&self) -> bool {
        self.selected_clip.is_some()
            || self.selected_transition.is_some()
            || self.selected_text.is_some()
    }
}
