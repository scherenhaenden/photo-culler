//! Provisional read adapter. See README.md for its integration boundary.
use std::{future::Future, pin::Pin};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filter {
    pub gallery_id: String,
    pub decision: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub filter: Filter,
    pub after_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Photo {
    pub id: String,
    pub gallery_id: String,
    pub name: String,
    pub decision: String,
    pub decision_revision: u64,
    pub score: Option<f64>,
    pub quality_tier: Option<String>,
    pub analysis_summary: Value,
    pub metadata: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PhotoItem {
    pub id: String,
    pub name: String,
    pub decision: String,
    pub decision_revision: u64,
    pub score: Option<f64>,
    pub quality_tier: Option<String>,
    pub thumbnail_url: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PhotoDetail {
    pub contract_version: u8,
    pub id: String,
    pub name: String,
    pub decision: String,
    pub decision_revision: u64,
    pub score: Option<f64>,
    pub quality_tier: Option<String>,
    pub analysis_summary: Value,
    pub metadata: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Page {
    pub contract_version: u8,
    pub items: Vec<PhotoItem>,
    pub next_cursor: Option<Cursor>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Items<T> {
    pub contract_version: u8,
    pub items: Vec<T>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gallery {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub photo_count: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub photo_count: u64,
    pub recommended_photo_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub total_photos: u64,
    pub total_files: u64,
    pub selected_count: u64,
    pub pending_count: u64,
    pub decisions: std::collections::BTreeMap<String, u64>,
    pub quality_tiers: std::collections::BTreeMap<String, u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thumbnail {
    pub content_type: String,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidPage,
    CursorFilterMismatch,
    NotFound,
    Unavailable,
}

/// All methods must read from the requested gallery. Implementations must not
/// expose file system paths; `thumbnail` returns bytes from the media service.
pub type CatalogFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Catalog: Send + Sync {
    fn galleries<'a>(&'a self) -> CatalogFuture<'a, Result<Vec<Gallery>, Error>>;
    fn active_gallery<'a>(&'a self) -> CatalogFuture<'a, Result<Option<String>, Error>>;
    fn photos<'a>(
        &'a self,
        filter: &'a Filter,
        after_id: Option<&'a str>,
        limit: usize,
    ) -> CatalogFuture<'a, Result<Vec<Photo>, Error>>;
    fn photo<'a>(&'a self, id: &'a str) -> CatalogFuture<'a, Result<Option<Photo>, Error>>;
    fn sessions<'a>(
        &'a self,
        gallery_id: &'a str,
    ) -> CatalogFuture<'a, Result<Vec<Session>, Error>>;
    fn groups<'a>(&'a self, gallery_id: &'a str) -> CatalogFuture<'a, Result<Vec<Group>, Error>>;
    fn profiles<'a>(&'a self) -> CatalogFuture<'a, Result<Vec<Profile>, Error>>;
    fn summary<'a>(&'a self, gallery_id: &'a str) -> CatalogFuture<'a, Result<Summary, Error>>;
    fn thumbnail<'a>(
        &'a self,
        photo_id: &'a str,
        size: u32,
    ) -> CatalogFuture<'a, Result<Option<Thumbnail>, Error>>;
}

pub struct Reads<C>(pub C);
impl<C: Catalog> Reads<C> {
    pub async fn galleries(&self) -> Result<Items<Gallery>, Error> {
        let mut items = self.0.galleries().await?;
        items.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Items {
            contract_version: 1,
            items,
        })
    }
    pub async fn active_gallery(&self) -> Result<Option<String>, Error> {
        self.0.active_gallery().await
    }
    pub async fn page(
        &self,
        filter: Filter,
        cursor: Option<Cursor>,
        limit: usize,
    ) -> Result<Page, Error> {
        if !(1..=200).contains(&limit) {
            return Err(Error::InvalidPage);
        }
        if cursor.as_ref().is_some_and(|c| c.filter != filter) {
            return Err(Error::CursorFilterMismatch);
        }
        let after_id = cursor.as_ref().map(|cursor| cursor.after_id.as_str());
        // Ask for one extra row to determine whether the response needs a next cursor.
        let mut photos = self.0.photos(&filter, after_id, limit + 1).await?;
        // Enforce filtering at the boundary even if the repository double is careless.
        photos.retain(|p| {
            p.gallery_id == filter.gallery_id
                && filter.decision.as_deref().is_none_or(|decision| {
                    decision_category(&p.decision) == decision_category(decision)
                })
        });
        photos.sort_by(|a, b| a.id.cmp(&b.id));
        if let Some(cursor) = cursor {
            photos.retain(|p| p.id > cursor.after_id);
        }
        let has_more = photos.len() > limit;
        photos.truncate(limit);
        let next_cursor = if has_more {
            photos.last().map(|p| Cursor {
                filter,
                after_id: p.id.clone(),
            })
        } else {
            None
        };
        let items = photos
            .into_iter()
            .map(|p| PhotoItem {
                thumbnail_url: format!("/api/v1/photos/{}/thumbnail/800", path_segment(&p.id)),
                id: p.id,
                name: p.name,
                decision: p.decision,
                decision_revision: p.decision_revision,
                score: p.score,
                quality_tier: p.quality_tier,
            })
            .collect();
        Ok(Page {
            contract_version: 1,
            items,
            next_cursor,
        })
    }
    pub async fn detail(&self, id: &str) -> Result<PhotoDetail, Error> {
        let p = self.0.photo(id).await?.ok_or(Error::NotFound)?;
        Ok(PhotoDetail {
            contract_version: 1,
            id: p.id,
            name: p.name,
            decision: p.decision,
            decision_revision: p.decision_revision,
            score: p.score,
            quality_tier: p.quality_tier,
            analysis_summary: p.analysis_summary,
            metadata: p.metadata,
        })
    }
    pub async fn sessions(&self, gallery: &str) -> Result<Items<Session>, Error> {
        let mut items = self.0.sessions(gallery).await?;
        items.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Items {
            contract_version: 1,
            items,
        })
    }
    pub async fn groups(&self, gallery: &str) -> Result<Items<Group>, Error> {
        let mut items = self.0.groups(gallery).await?;
        items.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Items {
            contract_version: 1,
            items,
        })
    }
    pub async fn profiles(&self) -> Result<Items<Profile>, Error> {
        let mut items = self.0.profiles().await?;
        items.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Items {
            contract_version: 1,
            items,
        })
    }
    pub async fn summary(&self, gallery: &str) -> Result<Summary, Error> {
        self.0.summary(gallery).await
    }
    pub async fn thumbnail(&self, photo_id: &str, size: u32) -> Result<Thumbnail, Error> {
        if size == 0 || size > 4096 {
            return Err(Error::InvalidPage);
        }
        self.0
            .thumbnail(photo_id, size)
            .await?
            .ok_or(Error::NotFound)
    }
}

fn decision_category(decision: &str) -> &'static str {
    match decision.to_ascii_uppercase().as_str() {
        "KEEP" | "BEST" | "PROTECTED_BY_COVERAGE" => "keep",
        "ALT" | "ALTERNATE" => "alt",
        "REJECT" | "REJECT_TECHNICAL" | "REJECT_REDUNDANT" => "reject",
        _ => "undecided",
    }
}

fn path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(b));
        } else {
            use std::fmt::Write;
            write!(&mut encoded, "%{b:02X}").expect("writing to String cannot fail");
        }
    }
    encoded
}

#[cfg(test)]
mod tests;

pub mod routes;
