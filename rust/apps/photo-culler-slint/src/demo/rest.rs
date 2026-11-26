//! The same typed REST client and presentation controller are used by desktop
//! and browser adapters. It reads bounded pages from the persistent catalog
//! API and keeps a narrow compatibility path for the original fixture.

use crate::demo::controller::{
    DemoClient as ControllerClient, DemoError, DemoFuture, DemoGallery, DemoPhoto, ThumbnailFuture,
};
use photo_culler_api::thumbnail_path;
use photo_culler_client::demo::previews::{PreviewClient, PreviewSize};
use photo_culler_client::demo::{
    AnalysisJob, CatalogCursor, CatalogFilter, ClientError, Decision, DemoClient as RestClient,
    DemoPhoto as RestPhoto, ImportControlAck, ImportEstimate, ImportJob, ImportJobsPage,
    ImportPauseAck, ImportQueueReceipt, ImportResumeReceipt, ImportSourcesPage, Representation,
    RescanReceipt, ScanRevisionsPage, Transport,
};

const DEMO_GALLERY: &str = "demo";
const ACTIVE_GALLERY: &str = "active";
pub const CATALOG_PAGE_LIMIT: usize = 12;
const THUMBNAIL_EDGE: u32 = 800;
const JSON_LIMIT: usize = 64 * 1024;
const THUMBNAIL_LIMIT: usize = 2 * 1024 * 1024;
const PREVIEW_LIMIT: usize = 16 * 1024 * 1024;

pub struct DemoRestClient<T> {
    inner: RestClient<T>,
    previews: PreviewClient<T>,
}

impl<T: Transport + Clone> DemoRestClient<T> {
    pub fn new(transport: T) -> Self {
        Self {
            inner: RestClient::new(transport.clone()).with_limits(JSON_LIMIT, THUMBNAIL_LIMIT),
            previews: PreviewClient::new(transport).with_max_response_bytes(PREVIEW_LIMIT),
        }
    }
}

impl<T: Transport> DemoRestClient<T> {
    pub async fn import_sources(&self, gallery_id: &str) -> Result<ImportSourcesPage, DemoError> {
        self.inner
            .import_sources(gallery_id)
            .await
            .map_err(map_import_client_error)
    }

    pub async fn estimate_import(
        &self,
        source_id: &str,
        recursive: bool,
        exclude_patterns: &[String],
    ) -> Result<ImportEstimate, DemoError> {
        self.inner
            .estimate_import(source_id, recursive, exclude_patterns)
            .await
            .map_err(map_client_error)
    }

    pub async fn start_import(
        &self,
        gallery_id: &str,
        source_id: &str,
        recursive: bool,
        exclude_patterns: &[String],
    ) -> Result<ImportQueueReceipt, DemoError> {
        self.inner
            .start_import(gallery_id, source_id, recursive, exclude_patterns)
            .await
            .map_err(map_client_error)
    }

    pub async fn import_job(&self, job_id: &str) -> Result<ImportJob, DemoError> {
        self.inner
            .import_job(job_id)
            .await
            .map_err(map_client_error)
    }

    pub async fn list_import_jobs(&self, limit: usize) -> Result<ImportJobsPage, DemoError> {
        self.inner
            .list_import_jobs(limit)
            .await
            .map_err(map_client_error)
    }

    pub async fn list_active_import_jobs(&self, limit: usize) -> Result<ImportJobsPage, DemoError> {
        self.inner
            .list_active_import_jobs(limit)
            .await
            .map_err(map_import_client_error)
    }

    pub async fn list_scan_revisions(
        &self,
        gallery_id: Option<&str>,
        limit: usize,
    ) -> Result<ScanRevisionsPage, DemoError> {
        self.inner
            .list_scan_revisions(gallery_id, limit)
            .await
            .map_err(map_client_error)
    }

    pub async fn rescan_gallery(&self, gallery_id: &str) -> Result<RescanReceipt, DemoError> {
        self.inner
            .rescan_gallery(gallery_id)
            .await
            .map_err(map_client_error)
    }

    pub async fn pause_import_job(&self, job_id: &str) -> Result<ImportPauseAck, DemoError> {
        self.inner
            .pause_import_job(job_id)
            .await
            .map_err(map_client_error)
    }

    pub async fn cancel_import_job(&self, job_id: &str) -> Result<ImportControlAck, DemoError> {
        self.inner
            .cancel_import_job(job_id)
            .await
            .map_err(map_client_error)
    }

