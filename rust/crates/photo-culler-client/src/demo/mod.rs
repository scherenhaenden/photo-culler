//! Async, same-origin REST client for the Rust/Slint gallery, imports and analysis.
pub mod previews;

use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin};

pub const API_PREFIX: &str = "/api/v1";
pub const IMPORT_API_PREFIX: &str = "/api/v2";
pub const DEFAULT_MAX_JSON_BYTES: usize = 1_048_576;
const DEFAULT_MAX_IMPORT_JSON_BYTES: usize = 1_048_576;
pub const DEFAULT_MAX_THUMBNAIL_BYTES: usize = 20 * 1_048_576;
// Fifty 256-character exclusion patterns can exceed 16 KiB in UTF-8 or
// JSON escapes. This accommodates the valid contract while bounding writes.
pub const MAX_IMPORT_REQUEST_BYTES: usize = 128 * 1_024;
pub const MAX_IMPORT_PAGE_ITEMS: usize = 100;
pub const MAX_ANALYSIS_REQUEST_BYTES: usize = 4 * 1_024;

pub type RequestFuture<'a> = Pin<Box<dyn Future<Output = Result<Response, TransportError>> + 'a>>;

/// Adapters make one same-origin request for each method and disable redirect
/// following. The core validates status, MIME, paths, and body bounds itself.
pub trait Transport {
    fn get<'a>(&'a self, relative_path: &'a str, max_response_bytes: usize) -> RequestFuture<'a>;

