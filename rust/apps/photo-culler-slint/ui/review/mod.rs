//! Group-review presentation state with an injected, versioned REST boundary.
//!
//! This module is standalone until the future Slint app registers it. Network
//! writes are split into `begin_write` and `finish_write` so a UI callback can
//! dispatch one request asynchronously and reject duplicate clicks while it is
//! in flight.

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Decision {
    Best,
    Keep,
    Review,
    Reject,
    Unprocessed,
}

impl Decision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Best => "best",
            Self::Keep => "keep",
            Self::Review => "review",
            Self::Reject => "reject",
            Self::Unprocessed => "unprocessed",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewMember {
    pub photo_id: String,
    pub name: String,
    pub score: Option<f64>,
    pub score_reasons: Vec<String>,
    pub decision: Decision,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewSnapshot {
    pub group_id: String,
    pub revision: u64,
    pub recommended_photo_id: Option<String>,
    pub representative_photo_id: String,
    pub members: Vec<ReviewMember>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReviewFilter {
    pub decision: Option<Decision>,
    pub minimum_score: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewDraft {
    pub representative_photo_id: String,
    pub decisions: BTreeMap<String, Decision>,
}

impl ReviewDraft {
    fn from_snapshot(snapshot: &ReviewSnapshot) -> Self {
        Self {
            representative_photo_id: snapshot.representative_photo_id.clone(),
            decisions: snapshot
                .members
                .iter()
                .map(|member| (member.photo_id.clone(), member.decision))
                .collect(),
        }
    }

    pub fn decision_for(&self, member: &ReviewMember) -> Decision {
        self.decisions
            .get(&member.photo_id)
            .copied()
            .unwrap_or(member.decision)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewWrite {
    pub request_id: u64,
    pub group_id: String,
    pub expected_revision: u64,
    pub representative_photo_id: String,
    pub decisions: BTreeMap<String, Decision>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewApiError {
    Unavailable(String),
    NotFound,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewWriteError {
    Conflict { current_revision: Option<u64> },
    Unavailable(String),
    Rejected(String),
}

/// The future REST client implements these snapshot and optimistic-write calls.
/// A stale expected revision must return `Conflict` without applying the draft.
pub trait ReviewApi {
    fn load_group(&self, group_id: &str) -> Result<ReviewSnapshot, ReviewApiError>;
    fn write_group(&self, write: &ReviewWrite) -> Result<ReviewSnapshot, ReviewWriteError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewError {
    NoGroupLoaded,
    UnknownPhoto(String),
    NoPendingWrite,
    WriteAlreadyPending,
    Api(ReviewApiError),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewRow {
    pub photo_id: String,
    pub name: String,
    pub score: Option<f64>,
    pub score_reasons: Vec<String>,
    pub decision: Decision,
    pub is_representative: bool,
    pub is_recommended: bool,
}

/// View model used to bind the Slint component to the REST API.
pub struct ReviewController {
    confirmed: Option<ReviewSnapshot>,
    draft: Option<ReviewDraft>,
    filter: ReviewFilter,
    pending_write: Option<u64>,
    next_request_id: u64,
    pub status: String,
    pub has_conflict: bool,
}

impl Default for ReviewController {
    fn default() -> Self {
        Self {
            confirmed: None,
            draft: None,
            filter: ReviewFilter::default(),
            pending_write: None,
            next_request_id: 1,
            status: "Load a group to begin review.".into(),
            has_conflict: false,
        }
    }
}

impl ReviewController {
    pub fn load<A: ReviewApi>(&mut self, api: &A, group_id: &str) -> Result<(), ReviewError> {
        let snapshot = api.load_group(group_id).map_err(ReviewError::Api)?;
        self.replace_confirmed(snapshot, false);
        self.status = "Group loaded.".into();
        Ok(())
    }

    /// Reload only confirmed server state. An uncommitted proposal remains
    /// available for review and can be submitted against the refreshed revision.
    pub fn reload<A: ReviewApi>(&mut self, api: &A) -> Result<(), ReviewError> {
        if self.pending_write.is_some() {
            return Err(ReviewError::WriteAlreadyPending);
        }
        let group_id = self
            .confirmed
            .as_ref()
            .ok_or(ReviewError::NoGroupLoaded)?
            .group_id
            .clone();
        let snapshot = api.load_group(&group_id).map_err(ReviewError::Api)?;
        self.replace_confirmed(snapshot, true);
        self.has_conflict = false;
        self.status = "Confirmed server state reloaded; local proposal retained.".into();
        Ok(())
    }

    fn replace_confirmed(&mut self, snapshot: ReviewSnapshot, keep_draft: bool) {
        if !keep_draft || self.draft.is_none() {
            self.draft = Some(ReviewDraft::from_snapshot(&snapshot));
        }
        self.confirmed = Some(snapshot);
    }

    pub fn confirmed(&self) -> Option<&ReviewSnapshot> {
        self.confirmed.as_ref()
    }

    pub fn draft(&self) -> Option<&ReviewDraft> {
        self.draft.as_ref()
    }

    pub fn set_filter(&mut self, filter: ReviewFilter) {
        self.filter = filter;
    }

    pub fn set_representative(&mut self, photo_id: &str) -> Result<(), ReviewError> {
        self.ensure_editable_member(photo_id)?;
        self.draft
            .as_mut()
            .ok_or(ReviewError::NoGroupLoaded)?
            .representative_photo_id = photo_id.to_owned();
        self.status = format!("{} selected as representative.", photo_id);
        Ok(())
    }

    pub fn set_decision(&mut self, photo_id: &str, decision: Decision) -> Result<(), ReviewError> {
        self.ensure_editable_member(photo_id)?;
        self.draft
            .as_mut()
            .ok_or(ReviewError::NoGroupLoaded)?
            .decisions
            .insert(photo_id.to_owned(), decision);
        self.status = format!("Proposed decision: {}.", decision.as_str());
        Ok(())
    }

    fn ensure_editable_member(&self, photo_id: &str) -> Result<(), ReviewError> {
        if self.pending_write.is_some() {
            return Err(ReviewError::WriteAlreadyPending);
        }
        let snapshot = self.confirmed.as_ref().ok_or(ReviewError::NoGroupLoaded)?;
        if snapshot
            .members
            .iter()
            .any(|member| member.photo_id == photo_id)
        {
            Ok(())
        } else {
            Err(ReviewError::UnknownPhoto(photo_id.to_owned()))
        }
    }

    pub fn visible_rows(&self) -> Vec<ReviewRow> {
        let (Some(snapshot), Some(draft)) = (&self.confirmed, &self.draft) else {
            return Vec::new();
        };
        snapshot
            .members
            .iter()
            .filter_map(|member| {
                let decision = draft.decision_for(member);
                if self
                    .filter
                    .decision
                    .is_some_and(|filter| filter != decision)
                    || self
                        .filter
                        .minimum_score
                        .is_some_and(|minimum| member.score.is_none_or(|score| score < minimum))
                {
                    return None;
                }
                Some(ReviewRow {
                    photo_id: member.photo_id.clone(),
                    name: member.name.clone(),
                    score: member.score,
                    score_reasons: member.score_reasons.clone(),
                    decision,
                    is_representative: draft.representative_photo_id == member.photo_id,
                    is_recommended: snapshot.recommended_photo_id.as_deref()
                        == Some(member.photo_id.as_str()),
                })
            })
            .collect()
    }

    /// Start one optimistic REST write. A caller dispatches this request once,
    /// then passes the response to `finish_write`; additional clicks are blocked.
    pub fn begin_write(&mut self) -> Result<ReviewWrite, ReviewError> {
        if self.pending_write.is_some() {
            return Err(ReviewError::WriteAlreadyPending);
        }
        let snapshot = self.confirmed.as_ref().ok_or(ReviewError::NoGroupLoaded)?;
        let draft = self.draft.as_ref().ok_or(ReviewError::NoGroupLoaded)?;
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        let write = ReviewWrite {
            request_id,
            group_id: snapshot.group_id.clone(),
            expected_revision: snapshot.revision,
            representative_photo_id: draft.representative_photo_id.clone(),
            decisions: draft.decisions.clone(),
        };
        self.pending_write = Some(request_id);
        self.status = "Saving review…".into();
        Ok(write)
    }

    pub fn finish_write(
        &mut self,
        request_id: u64,
        result: Result<ReviewSnapshot, ReviewWriteError>,
    ) -> Result<(), ReviewError> {
        if self.pending_write != Some(request_id) {
            return Err(ReviewError::NoPendingWrite);
        }
        self.pending_write = None;
        match result {
            Ok(snapshot) => {
                self.replace_confirmed(snapshot, false);
                self.has_conflict = false;
                self.status = "Review saved.".into();
            }
            Err(ReviewWriteError::Conflict { current_revision }) => {
                self.has_conflict = true;
                self.status = match current_revision {
                    Some(revision) => format!(
                        "Review changed on the server (revision {revision}); reload to compare."
                    ),
                    None => "Review changed on the server; reload to compare.".into(),
                };
                // Keep `draft` intact until the user reloads or explicitly retries.
            }
            Err(ReviewWriteError::Unavailable(message)) => {
                self.status = format!("Could not save review: {message}");
            }
            Err(ReviewWriteError::Rejected(message)) => {
                self.status = format!("Review was rejected: {message}");
            }
        }
        Ok(())
    }

    pub fn write_pending(&self) -> bool {
        self.pending_write.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const SLINT_SOURCE: &str = include_str!("review.slint");

    #[derive(Default)]
    struct FakeReviewApi {
        snapshot: Mutex<Option<ReviewSnapshot>>,
        writes: Mutex<Vec<ReviewWrite>>,
    }

    impl FakeReviewApi {
        fn with_group(snapshot: ReviewSnapshot) -> Self {
            Self {
                snapshot: Mutex::new(Some(snapshot)),
                writes: Mutex::new(Vec::new()),
            }
        }

        fn change_server_snapshot(&self, mutate: impl FnOnce(&mut ReviewSnapshot)) {
            mutate(self.snapshot.lock().unwrap().as_mut().unwrap());
        }

        fn write_count(&self) -> usize {
            self.writes.lock().unwrap().len()
        }
    }

    impl ReviewApi for FakeReviewApi {
        fn load_group(&self, group_id: &str) -> Result<ReviewSnapshot, ReviewApiError> {
            self.snapshot
                .lock()
                .unwrap()
                .as_ref()
                .filter(|snapshot| snapshot.group_id == group_id)
                .cloned()
                .ok_or(ReviewApiError::NotFound)
        }

        fn write_group(&self, write: &ReviewWrite) -> Result<ReviewSnapshot, ReviewWriteError> {
            self.writes.lock().unwrap().push(write.clone());
            let mut snapshot = self.snapshot.lock().unwrap();
            let current = snapshot.as_mut().expect("test group initialized");
            if current.revision != write.expected_revision {
                return Err(ReviewWriteError::Conflict {
                    current_revision: Some(current.revision),
                });
            }
            current.representative_photo_id = write.representative_photo_id.clone();
            for member in &mut current.members {
                if let Some(decision) = write.decisions.get(&member.photo_id) {
                    member.decision = *decision;
                }
            }
            current.revision += 1;
            Ok(current.clone())
        }
    }

    fn group() -> ReviewSnapshot {
        ReviewSnapshot {
            group_id: "burst-1".into(),
            revision: 8,
            recommended_photo_id: Some("photo-a".into()),
            representative_photo_id: "photo-a".into(),
            members: vec![
                ReviewMember {
                    photo_id: "photo-a".into(),
                    name: "frame-a.jpg".into(),
                    score: Some(0.92),
                    score_reasons: vec!["sharpness: high".into(), "exposure: balanced".into()],
                    decision: Decision::Keep,
                },
                ReviewMember {
                    photo_id: "photo-b".into(),
                    name: "frame-b.jpg".into(),
                    score: Some(0.92),
                    score_reasons: vec!["sharpness: high".into(), "focus: tie with frame-a".into()],
                    decision: Decision::Reject,
                },
                ReviewMember {
                    photo_id: "photo-c".into(),
                    name: "frame-c.jpg".into(),
                    score: Some(0.71),
                    score_reasons: vec!["motion: slight".into()],
                    decision: Decision::Review,
                },
            ],
        }
    }

    fn loaded() -> (ReviewController, FakeReviewApi) {
        let api = FakeReviewApi::with_group(group());
        let mut controller = ReviewController::default();
        controller.load(&api, "burst-1").unwrap();
        (controller, api)
    }

    #[test]
    fn shows_tied_scores_reasons_and_filters_group_members() {
        let (mut controller, _) = loaded();
        let rows = controller.visible_rows();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].score, rows[1].score);
        assert!(rows[1]
            .score_reasons
            .iter()
            .any(|reason| reason.contains("tie")));
        assert!(rows[0].is_representative);
        assert!(rows[0].is_recommended);

        controller.set_filter(ReviewFilter {
            decision: Some(Decision::Reject),
            minimum_score: Some(0.9),
        });
        let filtered = controller.visible_rows();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].photo_id, "photo-b");
    }

    #[test]
    fn changing_representative_is_a_local_proposal_until_saved() {
        let (mut controller, api) = loaded();
        controller.set_representative("photo-b").unwrap();
        assert_eq!(
            controller.confirmed().unwrap().representative_photo_id,
            "photo-a"
        );
        assert!(controller.visible_rows()[1].is_representative);

        let write = controller.begin_write().unwrap();
        let response = api.write_group(&write);
        controller.finish_write(write.request_id, response).unwrap();
        assert_eq!(api.write_count(), 1);
        assert_eq!(
            controller.confirmed().unwrap().representative_photo_id,
            "photo-b"
        );
        assert_eq!(controller.confirmed().unwrap().revision, 9);
    }

    #[test]
    fn correcting_a_rejected_decision_persists_the_replacement() {
        let (mut controller, api) = loaded();
        controller.set_decision("photo-b", Decision::Keep).unwrap();
        let write = controller.begin_write().unwrap();
        let response = api.write_group(&write);
        controller.finish_write(write.request_id, response).unwrap();
        assert_eq!(
            controller.confirmed().unwrap().members[1].decision,
            Decision::Keep
        );
    }

    #[test]
    fn stale_revision_conflict_keeps_the_local_proposal() {
        let (mut controller, api) = loaded();
        controller.set_decision("photo-b", Decision::Keep).unwrap();
        controller.set_representative("photo-c").unwrap();
        api.change_server_snapshot(|snapshot| snapshot.revision += 1);

        let write = controller.begin_write().unwrap();
        assert_eq!(write.expected_revision, 8);
        let response = api.write_group(&write);
        controller.finish_write(write.request_id, response).unwrap();

        assert!(controller.has_conflict);
        assert_eq!(
            controller.draft().unwrap().decisions["photo-b"],
            Decision::Keep
        );
        assert_eq!(
            controller.draft().unwrap().representative_photo_id,
            "photo-c"
        );
        assert_eq!(controller.confirmed().unwrap().revision, 8);
    }

    #[test]
    fn reload_replaces_confirmed_state_but_retains_proposal_for_reconciliation() {
        let (mut controller, api) = loaded();
        controller.set_decision("photo-b", Decision::Keep).unwrap();
        let write = controller.begin_write().unwrap();
        let conflict = api.write_group(&ReviewWrite {
            expected_revision: 7,
            ..write.clone()
        });
        controller.finish_write(write.request_id, conflict).unwrap();
        api.change_server_snapshot(|snapshot| {
            snapshot.revision = 9;
            snapshot.members[2].decision = Decision::Best;
        });

        controller.reload(&api).unwrap();
        assert_eq!(controller.confirmed().unwrap().revision, 9);
        assert_eq!(
            controller.confirmed().unwrap().members[2].decision,
            Decision::Best
        );
        assert_eq!(
            controller.draft().unwrap().decisions["photo-b"],
            Decision::Keep
        );
        // The proposal for photo-c is still the confirmed state at the time it
        // was loaded, not the remote Best update; the reviewer can reconcile it.
        assert_eq!(
            controller.draft().unwrap().decisions["photo-c"],
            Decision::Review
        );
        assert!(!controller.has_conflict);

        let retry = controller.begin_write().unwrap();
        assert_eq!(retry.expected_revision, 9);
    }

    #[test]
    fn pending_write_suppresses_double_submit_and_edit_actions() {
        let (mut controller, api) = loaded();
        controller.set_decision("photo-b", Decision::Keep).unwrap();
        let write = controller.begin_write().unwrap();
        assert_eq!(
            controller.begin_write(),
            Err(ReviewError::WriteAlreadyPending)
        );
        assert_eq!(
            controller.set_representative("photo-c"),
            Err(ReviewError::WriteAlreadyPending)
        );
        assert_eq!(
            controller.reload(&api),
            Err(ReviewError::WriteAlreadyPending)
        );
        assert_eq!(api.write_count(), 0);

        let response = api.write_group(&write);
        controller.finish_write(write.request_id, response).unwrap();
        assert_eq!(api.write_count(), 1);
    }

    #[test]
    fn panel_uses_shared_keyboard_actions_and_keeps_callbacks() {
        assert!(SLINT_SOURCE.contains("import { ActionButton, DesignTokens }"));
        assert!(SLINT_SOURCE.matches("ActionButton {").count() >= 5);
        for callback in [
            "root.filter_changed(filter)",
            "root.representative_selected(item.photo_id)",
            "root.decision_selected(item.photo_id, decision)",
            "root.save_review()",
            "root.reload_confirmed()",
        ] {
            assert!(SLINT_SOURCE.contains(callback), "missing {callback}");
        }
    }
}
