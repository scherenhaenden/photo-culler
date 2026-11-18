//! Standalone state model backing the provisional demo window.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DemoState {
    Loading,
    Ready { photo_id: String, filename: String, thumbnail_url: String },
    Empty,
    Error { message: String },
}

impl DemoState {
    pub fn accessible_label(&self) -> &'static str {
        match self { Self::Loading => "Loading gallery", Self::Ready { .. } => "Gallery ready", Self::Empty => "Gallery empty", Self::Error { .. } => "Gallery error" }
    }
    pub fn can_retry(&self) -> bool { matches!(self, Self::Error { .. }) }
    pub fn retry_requested(&self) -> Option<DemoState> { self.can_retry().then_some(Self::Loading) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loading_ready_empty_error_are_visibly_distinct() {
        let states = [DemoState::Loading, DemoState::Ready { photo_id:"demo-001".into(), filename:"demo-001.jpg".into(), thumbnail_url:"/api/v1/photos/demo-001/thumbnail/800".into() }, DemoState::Empty, DemoState::Error { message:"offline".into() }];
        let labels: Vec<_> = states.iter().map(DemoState::accessible_label).collect();
        assert_eq!(labels, ["Loading gallery", "Gallery ready", "Gallery empty", "Gallery error"]);
        assert_eq!(states[3].retry_requested(), Some(DemoState::Loading));
        assert_eq!(states[2].retry_requested(), None);
    }
    #[test]
    fn ready_state_keeps_d01_photo_and_thumbnail_contract() {
        assert_eq!(DemoState::Ready { photo_id:"demo-001".into(), filename:"demo-001.jpg".into(), thumbnail_url:"/api/v1/photos/demo-001/thumbnail/800".into() }.accessible_label(), "Gallery ready");
    }
}