    pub async fn resume_import_job(&self, job_id: &str) -> Result<ImportResumeReceipt, DemoError> {
        self.inner
            .resume_import_job(job_id)
            .await
            .map_err(map_client_error)
    }

    pub async fn submit_analysis_job(
        &self,
        gallery_id: &str,
        job_id: &str,
        profile_id: &str,
    ) -> Result<AnalysisJob, DemoError> {
        self.inner
            .submit_analysis_job(gallery_id, job_id, profile_id)
            .await
            .map_err(map_client_error)
    }

    pub async fn active_analysis_job(
        &self,
        gallery_id: &str,
    ) -> Result<Option<AnalysisJob>, DemoError> {
        self.inner
            .active_analysis_job(gallery_id)
            .await
            .map_err(map_client_error)
    }

    pub async fn analysis_job(
        &self,
        catalog_id: &str,
        job_id: &str,
    ) -> Result<AnalysisJob, DemoError> {
        self.inner
            .analysis_job(catalog_id, job_id)
            .await
            .map_err(map_client_error)
    }

    pub async fn start_analysis_job(
        &self,
        catalog_id: &str,
        job_id: &str,
    ) -> Result<AnalysisJob, DemoError> {
        self.inner
            .start_analysis_job(catalog_id, job_id)
            .await
            .map_err(map_client_error)
    }

    pub async fn pause_analysis_job(
        &self,
        catalog_id: &str,
        job_id: &str,
    ) -> Result<AnalysisJob, DemoError> {
        self.inner
            .pause_analysis_job(catalog_id, job_id)
            .await
            .map_err(map_client_error)
    }

    pub async fn resume_analysis_job(
        &self,
        catalog_id: &str,
        job_id: &str,
    ) -> Result<AnalysisJob, DemoError> {
        self.inner
            .resume_analysis_job(catalog_id, job_id)
            .await
            .map_err(map_client_error)
    }

    pub async fn cancel_analysis_job(
        &self,
        catalog_id: &str,
        job_id: &str,
    ) -> Result<AnalysisJob, DemoError> {
        self.inner
            .cancel_analysis_job(catalog_id, job_id)
            .await
            .map_err(map_client_error)
    }
}

impl<T: Transport> ControllerClient for DemoRestClient<T> {
    fn gallery<'a>(
        &'a self,
        gallery_id: &'a str,
        cursor: Option<&'a CatalogCursor>,
    ) -> DemoFuture<'a> {
        Box::pin(async move {
            self.inner.health().await.map_err(map_client_error)?;
            let resolved_gallery_id = if gallery_id == ACTIVE_GALLERY && cursor.is_none() {
                match self.inner.active_gallery().await {
                    Ok(Some(id)) => id,
                    Ok(None) => {
                        return Ok(DemoGallery {
                            gallery_id: String::new(),
                            photos: vec![],
                            next_cursor: None,
                        });
                    }
                    // The fixture-only server predates the active-gallery route.
                    Err(ClientError::Http { status: 404, .. }) => DEMO_GALLERY.to_owned(),
                    Err(error) => return Err(map_client_error(error)),
                }
            } else {
                gallery_id.to_owned()
            };

            match self
                .inner
                .catalog_gallery_page(&resolved_gallery_id, cursor, CATALOG_PAGE_LIMIT)
                .await
            {
                Ok(page) => catalog_gallery(resolved_gallery_id, page),
                // Old fixture servers reject query parameters. Do not hide other
                // client errors behind the compatibility route.
                Err(ClientError::Http {
                    status: 400,
                    body: Some(body),
                }) if cursor.is_none()
                    && body.code == "bad_request"
                    && body.message == "Query parameters are not supported" =>
                {
                    legacy_fixture_gallery(&self.inner, &resolved_gallery_id).await
                }
                Err(error) => Err(map_client_error(error)),
            }
        })
    }

    fn thumbnail<'a>(&'a self, photo: &'a DemoPhoto) -> ThumbnailFuture<'a> {
        Box::pin(async move {
            let expected =
                thumbnail_path(&photo.photo_id, THUMBNAIL_EDGE).map_err(|_| DemoError {
                    message: "La ruta de la miniatura no es válida.".into(),
                })?;
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
                    "alt" => Decision::Alt,
                    "reject" => Decision::Reject,
                    _ => Decision::Undecided,
                },
                representation: Representation::Jpeg,
                thumbnail_url: photo.thumbnail_url.clone(),
            };
            match self
                .previews
                .get(&photo.photo_id, PreviewSize::Medium)
                .await
            {
                Ok(jpeg) => Ok(jpeg),
                // Fixture servers predate the persistent preview route. Keep
                // their established thumbnail path as a narrow fallback.
                Err(ClientError::Http { status: 404, .. }) => self
                    .inner
                    .thumbnail(&rest_photo)
                    .await
                    .map_err(map_client_error),
                Err(error) => Err(map_client_error(error)),
            }
        })
    }

    fn update_decision<'a>(
        &'a self,
        photo: &'a DemoPhoto,
        decision: Decision,
        request_id: &'a str,
    ) -> super::controller::DecisionFuture<'a> {
        Box::pin(async move {
            let receipt = self
                .inner
                .update_decision(
                    &photo.photo_id,
                    decision,
                    photo.decision_revision,
                    request_id,
                )
                .await
                .map_err(map_client_error)?;
            Ok(receipt.revision)
        })
    }
}

