//! Bounded, UI-independent library-grid state for the Slint library screen.
//!
//! The shared Slint app and typed REST client are not present at the sprint
//! target. This module defines their page, filter, keyboard and thumbnail
//! scheduling contract without adding dependencies or reading image files.

use std::collections::BTreeSet;

pub const MAX_PAGE_SIZE: usize = 100;
pub const MAX_THUMBNAIL_BATCH: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Representation {
    Jpeg,
    Raw,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibraryPhoto {
    pub photo_id: String,
    pub filename: String,
    pub session_id: Option<String>,
    pub decision: Option<String>,
    pub representation: Representation,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LibraryFilter {
    pub query: String,
    pub session_id: Option<String>,
    pub decision: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageRequest {
    pub offset: usize,
    pub limit: usize,
    pub filter: LibraryFilter,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: usize,
}

pub trait LibrarySource {
    type Error;
    fn fetch_page(&mut self, request: &PageRequest) -> Result<Page<LibraryPhoto>, Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Enter,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GridError<E> {
    Source(E),
    EmptyPageSize,
}

/// One server page plus a bounded thumbnail request set for the viewport.
pub struct LibraryGrid {
    page_size: usize,
    columns: usize,
    page: Page<LibraryPhoto>,
    page_index: usize,
    filter: LibraryFilter,
    focus_index: usize,
    active_photo_id: Option<String>,
    thumbnail_requests: Vec<String>,
}

impl LibraryGrid {
    pub fn new(page_size: usize, columns: usize) -> Result<Self, GridError<()>> {
        if page_size == 0 || columns == 0 {
            return Err(GridError::EmptyPageSize);
        }
        Ok(Self {
            page_size: page_size.min(MAX_PAGE_SIZE),
            columns,
            page: Page {
                items: Vec::new(),
                total: 0,
            },
            page_index: 0,
            filter: LibraryFilter::default(),
            focus_index: 0,
            active_photo_id: None,
            thumbnail_requests: Vec::new(),
        })
    }

    pub fn items(&self) -> &[LibraryPhoto] {
        &self.page.items
    }

    pub fn total(&self) -> usize {
        self.page.total
    }

    pub fn page_index(&self) -> usize {
        self.page_index
    }

    pub fn focused_photo_id(&self) -> Option<&str> {
        self.page.items.get(self.focus_index).map(|p| p.photo_id.as_str())
    }

    pub fn active_photo_id(&self) -> Option<&str> {
        self.active_photo_id.as_deref()
    }

    pub fn thumbnail_requests(&self) -> &[String] {
        &self.thumbnail_requests
    }

    pub fn load<S: LibrarySource>(&mut self, source: &mut S) -> Result<(), GridError<S::Error>> {
        let offset = self.page_index.saturating_mul(self.page_size);
        let result = source
            .fetch_page(&PageRequest {
                offset,
                limit: self.page_size,
                filter: self.filter.clone(),
            })
            .map_err(GridError::Source)?;
        self.page = Page {
            items: result.items.into_iter().take(self.page_size).collect(),
            total: result.total,
        };
        self.focus_index = self
            .active_photo_id
            .as_ref()
            .and_then(|id| self.page.items.iter().position(|p| &p.photo_id == id))
            .unwrap_or_else(|| self.focus_index.min(self.page.items.len().saturating_sub(1)));
        if self.active_photo_id.as_ref().is_some_and(|id| {
            !self.page.items.iter().any(|photo| &photo.photo_id == id)
        }) {
            self.active_photo_id = None;
        }
        self.refresh_thumbnail_requests(0, self.page.items.len().min(MAX_THUMBNAIL_BATCH));
        Ok(())
    }

    pub fn set_filter<S: LibrarySource>(
        &mut self,
        source: &mut S,
        filter: LibraryFilter,
    ) -> Result<(), GridError<S::Error>> {
        self.filter = filter;
        self.page_index = 0;
        self.focus_index = 0;
        self.active_photo_id = None;
        self.load(source)
    }

    pub fn next_page<S: LibrarySource>(
        &mut self,
        source: &mut S,
    ) -> Result<(), GridError<S::Error>> {
        let last_page = self.page.total.saturating_sub(1) / self.page_size;
        if self.page_index < last_page {
            self.page_index += 1;
            self.active_photo_id = None;
            self.focus_index = 0;
            self.load(source)?;
        }
        Ok(())
    }

    pub fn previous_page<S: LibrarySource>(
        &mut self,
        source: &mut S,
    ) -> Result<(), GridError<S::Error>> {
        if self.page_index > 0 {
            self.page_index -= 1;
            self.active_photo_id = None;
            self.focus_index = 0;
            self.load(source)?;
        }
        Ok(())
    }

    pub fn handle_key(&mut self, key: Key) {
        let len = self.page.items.len();
        if len == 0 {
            return;
        }
        match key {
            Key::Left => self.focus_index = self.focus_index.saturating_sub(1),
            Key::Right => self.focus_index = (self.focus_index + 1).min(len - 1),
            Key::Up => self.focus_index = self.focus_index.saturating_sub(self.columns),
            Key::Down => self.focus_index = (self.focus_index + self.columns).min(len - 1),
            Key::Home => self.focus_index = 0,
            Key::End => self.focus_index = len - 1,
            Key::PageUp => self.focus_index = self.focus_index.saturating_sub(self.page_size),
            Key::PageDown => self.focus_index = (self.focus_index + self.page_size).min(len - 1),
            Key::Enter => self.active_photo_id = self.focused_photo_id().map(str::to_owned),
        }
    }

    /// Change the active row's representation while keeping its photo ID active.
    pub fn set_representation(&mut self, photo_id: &str, representation: Representation) -> bool {
        let Some(photo) = self.page.items.iter_mut().find(|photo| photo.photo_id == photo_id) else {
            return false;
        };
        photo.representation = representation;
        self.active_photo_id = Some(photo_id.to_owned());
        true
    }

    /// Build a bounded request batch for an inclusive viewport plus small preload.
    pub fn refresh_thumbnail_requests(&mut self, start: usize, count: usize) {
        let start = start.min(self.page.items.len());
        let end = start
            .saturating_add(count)
            .min(self.page.items.len())
            .min(start.saturating_add(MAX_THUMBNAIL_BATCH));
        let ids: BTreeSet<_> = self.page.items[start..end]
            .iter()
            .map(|photo| photo.photo_id.clone())
            .collect();
        self.thumbnail_requests = ids.into_iter().collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSource {
        photos: Vec<LibraryPhoto>,
        requests: Vec<PageRequest>,
    }

    impl LibrarySource for FakeSource {
        type Error = &'static str;

        fn fetch_page(&mut self, request: &PageRequest) -> Result<Page<LibraryPhoto>, Self::Error> {
            self.requests.push(request.clone());
            let query = request.filter.query.to_lowercase();
            let filtered: Vec<_> = self.photos.iter().filter(|photo| {
                (query.is_empty() || photo.filename.to_lowercase().contains(&query))
                    && request.filter.session_id.as_ref().map_or(true, |session| photo.session_id.as_ref() == Some(session))
                    && request.filter.decision.as_ref().map_or(true, |decision| photo.decision.as_ref() == Some(decision))
            }).cloned().collect();
            let total = filtered.len();
            let items = filtered.into_iter().skip(request.offset).take(request.limit).collect();
            Ok(Page { items, total })
        }
    }

    fn gallery(count: usize) -> FakeSource {
        FakeSource {
            photos: (0..count).map(|i| LibraryPhoto {
                photo_id: format!("photo-{i:05}"),
                filename: if i % 2 == 0 { format!("concert-{i:05}.jpg") } else { format!("backstage-{i:05}.jpg") },
                session_id: Some(if i < count / 2 { "session-a" } else { "session-b" }.to_owned()),
                decision: Some(if i % 3 == 0 { "KEEP" } else { "REVIEW" }.to_owned()),
                representation: Representation::Jpeg,
            }).collect(),
            requests: Vec::new(),
        }
    }

    #[test]
    fn ten_thousand_photo_gallery_fetches_only_a_bounded_page_and_viewport() {
        let mut source = gallery(10_000);
        let mut grid = LibraryGrid::new(40, 5).unwrap();
        grid.load(&mut source).unwrap();
        assert_eq!(grid.total(), 10_000);
        assert_eq!(grid.items().len(), 40);
        assert_eq!(grid.thumbnail_requests().len(), 32);
        assert_eq!(source.requests.len(), 1);
        assert_eq!(source.requests[0].limit, 40);
        grid.refresh_thumbnail_requests(20, 500);
        assert_eq!(grid.thumbnail_requests().len(), 20);
        assert!(grid.thumbnail_requests().len() <= MAX_THUMBNAIL_BATCH);
    }

    #[test]
    fn pagination_filters_order_and_keyboard_keep_photo_identity() {
        let mut source = gallery(10_000);
        let mut grid = LibraryGrid::new(40, 5).unwrap();
        grid.load(&mut source).unwrap();
        assert_eq!(grid.items()[0].photo_id, "photo-00000");
        grid.handle_key(Key::Right);
        grid.handle_key(Key::Enter);
        assert_eq!(grid.active_photo_id(), Some("photo-00001"));
        grid.set_representation("photo-00001", Representation::Raw);
        assert_eq!(grid.active_photo_id(), Some("photo-00001"));
        assert_eq!(grid.items()[1].representation, Representation::Raw);

        grid.next_page(&mut source).unwrap();
        assert_eq!(grid.items()[0].photo_id, "photo-00040");
        grid.previous_page(&mut source).unwrap();
        assert_eq!(grid.items()[0].photo_id, "photo-00000");

        grid.set_filter(&mut source, LibraryFilter {
            query: "concert".into(), session_id: Some("session-b".into()), decision: Some("KEEP".into())
        }).unwrap();
        assert_eq!(grid.page_index(), 0);
        assert!(!grid.items().is_empty());
        assert!(grid.items().iter().all(|photo| photo.filename.contains("concert") && photo.session_id.as_deref() == Some("session-b") && photo.decision.as_deref() == Some("KEEP")));
        assert!(grid.items().windows(2).all(|pair| pair[0].photo_id <= pair[1].photo_id));
        grid.handle_key(Key::End);
        assert_eq!(grid.focused_photo_id(), grid.items().last().map(|p| p.photo_id.as_str()));
    }

    #[test]
    fn empty_filters_and_navigation_never_request_unbounded_data() {
        let mut source = gallery(0);
        let mut grid = LibraryGrid::new(500, 4).unwrap();
        assert_eq!(grid.page_size, MAX_PAGE_SIZE);
        grid.load(&mut source).unwrap();
        grid.next_page(&mut source).unwrap();
        grid.handle_key(Key::Down);
        assert!(grid.items().is_empty());
        assert!(grid.thumbnail_requests().is_empty());
        assert!(source.requests.iter().all(|request| request.limit <= MAX_PAGE_SIZE));
    }
}
