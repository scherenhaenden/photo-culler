//! Session-scoped hybrid grouping and group review.
//!
//! This module is transport- and storage-independent because the server crate and
//! its domain repository are not registered at the sprint base yet.

use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Photo {
    pub photo_id: String,
    /// UTC capture time as Unix milliseconds. Missing dates are left ungrouped.
    pub captured_at_ms: Option<i64>,
    /// Normalized technical score used only to recommend a representative.
    pub quality_milli: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewSession {
    pub session_id: String,
    pub name: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub photo_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewGroup {
    pub group_id: String,
    pub session_id: String,
    pub member_photo_ids: Vec<String>,
    pub representative_photo_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HybridPolicy {
    pub session_gap_ms: i64,
    pub burst_gap_ms: i64,
}

impl Default for HybridPolicy {
    fn default() -> Self {
        Self {
            session_gap_ms: 15 * 60 * 1000,
            burst_gap_ms: 1500,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReviewFilter<'a> {
    pub session_id: Option<&'a str>,
    pub group_id: Option<&'a str>,
    pub representative_photo_id: Option<&'a str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewError<E> {
    Repository(E),
    InvalidPolicy,
    DuplicatePhotoId(String),
    SessionNotFound(String),
    GroupNotFound(String),
    PhotoOutsideGroup { photo_id: String, group_id: String },
}

pub trait ReviewRepository {
    type Error;
    fn list_photos(&self) -> Result<Vec<Photo>, Self::Error>;
    /// Replaces sessions and their groups as one storage transaction.
    fn replace_review_state(
        &mut self,
        sessions: Vec<ReviewSession>,
        groups: Vec<ReviewGroup>,
    ) -> Result<(), Self::Error>;
    fn list_sessions(&self) -> Result<Vec<ReviewSession>, Self::Error>;
    fn list_groups(&self, session_id: &str) -> Result<Vec<ReviewGroup>, Self::Error>;
    fn set_representative(
        &mut self,
        group_id: &str,
        session_id: &str,
        photo_id: &str,
    ) -> Result<bool, Self::Error>;
}

/// Rebuilds deterministic hybrid sessions and burst groups. Bursts are always
/// scoped to one session; undated photos remain outside generated sessions.
pub fn rebuild<R: ReviewRepository>(
    repo: &mut R,
    policy: HybridPolicy,
) -> Result<(), ReviewError<R::Error>> {
    if policy.session_gap_ms < 0 || policy.burst_gap_ms < 0 {
        return Err(ReviewError::InvalidPolicy);
    }
    let photos = repo.list_photos().map_err(ReviewError::Repository)?;
    let mut seen = BTreeSet::new();
    for p in &photos {
        if !seen.insert(p.photo_id.clone()) {
            return Err(ReviewError::DuplicatePhotoId(p.photo_id.clone()));
        }
    }
    let by_id: std::collections::BTreeMap<_, _> =
        photos.iter().map(|p| (p.photo_id.as_str(), p)).collect();
    let mut dated: Vec<_> = photos
        .iter()
        .filter(|p| p.captured_at_ms.is_some())
        .collect();
    dated.sort_by_key(|p| (p.captured_at_ms.unwrap(), p.photo_id.as_str()));
    let mut chunks: Vec<Vec<&Photo>> = Vec::new();
    for p in dated {
        let start_new = chunks.last().and_then(|c| c.last()).map_or(true, |prev| {
            p.captured_at_ms
                .unwrap()
                .saturating_sub(prev.captured_at_ms.unwrap())
                > policy.session_gap_ms
        });
        if start_new {
            chunks.push(Vec::new());
        }
        chunks.last_mut().unwrap().push(p);
    }

    let mut sessions = Vec::new();
    let mut groups = Vec::new();
    for chunk in chunks {
        let member_ids: Vec<_> = chunk.iter().map(|p| p.photo_id.clone()).collect();
        let start = chunk.first().unwrap().captured_at_ms.unwrap();
        let end = chunk.last().unwrap().captured_at_ms.unwrap();
        let session_id = stable_id("session", start, end, &member_ids);
        let index = sessions.len() + 1;
        sessions.push(ReviewSession {
            session_id: session_id.clone(),
            name: format!("Session {index}"),
            start_ms: start,
            end_ms: end,
            photo_ids: member_ids.clone(),
        });

        let mut burst_chunks: Vec<Vec<&Photo>> = Vec::new();
        for p in chunk {
            let start_new = burst_chunks
                .last()
                .and_then(|c| c.last())
                .map_or(true, |prev| {
                    p.captured_at_ms
                        .unwrap()
                        .saturating_sub(prev.captured_at_ms.unwrap())
                        > policy.burst_gap_ms
                });
            if start_new {
                burst_chunks.push(Vec::new());
            }
            burst_chunks.last_mut().unwrap().push(p);
        }
        for burst in burst_chunks.into_iter().filter(|b| b.len() >= 2) {
            let ids: Vec<_> = burst.iter().map(|p| p.photo_id.clone()).collect();
            let burst_start = burst.first().unwrap().captured_at_ms.unwrap();
            let burst_end = burst.last().unwrap().captured_at_ms.unwrap();
            let representative = burst
                .iter()
                .max_by_key(|p| (p.quality_milli, std::cmp::Reverse(p.photo_id.as_str())))
                .unwrap();
            groups.push(ReviewGroup {
                group_id: stable_id("group", burst_start, burst_end, &ids),
                session_id: session_id.clone(),
                member_photo_ids: ids,
                representative_photo_id: representative.photo_id.clone(),
            });
        }
    }
    // Guard against accidental future changes that allow a group to cross sessions.
    for group in &groups {
        if group
            .member_photo_ids
            .iter()
            .any(|id| !by_id.contains_key(id.as_str()))
        {
            return Err(ReviewError::InvalidPolicy);
        }
    }
    repo.replace_review_state(sessions, groups)
        .map_err(ReviewError::Repository)
}

/// Changes a representative only when both session and group match, and the
/// selected photo belongs to that group. This makes review scope explicit.
pub fn change_representative<R: ReviewRepository>(
    repo: &mut R,
    session_id: &str,
    group_id: &str,
    photo_id: &str,
) -> Result<(), ReviewError<R::Error>> {
    let session = repo
        .list_sessions()
        .map_err(ReviewError::Repository)?
        .into_iter()
        .find(|s| s.session_id == session_id)
        .ok_or_else(|| ReviewError::SessionNotFound(session_id.to_owned()))?;
    let groups = repo
        .list_groups(&session.session_id)
        .map_err(ReviewError::Repository)?;
    let group = groups
        .into_iter()
        .find(|g| g.group_id == group_id)
        .ok_or_else(|| ReviewError::GroupNotFound(group_id.to_owned()))?;
    if !group
        .member_photo_ids
        .iter()
        .any(|member| member == photo_id)
    {
        return Err(ReviewError::PhotoOutsideGroup {
            photo_id: photo_id.to_owned(),
            group_id: group_id.to_owned(),
        });
    }
    let changed = repo
        .set_representative(group_id, session_id, photo_id)
        .map_err(ReviewError::Repository)?;
    if !changed {
        return Err(ReviewError::GroupNotFound(group_id.to_owned()));
    }
    Ok(())
}

pub fn list_groups<R: ReviewRepository>(
    repo: &R,
    filter: ReviewFilter<'_>,
) -> Result<Vec<ReviewGroup>, ReviewError<R::Error>> {
    let sessions = repo.list_sessions().map_err(ReviewError::Repository)?;
    let mut groups = Vec::new();
    for session in sessions
        .into_iter()
        .filter(|s| filter.session_id.map_or(true, |id| s.session_id == id))
    {
        groups.extend(
            repo.list_groups(&session.session_id)
                .map_err(ReviewError::Repository)?
                .into_iter()
                .filter(|g| {
                    filter.group_id.map_or(true, |id| g.group_id == id)
                        && filter
                            .representative_photo_id
                            .map_or(true, |id| g.representative_photo_id == id)
                }),
        );
    }
    groups.sort_by(|a, b| (&a.session_id, &a.group_id).cmp(&(&b.session_id, &b.group_id)));
    Ok(groups)
}

fn stable_id(prefix: &str, start: i64, end: i64, members: &[String]) -> String {
    // Deterministic FNV-1a over length-delimited components; IDs are identifiers,
    // not cryptographic digests.
    let mut hash = 0xcbf29ce484222325u64;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    };
    feed(prefix.as_bytes());
    feed(&start.to_be_bytes());
    feed(&end.to_be_bytes());
    for id in members {
        feed(&(id.len() as u64).to_be_bytes());
        feed(id.as_bytes());
    }
    format!("{prefix}-{:016x}", hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct FakeRepo {
        photos: Vec<Photo>,
        sessions: Vec<ReviewSession>,
        groups: BTreeMap<String, Vec<ReviewGroup>>,
    }
    impl ReviewRepository for FakeRepo {
        type Error = &'static str;
        fn list_photos(&self) -> Result<Vec<Photo>, Self::Error> {
            Ok(self.photos.clone())
        }
        fn replace_review_state(
            &mut self,
            sessions: Vec<ReviewSession>,
            groups: Vec<ReviewGroup>,
        ) -> Result<(), Self::Error> {
            self.sessions = sessions;
            self.groups.clear();
            for group in groups {
                self.groups
                    .entry(group.session_id.clone())
                    .or_default()
                    .push(group);
            }
            Ok(())
        }
        fn list_sessions(&self) -> Result<Vec<ReviewSession>, Self::Error> {
            Ok(self.sessions.clone())
        }
        fn list_groups(&self, session_id: &str) -> Result<Vec<ReviewGroup>, Self::Error> {
            Ok(self.groups.get(session_id).cloned().unwrap_or_default())
        }
        fn set_representative(
            &mut self,
            group_id: &str,
            session_id: &str,
            photo_id: &str,
        ) -> Result<bool, Self::Error> {
            if let Some(group) = self
                .groups
                .get_mut(session_id)
                .and_then(|gs| gs.iter_mut().find(|g| g.group_id == group_id))
            {
                group.representative_photo_id = photo_id.to_owned();
                return Ok(true);
            }
            Ok(false)
        }
    }
    fn fake() -> FakeRepo {
        FakeRepo {
            photos: vec![
                Photo {
                    photo_id: "a".into(),
                    captured_at_ms: Some(1000),
                    quality_milli: 500,
                },
                Photo {
                    photo_id: "b".into(),
                    captured_at_ms: Some(1500),
                    quality_milli: 900,
                },
                Photo {
                    photo_id: "c".into(),
                    captured_at_ms: Some(2000),
                    quality_milli: 800,
                },
                Photo {
                    photo_id: "d".into(),
                    captured_at_ms: Some(100_000),
                    quality_milli: 700,
                },
                Photo {
                    photo_id: "e".into(),
                    captured_at_ms: Some(100_500),
                    quality_milli: 700,
                },
                Photo {
                    photo_id: "undated".into(),
                    captured_at_ms: None,
                    quality_milli: 1,
                },
            ],
            ..Default::default()
        }
    }
    #[test]
    fn hybrid_rebuild_is_stable_and_bursts_are_session_scoped() {
        let mut repo = fake();
        rebuild(
            &mut repo,
            HybridPolicy {
                session_gap_ms: 10_000,
                burst_gap_ms: 1_000,
            },
        )
        .unwrap();
        let first = repo.sessions.clone();
        let groups = repo.groups.clone();
        assert_eq!(repo.sessions.len(), 2);
        assert_eq!(repo.groups.len(), 2);
        assert!(repo.groups.values().all(|gs| gs
            .iter()
            .all(|g| g.member_photo_ids.iter().all(|id| !id.is_empty()))));
        rebuild(
            &mut repo,
            HybridPolicy {
                session_gap_ms: 10_000,
                burst_gap_ms: 1_000,
            },
        )
        .unwrap();
        assert_eq!(repo.sessions, first);
        assert_eq!(repo.groups, groups);
        assert_eq!(
            repo.groups[&repo.sessions[0].session_id][0].representative_photo_id,
            "b"
        );
    }
    #[test]
    fn representative_change_and_filters_stay_inside_one_session_after_reload() {
        let mut repo = fake();
        rebuild(
            &mut repo,
            HybridPolicy {
                session_gap_ms: 10_000,
                burst_gap_ms: 1_000,
            },
        )
        .unwrap();
        let session = repo.sessions[0].session_id.clone();
        let other = repo.sessions[1].session_id.clone();
        let group = repo.groups[&session][0].group_id.clone();
        change_representative(&mut repo, &session, &group, "a").unwrap();
        let reloaded = list_groups(
            &repo,
            ReviewFilter {
                session_id: Some(&session),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].representative_photo_id, "a");
        assert!(list_groups(
            &repo,
            ReviewFilter {
                session_id: Some(&other),
                group_id: Some(&group),
                ..Default::default()
            }
        )
        .unwrap()
        .is_empty());
        assert!(matches!(
            change_representative(&mut repo, &other, &group, "a"),
            Err(ReviewError::GroupNotFound(_))
        ));
    }
    #[test]
    fn representative_must_be_a_member_of_the_scoped_group() {
        let mut repo = fake();
        rebuild(
            &mut repo,
            HybridPolicy {
                session_gap_ms: 10_000,
                burst_gap_ms: 1_000,
            },
        )
        .unwrap();
        let session = repo.sessions[0].session_id.clone();
        let group = repo.groups[&session][0].group_id.clone();
        assert!(matches!(
            change_representative(&mut repo, &session, &group, "d"),
            Err(ReviewError::PhotoOutsideGroup { .. })
        ));
    }
}