    fn put_json<'a>(
        &'a self,
        _relative_path: &'a str,
        _body: Vec<u8>,
        _max_response_bytes: usize,
    ) -> RequestFuture<'a> {
        Box::pin(async {
            Err(TransportError(
                "this transport does not support JSON writes".into(),
            ))
        })
    }

    fn post_json<'a>(
        &'a self,
        _relative_path: &'a str,
        _body: Vec<u8>,
        _max_response_bytes: usize,
    ) -> RequestFuture<'a> {
        Box::pin(async {
            Err(TransportError(
                "this transport does not support JSON POST requests".into(),
            ))
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    /// Adapters disable automatic redirects and report any redirect here.
    pub redirected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportError(pub String);

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub request_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientError {
    Transport(String),
    Http {
        status: u16,
        body: Option<ApiErrorBody>,
    },
    Redirect {
        status: u16,
    },
    Redirected,
    InvalidMime {
        expected: &'static str,
        actual: Option<String>,
    },
    BodyTooLarge {
        limit: usize,
        actual: usize,
    },
    InvalidBody(String),
    UnsafePath,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Keep,
    Alt,
    Reject,
    Undecided,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DemoPhoto {
    pub photo_id: String,
    pub filename: String,
    pub decision: Decision,
    pub representation: Representation,
    pub thumbnail_url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Representation {
    Jpeg,
    Raw,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct GalleryPage {
    pub gallery_id: String,
    pub items: Vec<DemoPhoto>,
    pub next_cursor: Option<String>,
    pub total: u64,
}

/// Stable catalog-read contract returned by the persistent server routes.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CatalogPhotoPage {
    pub contract_version: u8,
    pub items: Vec<CatalogPhotoItem>,
    pub next_cursor: Option<CatalogCursor>,
}

/// Whole-catalog aggregates returned by the persistent gallery summary route.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CatalogSummary {
    pub total_photos: u64,
    pub total_files: u64,
    pub selected_count: u64,
    pub pending_count: u64,
    pub decisions: std::collections::BTreeMap<String, u64>,
    pub quality_tiers: std::collections::BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CatalogPhotoItem {
    pub id: String,
    pub name: String,
    pub decision: String,
    pub decision_revision: u64,
    pub score: Option<f64>,
    pub quality_tier: Option<String>,
    pub thumbnail_url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct CatalogCursor {
    pub filter: CatalogFilter,
    pub after_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct CatalogFilter {
    pub gallery_id: String,
    pub decision: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DecisionReceipt {
    pub contract_version: u8,
    pub photo_id: String,
    pub decision: Decision,
    pub revision: u64,
    pub request_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ImportSource {
    pub contract_version: u8,
    pub id: String,
    pub name: String,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ImportSourcesPage {
    pub contract_version: u8,
    pub items: Vec<ImportSource>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ImportEstimate {
    pub contract_version: u8,
    pub source_id: String,
    pub total_files: u64,
    pub total_bytes: u64,
    pub logical_photos: u64,
    pub extensions: std::collections::BTreeMap<String, u64>,
    pub roles: std::collections::BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ImportQueueReceipt {
    pub contract_version: u8,
    pub job_id: String,
    pub state: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ImportJob {
    pub contract_version: u8,
    pub id: String,
    pub gallery_id: String,
    pub scan_revision_id: Option<String>,
    pub source_id: String,
    pub state: String,
    pub discovered: u64,
    pub imported: u64,
    pub issues: u64,
    pub cancel_requested: bool,
    pub pause_requested: bool,
    pub updated_at: String,
    pub revision: u64,
    pub processed: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ImportJobsPage {
    pub contract_version: u8,
    pub items: Vec<ImportJob>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ScanRevision {
    pub contract_version: u8,
    pub id: String,
    pub gallery_id: String,
    pub source_id: String,
    pub state: String,
    pub discovered: u64,
    pub new_files: u64,
    pub modified_files: u64,
    pub moved_files: u64,
    pub missing_files: u64,
    pub started_at: String,
    pub completed_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ScanRevisionsPage {
    pub contract_version: u8,
    pub items: Vec<ScanRevision>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct RescanReceipt {
    pub contract_version: u8,
    pub gallery_id: String,
    pub job_ids: Vec<String>,
    pub offline_source_ids: Vec<String>,
    /// Online sources deferred after an unavailable service error.
    #[serde(default)]
    pub deferred_source_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ImportControlAck {
    pub contract_version: u8,
    pub job_id: String,
    pub cancel_requested: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ImportPauseAck {
    pub contract_version: u8,
    pub job_id: String,
    pub pause_requested: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ImportResumeReceipt {
    pub contract_version: u8,
    pub job_id: String,
    pub state: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct AnalysisJob {
    pub contract_version: u8,
    pub catalog_id: String,
    pub job_id: String,
    pub gallery_id: String,
    pub profile_id: String,
    pub state: String,
    pub completed: u64,
    pub total: u64,
    pub failed: u64,
    pub percent: u8,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ActiveAnalysisJob {
    pub contract_version: u8,
    pub active_job: Option<AnalysisJob>,
}

#[derive(Serialize)]
struct SubmitAnalysisRequest<'a> {
    job_id: &'a str,
    profile_id: &'a str,
}

#[derive(Serialize)]
struct ImportEstimateRequest<'a> {
    source_id: &'a str,
    recursive: bool,
    exclude_patterns: &'a [String],
}

#[derive(Serialize)]
struct StartImportRequest<'a> {
    source_id: &'a str,
    recursive: bool,
    exclude_patterns: &'a [String],
}

#[derive(Clone, Debug)]
pub struct DemoClient<T> {
    transport: T,
    max_json_bytes: usize,
    max_import_json_bytes: usize,
    max_thumbnail_bytes: usize,
}

impl<T: Transport> DemoClient<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            max_json_bytes: DEFAULT_MAX_JSON_BYTES,
            max_import_json_bytes: DEFAULT_MAX_IMPORT_JSON_BYTES,
            max_thumbnail_bytes: DEFAULT_MAX_THUMBNAIL_BYTES,
        }
    }

    pub fn with_limits(mut self, max_json_bytes: usize, max_thumbnail_bytes: usize) -> Self {
        self.max_json_bytes = max_json_bytes;
        self.max_thumbnail_bytes = max_thumbnail_bytes;
        self
    }

    fn json_limit_for_path(&self, path: &str) -> usize {
        if path.starts_with("/api/v2/") {
            self.max_import_json_bytes
        } else {
            self.max_json_bytes
        }
    }

    pub async fn health(&self) -> Result<(), ClientError> {
        #[derive(Deserialize)]
        struct Health {
            status: String,
        }
        let health: Health = self.json("/api/v1/health").await?;
        if health.status == "ready" {
            Ok(())
        } else {
            Err(ClientError::InvalidBody("service is not ready".into()))
        }
    }

    pub async fn gallery(&self, gallery_id: &str) -> Result<GalleryPage, ClientError> {
        let path = format!(
            "{API_PREFIX}/galleries/{}/photos",
            encode_segment(gallery_id)
        );
        self.json(&path).await
    }

    pub async fn active_gallery(&self) -> Result<Option<String>, ClientError> {
        #[derive(Deserialize)]
        struct ActiveGallery {
            contract_version: u8,
            gallery_id: Option<String>,
        }
        let active: ActiveGallery = self.json("/api/v1/galleries/active").await?;
        if active.contract_version != 1 {
            return Err(ClientError::InvalidBody(
                "unsupported active gallery contract version".into(),
            ));
        }
        Ok(active.gallery_id)
    }

    /// Fetch one page from the persistent catalog API. The opaque cursor is
    /// serialized as JSON and percent-encoded before it enters the query.
    pub async fn catalog_gallery_page(
        &self,
        filter: &CatalogFilter,
        cursor: Option<&CatalogCursor>,
        limit: usize,
    ) -> Result<CatalogPhotoPage, ClientError> {
        if !(1..=200).contains(&limit) {
            return Err(ClientError::InvalidBody("invalid page limit".into()));
        }
        if filter.gallery_id.is_empty()
            || cursor.is_some_and(|cursor| cursor.filter != *filter)
        {
            return Err(ClientError::InvalidBody(
                "catalog cursor does not match its filter".into(),
            ));
        }
        let mut path = format!(
            "{API_PREFIX}/galleries/{}/photos?limit={limit}",
            encode_segment(&filter.gallery_id)
        );
        if let Some(decision) = &filter.decision {
            path.push_str("&decision=");
            path.push_str(&encode_query_value(decision));
        }
        if let Some(cursor) = cursor {
            let serialized = serde_json::to_string(cursor)
                .map_err(|error| ClientError::InvalidBody(error.to_string()))?;
            path.push_str("&cursor=");
            path.push_str(&encode_query_value(&serialized));
        }
        self.json(&path).await
    }

    /// Fetch decision and quality aggregates for the whole gallery.
    pub async fn catalog_summary(&self, gallery_id: &str) -> Result<CatalogSummary, ClientError> {
        if gallery_id.is_empty() {
            return Err(ClientError::InvalidBody("invalid gallery ID".into()));
        }
        let path = format!(
            "{API_PREFIX}/galleries/{}/summary",
            encode_segment(gallery_id)
        );
        self.json(&path).await
    }

    /// Persist one explicit review decision using the server's revision check.
    pub async fn update_decision(
        &self,
        photo_id: &str,
        decision: Decision,
        expected_revision: u64,
        request_id: &str,
    ) -> Result<DecisionReceipt, ClientError> {
        if photo_id.is_empty()
            || photo_id.len() > 256
            || request_id.trim().is_empty()
            || request_id.len() > 128
        {
            return Err(ClientError::InvalidBody(
                "invalid decision write request".into(),
            ));
        }
        let path = format!("{API_PREFIX}/photos/{}/decision", encode_segment(photo_id));
        validate_api_path(&path)?;
        let body = serde_json::to_vec(&DecisionWriteRequest {
            decision,
            expected_revision,
            request_id: request_id.trim(),
        })
        .map_err(|error| ClientError::InvalidBody(error.to_string()))?;
        if body.len() > 4 * 1024 {
            return Err(ClientError::BodyTooLarge {
                limit: 4 * 1024,
                actual: body.len(),
            });
        }
        let response = self
            .transport
            .put_json(&path, body, self.max_json_bytes)
            .await
            .map_err(|error| ClientError::Transport(error.0))?;
        if response.body.len() > self.max_json_bytes {
            return Err(ClientError::BodyTooLarge {
                limit: self.max_json_bytes,
                actual: response.body.len(),
            });
        }
        if response.redirected {
            return Err(ClientError::Redirected);
        }
        if (300..400).contains(&response.status) {
            return Err(ClientError::Redirect {
                status: response.status,
            });
        }
        if !(200..300).contains(&response.status) {
            return Err(http_error(response));
        }
        if !mime_is(&response.content_type, "application/json") {
            return Err(ClientError::InvalidMime {
                expected: "application/json",
                actual: response.content_type,
            });
        }
        let receipt: DecisionReceipt = serde_json::from_slice(&response.body)
            .map_err(|error| ClientError::InvalidBody(error.to_string()))?;
        if receipt.contract_version != 1
            || receipt.photo_id != photo_id
            || receipt.decision != decision
            || receipt.request_id != request_id.trim()
            || receipt.revision <= expected_revision
        {
            return Err(ClientError::InvalidBody(
                "decision receipt does not match the requested write".into(),
            ));
        }
        Ok(receipt)
    }

    /// List the server-local import sources registered for a gallery.
    pub async fn import_sources(&self, gallery_id: &str) -> Result<ImportSourcesPage, ClientError> {
        validate_import_id(gallery_id)?;
        let path = format!(
            "{IMPORT_API_PREFIX}/galleries/{}/sources",
            encode_segment(gallery_id)
        );
        let page: ImportSourcesPage = self.json(&path).await?;
        if page.contract_version != 2
            || page.items.iter().any(|source| {
                source.contract_version != 2
                    || !valid_import_id(&source.id)
                    || !matches!(source.status.as_str(), "online" | "offline")
            })
        {
            return Err(ClientError::InvalidBody(
                "invalid import sources response".into(),
            ));
        }
        Ok(page)
    }

    /// Estimate the files and logical photos below one configured source.
    pub async fn estimate_import(
        &self,
        source_id: &str,
        recursive: bool,
        exclude_patterns: &[String],
    ) -> Result<ImportEstimate, ClientError> {
        validate_import_id(source_id)?;
        let patterns = normalize_import_patterns(exclude_patterns)?;
        let body = serde_json::to_vec(&ImportEstimateRequest {
            source_id,
            recursive,
            exclude_patterns: &patterns,
        })
        .map_err(|error| ClientError::InvalidBody(error.to_string()))?;
        let response: ImportEstimate = self.post_json("/api/v2/import-estimates", body).await?;
        if response.contract_version != 2
            || response.source_id != source_id
            || !response
                .extensions
                .values()
                .all(|count| *count <= response.total_files)
            || !response
                .roles
                .values()
                .all(|count| *count <= response.total_files)
        {
            return Err(ClientError::InvalidBody(
                "invalid import estimate response".into(),
            ));
        }
        Ok(response)
    }

    /// Start a durable import from an opaque, previously registered source ID.
    pub async fn start_import(
        &self,
        gallery_id: &str,
        source_id: &str,
        recursive: bool,
        exclude_patterns: &[String],
    ) -> Result<ImportQueueReceipt, ClientError> {
        validate_import_id(gallery_id)?;
        validate_import_id(source_id)?;
        let patterns = normalize_import_patterns(exclude_patterns)?;
        let body = serde_json::to_vec(&StartImportRequest {
            source_id,
            recursive,
            exclude_patterns: &patterns,
        })
        .map_err(|error| ClientError::InvalidBody(error.to_string()))?;
        let path = format!(
            "{IMPORT_API_PREFIX}/galleries/{}/imports",
            encode_segment(gallery_id)
        );
        let receipt: ImportQueueReceipt = self.post_json(&path, body).await?;
        if receipt.contract_version != 2
            || !valid_import_id(&receipt.job_id)
            || receipt.state != "queued"
        {
            return Err(ClientError::InvalidBody(
                "invalid import queue response".into(),
            ));
        }
        Ok(receipt)
    }

    pub async fn list_import_jobs(&self, limit: usize) -> Result<ImportJobsPage, ClientError> {
        validate_import_limit(limit)?;
        let path = format!("{IMPORT_API_PREFIX}/import-jobs?limit={limit}");
        let page: ImportJobsPage = self.json(&path).await?;
        validate_job_page(&page, limit)?;
        Ok(page)
    }

    pub async fn list_active_import_jobs(
        &self,
        limit: usize,
    ) -> Result<ImportJobsPage, ClientError> {
        validate_import_limit(limit)?;
        let path = format!("{IMPORT_API_PREFIX}/import-jobs?limit={limit}&active_only=true");
        let page: ImportJobsPage = self.json(&path).await?;
        validate_job_page(&page, limit)?;
        if page
            .items
            .iter()
            .any(|job| !matches!(job.state.as_str(), "queued" | "running" | "paused"))
        {
            return Err(ClientError::InvalidBody(
                "active import jobs response contains a terminal job".into(),
            ));
        }
        Ok(page)
    }

    pub async fn import_job(&self, job_id: &str) -> Result<ImportJob, ClientError> {
        validate_import_id(job_id)?;
        let path = format!("{IMPORT_API_PREFIX}/import-jobs/{}", encode_segment(job_id));
        let job: ImportJob = self.json(&path).await?;
        validate_import_job(&job)?;
        if job.id != job_id {
            return Err(ClientError::InvalidBody(
                "import job response ID does not match request".into(),
            ));
        }
        Ok(job)
    }

    pub async fn list_scan_revisions(
        &self,
        gallery_id: Option<&str>,
        limit: usize,
    ) -> Result<ScanRevisionsPage, ClientError> {
        validate_import_limit(limit)?;
        let mut path = format!("{IMPORT_API_PREFIX}/scan-revisions?limit={limit}");
        if let Some(gallery_id) = gallery_id {
            validate_import_id(gallery_id)?;
            path.push_str("&gallery_id=");
            path.push_str(&encode_query_value(gallery_id));
        }
        let page: ScanRevisionsPage = self.json(&path).await?;
        if page.contract_version != 2
            || page.items.len() > limit
            || page.items.iter().any(|revision| {
                revision.contract_version != 2
                    || !valid_import_id(&revision.id)
                    || !valid_import_id(&revision.gallery_id)
                    || !valid_import_id(&revision.source_id)
            })
            || gallery_id.is_some_and(|gallery| {
                page.items
                    .iter()
                    .any(|revision| revision.gallery_id != gallery)
            })
        {
            return Err(ClientError::InvalidBody(
                "invalid scan revisions response".into(),
            ));
        }
        Ok(page)
    }

    pub async fn rescan_gallery(&self, gallery_id: &str) -> Result<RescanReceipt, ClientError> {
        validate_import_id(gallery_id)?;
        let path = format!(
            "{IMPORT_API_PREFIX}/galleries/{}/rescan",
            encode_segment(gallery_id)
        );
        let receipt: RescanReceipt = self.post_json(&path, Vec::new()).await?;
        if receipt.contract_version != 2
            || receipt.gallery_id != gallery_id
            || receipt.job_ids.iter().any(|id| !valid_import_id(id))
            || receipt
                .offline_source_ids
                .iter()
                .any(|id| !valid_import_id(id))
            || receipt.deferred_source_ids.iter().any(|id| {
                !valid_import_id(id)
                    || receipt
                        .offline_source_ids
                        .iter()
                        .any(|offline_id| offline_id == id)
            })
        {
            return Err(ClientError::InvalidBody("invalid rescan response".into()));
        }
        Ok(receipt)
    }

    pub async fn pause_import_job(&self, job_id: &str) -> Result<ImportPauseAck, ClientError> {
        validate_import_id(job_id)?;
        let path = format!(
            "{IMPORT_API_PREFIX}/import-jobs/{}/pause",
            encode_segment(job_id)
        );
        let ack: ImportPauseAck = self.post_json(&path, Vec::new()).await?;
        if ack.contract_version != 2 || ack.job_id != job_id || !ack.pause_requested {
            return Err(ClientError::InvalidBody("invalid pause response".into()));
        }
        Ok(ack)
    }

    pub async fn cancel_import_job(&self, job_id: &str) -> Result<ImportControlAck, ClientError> {
        validate_import_id(job_id)?;
        let path = format!(
            "{IMPORT_API_PREFIX}/import-jobs/{}/cancel",
            encode_segment(job_id)
        );
        let ack: ImportControlAck = self.post_json(&path, Vec::new()).await?;
        if ack.contract_version != 2 || ack.job_id != job_id || !ack.cancel_requested {
            return Err(ClientError::InvalidBody("invalid cancel response".into()));
        }
        Ok(ack)
    }

    pub async fn resume_import_job(
        &self,
        job_id: &str,
    ) -> Result<ImportResumeReceipt, ClientError> {
        validate_import_id(job_id)?;
        let path = format!(
            "{IMPORT_API_PREFIX}/import-jobs/{}/resume",
            encode_segment(job_id)
        );
        let receipt: ImportResumeReceipt = self.post_json(&path, Vec::new()).await?;
        if receipt.contract_version != 2 || receipt.job_id != job_id || receipt.state != "queued" {
            return Err(ClientError::InvalidBody("invalid resume response".into()));
        }
        Ok(receipt)
    }

    /// Submit a durable analysis job for one gallery using a server-owned
    /// source plan. Filesystem paths are never accepted from the client.
    pub async fn submit_analysis_job(
        &self,
        gallery_id: &str,
        job_id: &str,
        profile_id: &str,
    ) -> Result<AnalysisJob, ClientError> {
        validate_analysis_id(gallery_id)?;
        validate_analysis_id(job_id)?;
        validate_analysis_id(profile_id)?;
        let path = format!(
            "{API_PREFIX}/galleries/{}/analysis-jobs",
            encode_segment(gallery_id)
        );
        let body = serde_json::to_vec(&SubmitAnalysisRequest { job_id, profile_id })
            .map_err(|error| ClientError::InvalidBody(error.to_string()))?;
        if body.len() > MAX_ANALYSIS_REQUEST_BYTES {
            return Err(ClientError::BodyTooLarge {
                limit: MAX_ANALYSIS_REQUEST_BYTES,
                actual: body.len(),
            });
        }
        let job: AnalysisJob = self.post_json(&path, body).await?;
        validate_analysis_job(&job, gallery_id, job_id)?;
        if job.catalog_id != gallery_id || job.profile_id != profile_id || job.state != "queued" {
            return Err(ClientError::InvalidBody(
                "analysis submission response does not match the request".into(),
            ));
        }
        Ok(job)
    }

    /// Discover one durable queued, running, or paused job for a gallery.
    /// Terminal jobs are excluded so opening Processing without active work
    /// can submit a fresh analysis.
    pub async fn active_analysis_job(
        &self,
        gallery_id: &str,
    ) -> Result<Option<AnalysisJob>, ClientError> {
        validate_analysis_id(gallery_id)?;
        let path = format!(
            "{API_PREFIX}/galleries/{}/analysis-jobs",
            encode_segment(gallery_id)
        );
        let response: ActiveAnalysisJob = self.json(&path).await?;
        if response.contract_version != 1 {
            return Err(ClientError::InvalidBody(
                "unsupported active analysis job response version".into(),
            ));
        }
        if let Some(job) = &response.active_job {
            validate_analysis_job(job, gallery_id, &job.job_id)?;
            if job.catalog_id != gallery_id {
                return Err(ClientError::InvalidBody(
                    "active analysis job catalog does not match the gallery".into(),
                ));
            }
        }
        Ok(response.active_job)
    }

    /// Read aggregate progress for a durable analysis job.
    pub async fn analysis_job(
        &self,
        catalog_id: &str,
        job_id: &str,
    ) -> Result<AnalysisJob, ClientError> {
        validate_analysis_id(catalog_id)?;
        validate_analysis_id(job_id)?;
        let path = format!(
            "{API_PREFIX}/analysis-jobs/{}/{}",
            encode_segment(catalog_id),
            encode_segment(job_id)
        );
        let job: AnalysisJob = self.json(&path).await?;
        validate_analysis_job(&job, catalog_id, job_id)?;
        if job.catalog_id != catalog_id {
            return Err(ClientError::InvalidBody(
                "analysis status catalog does not match the request".into(),
            ));
        }
        Ok(job)
    }

    pub async fn start_analysis_job(
        &self,
        catalog_id: &str,
        job_id: &str,
    ) -> Result<AnalysisJob, ClientError> {
        self.control_analysis_job(catalog_id, job_id, "start").await
    }

    pub async fn pause_analysis_job(
        &self,
        catalog_id: &str,
        job_id: &str,
    ) -> Result<AnalysisJob, ClientError> {
        self.control_analysis_job(catalog_id, job_id, "pause").await
    }

    pub async fn resume_analysis_job(
        &self,
        catalog_id: &str,
        job_id: &str,
    ) -> Result<AnalysisJob, ClientError> {
        self.control_analysis_job(catalog_id, job_id, "resume")
            .await
    }

    pub async fn cancel_analysis_job(
        &self,
        catalog_id: &str,
        job_id: &str,
    ) -> Result<AnalysisJob, ClientError> {
        self.control_analysis_job(catalog_id, job_id, "cancel")
            .await
    }

    async fn control_analysis_job(
        &self,
        catalog_id: &str,
        job_id: &str,
        action: &str,
    ) -> Result<AnalysisJob, ClientError> {
        validate_analysis_id(catalog_id)?;
        validate_analysis_id(job_id)?;
        if !matches!(action, "start" | "pause" | "resume" | "cancel") {
            return Err(ClientError::UnsafePath);
        }
        let path = format!(
            "{API_PREFIX}/analysis-jobs/{}/{}/{action}",
            encode_segment(catalog_id),
            encode_segment(job_id)
        );
        let job: AnalysisJob = self.post_json(&path, Vec::new()).await?;
        validate_analysis_job(&job, catalog_id, job_id)?;
        if job.catalog_id != catalog_id {
            return Err(ClientError::InvalidBody(
                "analysis control response catalog does not match the request".into(),
            ));
        }
        Ok(job)
    }

    pub async fn thumbnail(&self, photo: &DemoPhoto) -> Result<Vec<u8>, ClientError> {
        validate_api_path(&photo.thumbnail_url)?;
        let response = self
            .request(&photo.thumbnail_url, self.max_thumbnail_bytes)
            .await?;
        if response.body.len() > self.max_thumbnail_bytes {
            return Err(ClientError::BodyTooLarge {
                limit: self.max_thumbnail_bytes,
                actual: response.body.len(),
            });
        }
        if response.redirected {
            return Err(ClientError::Redirected);
        }
        if (300..400).contains(&response.status) {
            return Err(ClientError::Redirect {
                status: response.status,
            });
        }
        if !(200..300).contains(&response.status) {
            return Err(http_error(response));
        }
        if !mime_is(&response.content_type, "image/jpeg") {
            return Err(ClientError::InvalidMime {
                expected: "image/jpeg",
                actual: response.content_type,
            });
        }
        Ok(response.body)
    }

    async fn json<R: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<R, ClientError> {
        validate_api_path(path)?;
        let limit = self.json_limit_for_path(path);
        let response = self.request(path, limit).await?;
        if response.body.len() > limit {
            return Err(ClientError::BodyTooLarge {
                limit,
                actual: response.body.len(),
            });
        }
        if response.redirected {
            return Err(ClientError::Redirected);
        }
        if (300..400).contains(&response.status) {
            return Err(ClientError::Redirect {
                status: response.status,
            });
        }
        if !(200..300).contains(&response.status) {
            return Err(http_error(response));
        }
        if !mime_is(&response.content_type, "application/json") {
            return Err(ClientError::InvalidMime {
                expected: "application/json",
                actual: response.content_type,
            });
        }
        serde_json::from_slice(&response.body).map_err(|e| ClientError::InvalidBody(e.to_string()))
    }

    async fn post_json<R: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: Vec<u8>,
    ) -> Result<R, ClientError> {
        validate_api_path(path)?;
        if body.len() > MAX_IMPORT_REQUEST_BYTES {
            return Err(ClientError::BodyTooLarge {
                limit: MAX_IMPORT_REQUEST_BYTES,
                actual: body.len(),
            });
        }
        let response_limit = self.json_limit_for_path(path);
        let response = self
            .transport
            .post_json(path, body, response_limit)
            .await
            .map_err(|error| ClientError::Transport(error.0))?;
        if response.body.len() > response_limit {
            return Err(ClientError::BodyTooLarge {
                limit: response_limit,
                actual: response.body.len(),
            });
        }
        if response.redirected {
            return Err(ClientError::Redirected);
        }
        if (300..400).contains(&response.status) {
            return Err(ClientError::Redirect {
                status: response.status,
            });
        }
        if !(200..300).contains(&response.status) {
            return Err(http_error(response));
        }
        if !mime_is(&response.content_type, "application/json") {
            return Err(ClientError::InvalidMime {
                expected: "application/json",
                actual: response.content_type,
            });
        }
        serde_json::from_slice(&response.body)
            .map_err(|error| ClientError::InvalidBody(error.to_string()))
    }

    async fn request(
        &self,
        path: &str,
        max_response_bytes: usize,
    ) -> Result<Response, ClientError> {
        self.transport
            .get(path, max_response_bytes)
            .await
            .map_err(|e| ClientError::Transport(e.0))
    }
}

#[derive(Serialize)]
struct DecisionWriteRequest<'a> {
    decision: Decision,
    expected_revision: u64,
    request_id: &'a str,
}

fn validate_api_path(path: &str) -> Result<(), ClientError> {
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    let has_query = path.contains('?');
    let route_is_v1_catalog_page = route.starts_with("/api/v1/galleries/")
        && route.ends_with("/photos")
        && !route
            .strip_prefix('/')
            .unwrap_or(route)
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..");
    let route_is_v2_jobs = route == "/api/v2/import-jobs";
    let route_is_v2_revisions = route == "/api/v2/scan-revisions";
    let route_is_v2 = valid_v2_route(route);
    let query_is_safe = !has_query
        || ((route_is_v1_catalog_page || route_is_v2_jobs || route_is_v2_revisions)
            && !query.is_empty()
            && !query.contains(['#', '\\'])
            && valid_query_parameters(
                query,
                route_is_v1_catalog_page,
                route_is_v2_revisions,
                route_is_v2_jobs,
            ));
    if !(route.starts_with("/api/v1/") || route_is_v2)
        || route.starts_with("//")
        || route.contains(['#', '?', '\\'])
        || route.contains("://")
        || route.split('/').any(|s| s == "..")
        || !query_is_safe
    {
        Err(ClientError::UnsafePath)
    } else {
        Ok(())
    }
}

fn valid_v2_route(route: &str) -> bool {
    let Some(path) = route.strip_prefix('/') else {
        return false;
    };
    let segments = path.split('/').collect::<Vec<_>>();
    if segments.iter().any(|segment| segment.is_empty()) {
        return false;
    }
    match segments.as_slice() {
        ["api", "v2", "galleries", gallery_id, action]
            if valid_import_id(gallery_id)
                && matches!(*action, "sources" | "imports" | "rescan") =>
        {
            true
        }
        ["api", "v2", "import-estimates"]
        | ["api", "v2", "import-jobs"]
        | ["api", "v2", "scan-revisions"] => true,
        ["api", "v2", "import-jobs", job_id] => valid_import_id(job_id),
        ["api", "v2", "import-jobs", job_id, action]
            if valid_import_id(job_id) && matches!(*action, "pause" | "resume" | "cancel") =>
        {
            true
        }
        _ => false,
    }
}

fn valid_query_parameters(
    query: &str,
    is_v1_catalog_page: bool,
    is_v2_revisions: bool,
    is_v2_jobs: bool,
) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut seen = std::collections::BTreeSet::new();
    query.split('&').all(|parameter| {
        let Some((key, value)) = parameter.split_once('=') else {
            return false;
        };
        let allowed_key = if is_v1_catalog_page {
            matches!(key, "limit" | "decision" | "cursor")
        } else if is_v2_revisions {
            matches!(key, "limit" | "gallery_id")
        } else if is_v2_jobs {
            matches!(key, "limit" | "active_only")
        } else {
            key == "limit"
        };
        allowed_key
            && seen.insert(key)
            && (key != "active_only" || value == "true")
            && !value.is_empty()
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'%')
            })
            && valid_percent_escapes(value)
    })
}

fn validate_import_id(value: &str) -> Result<(), ClientError> {
    if valid_import_id(value) {
        Ok(())
    } else {
        Err(ClientError::InvalidBody("invalid opaque import ID".into()))
    }
}

fn validate_analysis_id(value: &str) -> Result<(), ClientError> {
    if valid_analysis_id(value) {
        Ok(())
    } else {
        Err(ClientError::InvalidBody("invalid analysis ID".into()))
    }
}

fn valid_analysis_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn validate_analysis_job(
    job: &AnalysisJob,
    expected_gallery_id: &str,
    expected_job_id: &str,
) -> Result<(), ClientError> {
    let expected_percent = if job.total == 0 {
        100
    } else {
        ((job.completed as u128 * 100) / job.total as u128) as u8
    };
    if job.contract_version != 1
        || !valid_analysis_id(&job.catalog_id)
        || !valid_analysis_id(&job.job_id)
        || !valid_analysis_id(&job.gallery_id)
        || !valid_analysis_id(&job.profile_id)
        || job.gallery_id != expected_gallery_id
        || job.job_id != expected_job_id
        || !matches!(
            job.state.as_str(),
            "queued" | "running" | "paused" | "succeeded" | "failed" | "cancelled"
        )
        || job.completed > job.total
        || job.failed > job.completed
        || job.percent > 100
        || job.percent != expected_percent
        || job.updated_at < job.created_at
        || (job.state == "queued" && job.completed != 0)
        || (job.state == "succeeded" && job.completed != job.total)
    {
        return Err(ClientError::InvalidBody(
            "invalid analysis job response".into(),
        ));
    }
    Ok(())
}

fn valid_import_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn validate_import_limit(limit: usize) -> Result<(), ClientError> {
    if (1..=MAX_IMPORT_PAGE_ITEMS).contains(&limit) {
        Ok(())
    } else {
        Err(ClientError::InvalidBody("invalid import page limit".into()))
    }
}

fn normalize_import_patterns(patterns: &[String]) -> Result<Vec<String>, ClientError> {
    if patterns.len() > 50 {
        return Err(ClientError::InvalidBody(
            "too many import exclusion patterns".into(),
        ));
    }
    let normalized = patterns
        .iter()
        .map(|pattern| pattern.trim().replace('\\', "/"))
        .filter(|pattern| !pattern.is_empty())
        .collect::<Vec<_>>();
    if normalized
        .iter()
        .any(|pattern| pattern.chars().count() > 256 || pattern.contains('\0'))
    {
        return Err(ClientError::InvalidBody(
            "invalid import exclusion pattern".into(),
        ));
    }
    Ok(normalized)
}

fn validate_job_page(page: &ImportJobsPage, limit: usize) -> Result<(), ClientError> {
    if page.contract_version != 2
        || page.items.len() > limit
        || page
            .items
            .iter()
            .any(|job| validate_import_job(job).is_err())
    {
        return Err(ClientError::InvalidBody(
            "invalid import jobs response".into(),
        ));
    }
    Ok(())
}

fn validate_import_job(job: &ImportJob) -> Result<(), ClientError> {
    if job.contract_version != 2
        || !valid_import_id(&job.id)
        || !valid_import_id(&job.gallery_id)
        || !valid_import_id(&job.source_id)
        || job
            .scan_revision_id
            .as_deref()
            .is_some_and(|id| !valid_import_id(id))
        || !matches!(
            job.state.as_str(),
            "queued" | "running" | "paused" | "completed" | "cancelled" | "failed"
        )
        || job.imported > job.discovered
        || job.total.is_some_and(|total| total < job.discovered)
    {
        return Err(ClientError::InvalidBody(
            "invalid import job response".into(),
        ));
    }
    Ok(())
}

fn valid_percent_escapes(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

fn mime_is(actual: &Option<String>, expected: &str) -> bool {
    actual
        .as_deref()
        .and_then(|v| v.split(';').next())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case(expected))
}

fn http_error(response: Response) -> ClientError {
    let body = serde_json::from_slice::<ApiErrorBody>(&response.body).ok();
    ClientError::Http {
        status: response.status,
        body,
    }
}

fn encode_segment(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn encode_query_value(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::RefCell,
        task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
    };
    struct Fake {
        response: RefCell<Option<Result<Response, TransportError>>>,
        paths: RefCell<Vec<String>>,
    }
    impl Fake {
        fn new(response: Result<Response, TransportError>) -> Self {
            Self {
                response: RefCell::new(Some(response)),
                paths: RefCell::new(vec![]),
            }
        }
    }
    impl Transport for Fake {
        fn get<'a>(&'a self, path: &'a str, _: usize) -> RequestFuture<'a> {
            self.paths.borrow_mut().push(path.into());
            let r = self.response.borrow_mut().take().unwrap();
            Box::pin(async move { r })
        }
    }
    fn block_on<F: Future>(future: F) -> F::Output {
        unsafe fn clone(_: *const ()) -> RawWaker {
            raw()
        }
        unsafe fn no_op(_: *const ()) {}
        fn raw() -> RawWaker {
            RawWaker::new(
                std::ptr::null(),
                &RawWakerVTable::new(clone, no_op, no_op, no_op),
            )
        }
        let waker = unsafe { Waker::from_raw(raw()) };
        let mut cx = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("unexpected pending"),
        }
    }
    fn json(body: &str) -> Response {
        Response {
            status: 200,
            content_type: Some("application/json; charset=utf-8".into()),
            body: body.as_bytes().to_vec(),
            redirected: false,
        }
    }
    fn demo_photo() -> DemoPhoto {
        DemoPhoto {
            photo_id: "demo-001".into(),
            filename: "demo-001.jpg".into(),
            decision: Decision::Undecided,
            representation: Representation::Jpeg,
            thumbnail_url: "/api/v1/photos/demo-001/thumbnail/800".into(),
        }
    }

    #[test]
    fn health_and_gallery_match_demo_contract() {
        let fake = Fake::new(Ok(json(
            r#"{"gallery_id":"demo","items":[{"photo_id":"demo-001","filename":"demo-001.jpg","decision":"undecided","representation":"jpeg","thumbnail_url":"/api/v1/photos/demo-001/thumbnail/800"}],"next_cursor":null,"total":1}"#,
        )));
        let client = DemoClient::new(fake);
        let page = block_on(client.gallery("demo")).unwrap();
        assert_eq!(page.gallery_id, "demo");
        assert_eq!(page.items[0], demo_photo());
    }

    #[test]
    fn catalog_filter_and_cursor_are_sent_as_one_server_filter() {
        let filter = CatalogFilter {
            gallery_id: "demo".into(),
            decision: Some("KEEP".into()),
        };
        let cursor = CatalogCursor {
            filter: filter.clone(),
            after_id: "p-12".into(),
        };
        let fake = Fake::new(Ok(json(
            r#"{"contract_version":1,"items":[],"next_cursor":null}"#,
        )));
        let client = DemoClient::new(fake);
        block_on(client.catalog_gallery_page(&filter, Some(&cursor), 12)).unwrap();
        let request = client.transport.paths.borrow()[0].clone();
        assert!(request.starts_with(
            "/api/v1/galleries/demo/photos?limit=12&decision=KEEP&cursor="
        ));
        assert!(request.contains("%22decision%22%3A%22KEEP%22"));
        assert!(request.contains("%22after_id%22%3A%22p-12%22"));
    }

    #[test]
    fn catalog_summary_reads_typed_whole_gallery_counts() {
        let fake = Fake::new(Ok(json(
            r#"{"total_photos":14,"total_files":17,"selected_count":3,"pending_count":5,"decisions":{"BEST":2,"KEEP":1,"REVIEW":2,"UNPROCESSED":3},"quality_tiers":{"unknown":14}}"#,
        )));
        let client = DemoClient::new(fake);
        let summary = block_on(client.catalog_summary("demo")).unwrap();
        assert_eq!(summary.total_photos, 14);
        assert_eq!(summary.decisions.get("REVIEW"), Some(&2));
        assert_eq!(client.transport.paths.borrow()[0], "/api/v1/galleries/demo/summary");
    }

    #[test]
    fn catalog_cursor_with_different_filter_is_rejected_before_request() {
        let filter = CatalogFilter {
            gallery_id: "demo".into(),
            decision: Some("KEEP".into()),
        };
        let cursor = CatalogCursor {
            filter: CatalogFilter {
                gallery_id: "demo".into(),
                decision: None,
            },
            after_id: "p-12".into(),
        };
        let client = DemoClient::new(Fake::new(Ok(json(
            r#"{"contract_version":1,"items":[],"next_cursor":null}"#,
        ))));
        assert!(matches!(
            block_on(client.catalog_gallery_page(&filter, Some(&cursor), 12)),
            Err(ClientError::InvalidBody(_))
        ));
        assert!(client.transport.paths.borrow().is_empty());
    }
    #[test]
    fn image_response_requires_mime_and_enforces_limit() {
        let fake = Fake::new(Ok(Response {
            status: 200,
            content_type: Some("image/jpeg".into()),
            body: vec![1; 5],
            redirected: false,
        }));
        let client = DemoClient::new(fake).with_limits(100, 4);
        assert!(matches!(
            block_on(client.thumbnail(&demo_photo())),
            Err(ClientError::BodyTooLarge { .. })
        ));
    }
    #[test]
    fn distinguishes_transport_status_mime_and_bad_json() {
        let t = DemoClient::new(Fake::new(Err(TransportError("offline".into()))));
        assert!(matches!(
            block_on(t.health()),
            Err(ClientError::Transport(_))
        ));
        let h = DemoClient::new(Fake::new(Ok(Response {
            status: 503,
            content_type: None,
            body: b"{}".to_vec(),
            redirected: false,
        })));
        assert!(matches!(
            block_on(h.health()),
            Err(ClientError::Http { status: 503, .. })
        ));
        let m = DemoClient::new(Fake::new(Ok(Response {
            status: 200,
            content_type: Some("text/plain".into()),
            body: b"{}".to_vec(),
            redirected: false,
        })));
        assert!(matches!(
            block_on(m.health()),
            Err(ClientError::InvalidMime { .. })
        ));
        let j = DemoClient::new(Fake::new(Ok(json("{"))));
        assert!(matches!(
            block_on(j.health()),
            Err(ClientError::InvalidBody(_))
        ));
    }
    #[test]
    fn redirects_and_unsafe_paths_are_rejected() {
        let c = DemoClient::new(Fake::new(Ok(Response {
            status: 302,
            content_type: None,
            body: vec![],
            redirected: false,
        })));
        assert!(matches!(
            block_on(c.thumbnail(&demo_photo())),
            Err(ClientError::Redirect { .. })
        ));
        assert_eq!(
            validate_api_path("https://evil.test/api/v1"),
            Err(ClientError::UnsafePath)
        );
        assert_eq!(
            validate_api_path("/api/v1/../../secret"),
            Err(ClientError::UnsafePath)
        );
        let followed = DemoClient::new(Fake::new(Ok(Response {
            status: 200,
            content_type: Some("image/jpeg".into()),
            body: vec![1],
            redirected: true,
        })));
        assert_eq!(
            block_on(followed.thumbnail(&demo_photo())),
            Err(ClientError::Redirected)
        );
    }
    #[test]
    fn checks_health_ready_state() {
        let ready = DemoClient::new(Fake::new(Ok(json(
            r#"{"status":"ready","resources":null,"request_id":"req-example"}"#,
        ))));
        assert!(block_on(ready.health()).is_ok());
    }
}
