//! Demo presentation controller. Platform code adapts [`UiDispatcher`] to
//! `slint::invoke_from_event_loop`; network work runs before publication.

use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

pub type DemoFuture<'a> = Pin<Box<dyn Future<Output = Result<DemoGallery, DemoError>> + 'a>>;
pub type ThumbnailFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<u8>, DemoError>> + 'a>>;
pub trait DemoClient {
    fn gallery<'a>(&'a self, gallery_id: &'a str) -> DemoFuture<'a>;
    fn thumbnail<'a>(&'a self, photo: &'a DemoPhoto) -> ThumbnailFuture<'a>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemoPhoto {
    pub photo_id: String,
    pub filename: String,
    pub decision: String,
    pub thumbnail_url: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemoGallery {
    pub gallery_id: String,
    pub photos: Vec<DemoPhoto>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemoError {
    pub message: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewState {
    Loading,
    Ready {
        gallery_id: String,
        photo: DemoPhoto,
        jpeg: Option<Vec<u8>>,
        preview_error: Option<String>,
    },
    Empty,
    Error {
        message: String,
    },
}

pub trait DemoView: Send + Sync + 'static {
    fn set_state(&self, state: ViewState);
}
/// Dispatches a closure onto the Slint event loop. Implementations must never
/// run it inline on a worker thread.
pub trait UiDispatcher: Send + Sync + 'static {
    fn dispatch(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), DemoError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadTicket {
    generation: u64,
    gallery_id: String,
}
struct State {
    generation: u64,
    current: ViewState,
}

pub struct DemoController<V, D> {
    state: Arc<Mutex<State>>,
    view: Arc<V>,
    dispatcher: Arc<D>,
}

impl<V: DemoView, D: UiDispatcher> DemoController<V, D> {
    pub fn new(view: Arc<V>, dispatcher: Arc<D>) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                generation: 0,
                current: ViewState::Loading,
            })),
            view,
            dispatcher,
        }
    }
    pub fn begin_load(&self, gallery_id: impl Into<String>) -> LoadTicket {
        let gallery_id = gallery_id.into();
        let mut state = self.state.lock().expect("controller mutex poisoned");
        state.generation = state.generation.wrapping_add(1);
        state.current = ViewState::Loading;
        let generation = state.generation;
        drop(state);
        let view = self.view.clone();
        let _ = self
            .dispatcher
            .dispatch(Box::new(move || view.set_state(ViewState::Loading)));
        LoadTicket {
            generation,
            gallery_id,
        }
    }
    pub fn retry(&self, last_gallery_id: impl Into<String>) -> LoadTicket {
        self.begin_load(last_gallery_id)
    }
    pub fn snapshot(&self) -> ViewState {
        self.state
            .lock()
            .expect("controller mutex poisoned")
            .current
            .clone()
    }

    /// Fetch gallery and first JPEG off the event loop, then publish only if
    /// this ticket is still current. A stale completion is silently discarded.
    pub async fn execute<C: DemoClient>(
        &self,
        client: &C,
        ticket: LoadTicket,
    ) -> Result<(), DemoError> {
        let result = match client.gallery(&ticket.gallery_id).await {
            Ok(gallery) if gallery.photos.is_empty() => Ok(ViewState::Empty),
            Ok(gallery) => {
                let photo = gallery
                    .photos
                    .into_iter()
                    .next()
                    .expect("non-empty gallery");
                match client.thumbnail(&photo).await {
                    Ok(jpeg) => Ok(ViewState::Ready {
                        gallery_id: ticket.gallery_id.clone(),
                        photo,
                        jpeg: Some(jpeg),
                        preview_error: None,
                    }),
                    Err(error) => Ok(ViewState::Ready {
                        gallery_id: ticket.gallery_id.clone(),
                        photo,
                        jpeg: None,
                        preview_error: Some(error.message),
                    }),
                }
            }
            Err(error) => Err(error),
        };
        let next = result.unwrap_or_else(|e| ViewState::Error { message: e.message });
        let state = self.state.clone();
        let view = self.view.clone();
        self.dispatcher.dispatch(Box::new(move || {
            let Ok(mut state) = state.lock() else { return };
            if state.generation != ticket.generation {
                return;
            }
            state.current = next.clone();
            view.set_state(next);
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
    };
    #[derive(Default)]
    struct FakeView {
        states: Mutex<Vec<ViewState>>,
    }
    impl DemoView for FakeView {
        fn set_state(&self, state: ViewState) {
            self.states.lock().unwrap().push(state);
        }
    }
    #[derive(Default)]
    struct Queue {
        tasks: Mutex<VecDeque<Box<dyn FnOnce() + Send>>>,
    }
    impl UiDispatcher for Queue {
        fn dispatch(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), DemoError> {
            self.tasks.lock().unwrap().push_back(task);
            Ok(())
        }
    }
    impl Queue {
        fn run(&self) {
            self.tasks.lock().unwrap().pop_front().unwrap()();
        }
        fn run_all(&self) {
            while !self.tasks.lock().unwrap().is_empty() {
                self.run();
            }
        }
    }
    struct FakeClient {
        photos: Vec<DemoPhoto>,
        image: Result<Vec<u8>, DemoError>,
    }
    impl DemoClient for FakeClient {
        fn gallery<'a>(&'a self, _: &'a str) -> DemoFuture<'a> {
            let photos = self.photos.clone();
            Box::pin(async move {
                Ok(DemoGallery {
                    gallery_id: "demo".into(),
                    photos,
                })
            })
        }
        fn thumbnail<'a>(&'a self, _: &'a DemoPhoto) -> ThumbnailFuture<'a> {
            let image = self.image.clone();
            Box::pin(async move { image })
        }
    }
    fn photo() -> DemoPhoto {
        DemoPhoto {
            photo_id: "demo-001".into(),
            filename: "demo-001.jpg".into(),
            decision: "undecided".into(),
            thumbnail_url: "/api/v1/photos/demo-001/thumbnail/800".into(),
        }
    }
    fn block_on<F: Future>(f: F) -> F::Output {
        unsafe fn clone(_: *const ()) -> RawWaker {
            raw()
        }
        unsafe fn nop(_: *const ()) {}
        fn raw() -> RawWaker {
            RawWaker::new(std::ptr::null(), &RawWakerVTable::new(clone, nop, nop, nop))
        }
        let w = unsafe { Waker::from_raw(raw()) };
        let mut c = Context::from_waker(&w);
        let mut f = std::pin::pin!(f);
        match f.as_mut().poll(&mut c) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("pending"),
        }
    }
    #[test]
    fn loads_ready_and_publishes_only_when_dispatcher_runs() {
        let view = Arc::new(FakeView::default());
        let q = Arc::new(Queue::default());
        let c = DemoController::new(view.clone(), q.clone());
        let t = c.begin_load("demo");
        let api = FakeClient {
            photos: vec![photo()],
            image: Ok(vec![1, 2]),
        };
        block_on(c.execute(&api, t)).unwrap();
        assert_eq!(c.snapshot(), ViewState::Loading);
        q.run_all();
        assert!(
            matches!(c.snapshot(),ViewState::Ready{photo:DemoPhoto{photo_id,..},jpeg:Some(jpeg),preview_error:None,..} if photo_id=="demo-001"&&jpeg==vec![1,2])
        );
    }
    #[test]
    fn empty_gallery_and_thumbnail_error_keep_gallery_ready() {
        let view = Arc::new(FakeView::default());
        let q = Arc::new(Queue::default());
        let c = DemoController::new(view, q.clone());
        let t = c.begin_load("demo");
        block_on(c.execute(
            &FakeClient {
                photos: vec![],
                image: Ok(vec![]),
            },
            t,
        ))
        .unwrap();
        q.run_all();
        assert_eq!(c.snapshot(), ViewState::Empty);
        let t = c.retry("demo");
        block_on(c.execute(
            &FakeClient {
                photos: vec![photo()],
                image: Err(DemoError {
                    message: "bad preview".into(),
                }),
            },
            t,
        ))
        .unwrap();
        q.run_all();
        assert!(
            matches!(c.snapshot(),ViewState::Ready{photo, jpeg:None, preview_error:Some(message),..} if photo.photo_id=="demo-001"&&message=="bad preview")
        );
    }
    #[test]
    fn delayed_old_gallery_result_cannot_replace_new_load() {
        let view = Arc::new(FakeView::default());
        let q = Arc::new(Queue::default());
        let c = DemoController::new(view, q.clone());
        let old = c.begin_load("old");
        let fresh = c.begin_load("demo");
        block_on(c.execute(
            &FakeClient {
                photos: vec![photo()],
                image: Ok(vec![1]),
            },
            old,
        ))
        .unwrap();
        block_on(c.execute(
            &FakeClient {
                photos: vec![photo()],
                image: Ok(vec![2]),
            },
            fresh,
        ))
        .unwrap();
        q.run_all();
        assert!(matches!(c.snapshot(),ViewState::Ready{jpeg:Some(jpeg),..} if jpeg==vec![2]));
    }
}