fn catalog_gallery(
    gallery_id: String,
    page: photo_culler_client::demo::CatalogPhotoPage,
) -> Result<DemoGallery, DemoError> {
    if page.contract_version != 1 || page.items.len() > CATALOG_PAGE_LIMIT {
        return Err(invalid_gallery_response());
    }
    if let Some(cursor) = &page.next_cursor {
        if cursor.filter
            != (CatalogFilter {
                gallery_id: gallery_id.clone(),
                decision: None,
            })
            || page
                .items
                .last()
                .is_none_or(|item| item.id != cursor.after_id)
        {
            return Err(invalid_gallery_response());
        }
    }

    let mut photos = Vec::with_capacity(page.items.len());
    let mut previous_id: Option<String> = None;
    for item in page.items {
        if item.id.is_empty()
            || previous_id
                .as_ref()
                .is_some_and(|previous| previous >= &item.id)
        {
            return Err(invalid_gallery_response());
        }
        previous_id = Some(item.id.clone());
        let expected =
            thumbnail_path(&item.id, THUMBNAIL_EDGE).map_err(|_| invalid_gallery_response())?;
        if item.thumbnail_url != expected {
            return Err(invalid_gallery_response());
        }
        photos.push(DemoPhoto {
            photo_id: item.id,
            filename: item.name,
            decision: catalog_decision_name(&item.decision).into(),
            thumbnail_url: item.thumbnail_url,
            decision_revision: item.decision_revision,
        });
    }

    Ok(DemoGallery {
        gallery_id,
        photos,
        next_cursor: page.next_cursor,
    })
}

async fn legacy_fixture_gallery<T: Transport>(
    client: &RestClient<T>,
    gallery_id: &str,
) -> Result<DemoGallery, DemoError> {
    let page = client.gallery(gallery_id).await.map_err(map_client_error)?;
    if page.gallery_id != gallery_id
        || page.next_cursor.is_some()
        || page.total != page.items.len() as u64
        || page.items.len() > CATALOG_PAGE_LIMIT
    {
        return Err(invalid_gallery_response());
    }
    let mut photos = Vec::with_capacity(page.items.len());
    for photo in page.items {
        let expected = thumbnail_path(&photo.photo_id, THUMBNAIL_EDGE)
            .map_err(|_| invalid_gallery_response())?;
        if photo.thumbnail_url != expected {
            return Err(invalid_gallery_response());
        }
        photos.push(DemoPhoto {
            photo_id: photo.photo_id,
            filename: photo.filename,
            decision: legacy_decision_name(&photo.decision).into(),
            thumbnail_url: photo.thumbnail_url,
            decision_revision: 0,
        });
    }
    Ok(DemoGallery {
        gallery_id: page.gallery_id,
        photos,
        next_cursor: None,
    })
}

fn invalid_gallery_response() -> DemoError {
    DemoError {
        message: "El servicio devolvió una página de galería no válida.".into(),
    }
}

fn catalog_decision_name(decision: &str) -> &'static str {
    match decision.to_ascii_lowercase().as_str() {
        "keep" | "best" | "protected_by_coverage" => "keep",
        "alt" | "alternate" => "alt",
        "reject" | "reject_technical" | "reject_redundant" => "reject",
        _ => "undecided",
    }
}

fn legacy_decision_name(decision: &Decision) -> &'static str {
    match decision {
        Decision::Keep => "keep",
        Decision::Alt => "alt",
        Decision::Reject => "reject",
        Decision::Undecided => "undecided",
    }
}

