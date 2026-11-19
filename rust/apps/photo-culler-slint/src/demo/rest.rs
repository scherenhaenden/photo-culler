//! The same typed REST client and presentation controller are used by desktop
//! and browser adapters. This module limits the sprint demo to the included
//! read-only gallery and its canonical thumbnail route.

use crate::demo::controller::{
    DemoClient as ControllerClient, DemoError, DemoFuture, DemoGallery, DemoPhoto, ThumbnailFuture,
};
use photo_culler_api::thumbnail_path;
use photo_culler_client::demo::{
    Decision, DemoClient as RestClient, DemoPhoto as RestPhoto, Representation, Transport,
};

const DEMO_GALLERY: &str = "demo";
const THUMBNAIL_EDGE: u32 = 800;
const JSON_LIMIT: usize = 64 * 1024;
const THUMBNAIL_LIMIT: usize = 2 * 1024 * 1024;

pub struct DemoRestClient<T> {
    inner: RestClient<T>,
}

impl<T: Transport> DemoRestClient<T> {
    pub fn new(transport: T) -> Self {
        Self {
            inner: RestClient::new(transport).with_limits(JSON_LIMIT, THUMBNAIL_LIMIT),
        }
    }
}

impl<T: Transport> ControllerClient for DemoRestClient<T> {
    fn gallery<'a>(&'a self, gallery_id: &'a str) -> DemoFuture<'a> {
        Box::pin(async move {
            if gallery_id != DEMO_GALLERY {
                return Err(DemoError {
                    message: "Only the included demo gallery is available in this sprint.".into(),
                });
            }

            self.inner.health().await.map_err(map_error)?;
            let page = self.inner.gallery(gallery_id).await.map_err(map_error)?;
            if page.gallery_id != DEMO_GALLERY
                || page.next_cursor.is_some()
                || page.total != page.items.len() as u64
            {
                return Err(DemoError {
                    message: "The service returned an invalid demo gallery response.".into(),
                });
            }

            for photo in &page.items {
                let expected =
                    thumbnail_path(&photo.photo_id, THUMBNAIL_EDGE).map_err(map_error)?;
                if photo.thumbnail_url != expected {
                    return Err(DemoError {
                        message: "The service returned a non-canonical thumbnail route.".into(),
                    });
                }
            }

            Ok(DemoGallery {
                gallery_id: page.gallery_id,
                photos: page
                    .items
                    .into_iter()
                    .map(|photo| DemoPhoto {
                        photo_id: photo.photo_id,
                        filename: photo.filename,
                        decision: decision_name(&photo.decision).into(),
                        thumbnail_url: photo.thumbnail_url,
                    })
                    .collect(),
            })
        })
    }

    fn thumbnail<'a>(&'a self, photo: &'a DemoPhoto) -> ThumbnailFuture<'a> {
        Box::pin(async move {
            let expected = thumbnail_path(&photo.photo_id, THUMBNAIL_EDGE).map_err(map_error)?;
            if photo.thumbnail_url != expected {
                return Err(DemoError {
                    message: "The requested thumbnail route is invalid.".into(),
                });
            }

            let rest_photo = RestPhoto {
                photo_id: photo.photo_id.clone(),
                filename: photo.filename.clone(),
                decision: match photo.decision.as_str() {
                    "keep" => Decision::Keep,
                    "reject" => Decision::Reject,
                    _ => Decision::Undecided,
                },
                representation: Representation::Jpeg,
                thumbnail_url: photo.thumbnail_url.clone(),
            };
            self.inner.thumbnail(&rest_photo).await.map_err(map_error)
        })
    }
}

fn decision_name(decision: &Decision) -> &'static str {
    match decision {
        Decision::Keep => "keep",
        Decision::Reject => "reject",
        Decision::Undecided => "undecided",
    }
}

fn map_error(error: impl std::fmt::Debug) -> DemoError {
    DemoError {
        message: format!("{error:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use photo_culler_client::demo::{
        RequestFuture, Response, Transport as RestTransport, TransportError,
    };
    use std::{
        collections::VecDeque,
        future,
        sync::{Arc, Mutex},
        task::{Context, Poll, Wake, Waker},
    };

    struct NoopWake;
    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    fn poll_ready<F: std::future::Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("fake REST transport must complete immediately"),
        }
    }

    struct FakeTransport {
        responses: Mutex<VecDeque<Result<Response, TransportError>>>,
    }

    impl RestTransport for FakeTransport {
        fn get<'a>(&'a self, _: &'a str, _: usize) -> RequestFuture<'a> {
            let response = self.responses.lock().unwrap().pop_front().unwrap();
            Box::pin(future::ready(response))
        }
    }

    fn response(content_type: &str, body: &[u8]) -> Response {
        Response {
            status: 200,
            content_type: Some(content_type.into()),
            body: body.into(),
            redirected: false,
        }
    }

    #[test]
    fn maps_typed_rest_dtos_and_enforces_the_canonical_gallery_contract() {
        let transport = FakeTransport {
            responses: Mutex::new(VecDeque::from([
                Ok(response(
                    "application/json",
                    br#"{"status":"ready","resources":null,"request_id":"req-demo"}"#,
                )),
                Ok(response(
                    "application/json",
                    br#"{"gallery_id":"demo","items":[{"photo_id":"demo-001","filename":"demo-001.jpg","decision":"undecided","representation":"jpeg","thumbnail_url":"/api/v1/photos/demo-001/thumbnail/800"}],"next_cursor":null,"total":1}"#,
                )),
                Ok(response("image/jpeg", &[0xff, 0xd8, 0xff, 0xd9])),
            ])),
        };
        let client = DemoRestClient::new(transport);
        poll_ready(async {
            let gallery = client.gallery("demo").await.unwrap();
            assert_eq!(gallery.photos[0].filename, "demo-001.jpg");
            assert_eq!(gallery.photos[0].decision, "undecided");
            assert_eq!(
                client.thumbnail(&gallery.photos[0]).await.unwrap(),
                [0xff, 0xd8, 0xff, 0xd9]
            );
        });
    }

    #[test]
    fn rejects_a_gallery_thumbnail_that_changes_the_route() {
        let transport = FakeTransport {
            responses: Mutex::new(VecDeque::from([
                Ok(response(
                    "application/json",
                    br#"{"status":"ready","resources":null,"request_id":"req-demo"}"#,
                )),
                Ok(response(
                    "application/json",
                    br#"{"gallery_id":"demo","items":[{"photo_id":"demo-001","filename":"demo-001.jpg","decision":"undecided","representation":"jpeg","thumbnail_url":"https://example.invalid/image.jpg"}],"next_cursor":null,"total":1}"#,
                )),
            ])),
        };
        let client = DemoRestClient::new(transport);
        let result = poll_ready(client.gallery("demo"));
        assert!(matches!(result, Err(DemoError { .. })));
    }
}