fn map_client_error(error: ClientError) -> DemoError {
    let message = match error {
        ClientError::Http { status, body } => {
            let version_conflict = status == 409
                && body
                    .as_ref()
                    .is_some_and(|body| body.code == "version_conflict");
            let context = body.map(|body| match body.request_id {
                Some(request_id) => format!("{} · {}", body.code, request_id),
                None => body.code,
            });
            if version_conflict {
                return DemoError {
                    message: format!(
                        "La decisión cambió en otro lugar (HTTP 409; {}). Recarga la galería antes de volver a intentarlo.",
                        context.unwrap_or_else(|| "conflict".into())
                    ),
                };
            }
            match context {
                Some(context) => {
                    format!("Falló la solicitud al servicio local (HTTP {status}; {context}).")
                }
                None => format!("Falló la solicitud al servicio local (HTTP {status})."),
            }
        }
        ClientError::Transport(detail) => {
            // The hosted desktop acceptance log needs the transport failure
            // class to diagnose connectivity without exposing it in the UI.
            if std::env::var_os("PHOTO_CULLER_SMOKE_STATUS_FILE").is_some() {
                eprintln!("Photo Culler smoke transport detail: {detail}");
            }
            "No se pudo conectar con el servicio local.".into()
        }
        ClientError::Redirect { .. } | ClientError::Redirected => {
            "El servicio local redirigió una solicitud inesperadamente.".into()
        }
        ClientError::InvalidMime { .. } => {
            "El servicio local devolvió un formato de respuesta inesperado.".into()
        }
        ClientError::BodyTooLarge { .. } => {
            "La respuesta del servicio local supera el tamaño permitido.".into()
        }
        ClientError::InvalidBody(_) | ClientError::UnsafePath => {
            "El servicio local devolvió una respuesta no válida.".into()
        }
    };
    DemoError { message }
}

fn map_import_client_error(error: ClientError) -> DemoError {
    match error {
        ClientError::Http { status: 404, .. } => DemoError {
            message: "La importación local no está habilitada. Añade --authorized-root <carpeta> a los flags del servidor, reinícialo y vuelve a abrir Import.".into(),
        },
        error => map_client_error(error),
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

    #[derive(Clone)]
    struct FakeTransport {
        responses: Arc<Mutex<VecDeque<Result<Response, TransportError>>>>,
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
    fn user_facing_http_error_keeps_safe_correlation_details() {
        let error = ClientError::Http {
            status: 503,
            body: Some(photo_culler_client::demo::ApiErrorBody {
                code: "service_unavailable".into(),
                message: "internal path /private/catalog.db".into(),
                request_id: Some("req-123".into()),
            }),
        };

        let message = map_client_error(error).message;

        assert_eq!(
            message,
            "Falló la solicitud al servicio local (HTTP 503; service_unavailable · req-123)."
        );
        assert!(!message.contains("/private/catalog.db"));
        assert!(!message.contains("Http {"));
    }

    #[test]
    fn maps_typed_rest_dtos_and_enforces_the_canonical_gallery_contract() {
        let transport = FakeTransport {
            responses: Arc::new(Mutex::new(VecDeque::from([
                Ok(response(
                    "application/json",
                    br#"{"status":"ready","resources":null,"request_id":"req-demo"}"#,
                )),
                Ok(response(
                    "application/json",
                    br#"{"contract_version":1,"items":[{"id":"demo-001","name":"demo-001.jpg","decision":"undecided","decision_revision":0,"score":null,"quality_tier":null,"thumbnail_url":"/api/v1/photos/demo-001/thumbnail/800"}],"next_cursor":null}"#,
                )),
                Ok(response("image/jpeg", &[0xff, 0xd8, 0xff, 0xd9])),
            ]))),
        };
        let client = DemoRestClient::new(transport);
        poll_ready(async {
            let gallery = client.gallery("demo", None, None).await.unwrap();
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
            responses: Arc::new(Mutex::new(VecDeque::from([
                Ok(response(
                    "application/json",
                    br#"{"status":"ready","resources":null,"request_id":"req-demo"}"#,
                )),
                Ok(response(
                    "application/json",
                    br#"{"contract_version":1,"items":[{"id":"demo-001","name":"demo-001.jpg","decision":"undecided","decision_revision":0,"score":null,"quality_tier":null,"thumbnail_url":"https://example.invalid/image.jpg"}],"next_cursor":null}"#,
                )),
            ]))),
        };
        let client = DemoRestClient::new(transport);
        let result = poll_ready(client.gallery("demo", None, None));
        assert!(matches!(result, Err(DemoError { .. })));
    }
}
