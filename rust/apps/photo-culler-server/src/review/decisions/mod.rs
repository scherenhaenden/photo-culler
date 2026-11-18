//! Non-destructive, revision-checked review decisions.
//!
//! The repository port is the transaction boundary: an implementation must
//! check the scope revision and persist every update atomically. See README.md.

use std::collections::HashSet;

/// Legacy decision values retained by the Python catalog and clients.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionState {
    Keep,
    Best,
    Alternate,
    Review,
    Recover,
    RejectTechnical,
    RejectRedundant,
    ProtectedByCoverage,
    Unprocessed,
}

impl DecisionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "KEEP",
            Self::Best => "BEST",
            Self::Alternate => "ALTERNATE",
            Self::Review => "REVIEW",
            Self::Recover => "RECOVER",
            Self::RejectTechnical => "REJECT_TECHNICAL",
            Self::RejectRedundant => "REJECT_REDUNDANT",
            Self::ProtectedByCoverage => "PROTECTED_BY_COVERAGE",
            Self::Unprocessed => "UNPROCESSED",
        }
    }
}

/// Restricts a decision command to a named review session or an explicit set
/// of photo IDs. A session ID is never inferred from the selected photos.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum DecisionScope {
    Session(String),
    Photos(Vec<String>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionChange {
    pub photo_id: String,
    pub state: DecisionState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredDecision {
    pub photo_id: String,
    pub session_id: Option<String>,
    pub state: DecisionState,
}

/// The port owns persistence and must implement `apply_atomically` as one
/// compare-and-set transaction. It may update decision fields only; original
/// file paths and bytes are outside this interface.
pub trait DecisionRepository {
    type Error;

    fn load_scope(&self, scope: &DecisionScope) -> Result<Vec<StoredDecision>, Self::Error>;

    fn apply_atomically(
        &mut self,
        scope: &DecisionScope,
        expected_revision: u64,
        changes: &[DecisionChange],
    ) -> Result<u64, RepositoryError<Self::Error>>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum RepositoryError<E> {
    RevisionConflict { expected: u64, actual: u64 },
    Failure(E),
}

#[derive(Debug, Eq, PartialEq)]
pub enum DecisionError<E> {
    EmptyScope,
    EmptyPhotoId,
    DuplicatePhotoId(String),
    PhotoOutsideScope(String),
    MissingPhoto(String),
    RevisionConflict { expected: u64, actual: u64 },
    Repository(E),
}

/// Validate explicit scope and persist manual or bulk changes atomically.
pub fn apply_decisions<R: DecisionRepository>(
    repository: &mut R,
    scope: &DecisionScope,
    expected_revision: u64,
    changes: &[DecisionChange],
) -> Result<u64, DecisionError<R::Error>> {
    match scope {
        DecisionScope::Session(id) if id.trim().is_empty() => {
            return Err(DecisionError::EmptyScope)
        }
        DecisionScope::Photos(ids) if ids.is_empty() => return Err(DecisionError::EmptyScope),
        _ => {}
    }
    if changes.is_empty() {
        return Err(DecisionError::EmptyScope);
    }

    let mut requested = HashSet::with_capacity(changes.len());
    for change in changes {
        if change.photo_id.trim().is_empty() {
            return Err(DecisionError::EmptyPhotoId);
        }
        if !requested.insert(change.photo_id.as_str()) {
            return Err(DecisionError::DuplicatePhotoId(change.photo_id.clone()));
        }
        if let DecisionScope::Photos(ids) = scope {
            if !ids.iter().any(|id| id == &change.photo_id) {
                return Err(DecisionError::PhotoOutsideScope(change.photo_id.clone()));
            }
        }
    }

    let stored = repository
        .load_scope(scope)
        .map_err(DecisionError::Repository)?;
    let found: HashSet<&str> = stored.iter().map(|photo| photo.photo_id.as_str()).collect();
    for change in changes {
        if !found.contains(change.photo_id.as_str()) {
            return Err(DecisionError::MissingPhoto(change.photo_id.clone()));
        }
    }

    repository
        .apply_atomically(scope, expected_revision, changes)
        .map_err(|error| match error {
            RepositoryError::RevisionConflict { expected, actual } => {
                DecisionError::RevisionConflict { expected, actual }
            }
            RepositoryError::Failure(error) => DecisionError::Repository(error),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Clone)]
    struct FakePhoto {
        session_id: Option<String>,
        state: DecisionState,
        original_bytes: Vec<u8>,
    }

    struct FakeRepository {
        revision: u64,
        photos: BTreeMap<String, FakePhoto>,
    }

    impl FakeRepository {
        fn fixture() -> Self {
            Self {
                revision: 4,
                photos: BTreeMap::from([
                    (
                        "p1".into(),
                        FakePhoto {
                            session_id: Some("s1".into()),
                            state: DecisionState::Review,
                            original_bytes: vec![0, 1, 2],
                        },
                    ),
                    (
                        "p2".into(),
                        FakePhoto {
                            session_id: Some("s1".into()),
                            state: DecisionState::Unprocessed,
                            original_bytes: vec![3, 4],
                        },
                    ),
                    (
                        "p3".into(),
                        FakePhoto {
                            session_id: Some("s2".into()),
                            state: DecisionState::Review,
                            original_bytes: vec![5, 6],
                        },
                    ),
                ]),
            }
        }
    }

    impl DecisionRepository for FakeRepository {
        type Error = &'static str;

        fn load_scope(&self, scope: &DecisionScope) -> Result<Vec<StoredDecision>, Self::Error> {
            Ok(self
                .photos
                .iter()
                .filter(|(id, photo)| match scope {
                    DecisionScope::Session(session) => photo.session_id.as_deref() == Some(session),
                    DecisionScope::Photos(ids) => ids.iter().any(|wanted| wanted == *id),
                })
                .map(|(id, photo)| StoredDecision {
                    photo_id: id.clone(),
                    session_id: photo.session_id.clone(),
                    state: photo.state,
                })
                .collect())
        }

        fn apply_atomically(
            &mut self,
            scope: &DecisionScope,
            expected_revision: u64,
            changes: &[DecisionChange],
        ) -> Result<u64, RepositoryError<Self::Error>> {
            if self.revision != expected_revision {
                return Err(RepositoryError::RevisionConflict {
                    expected: expected_revision,
                    actual: self.revision,
                });
            }
            for change in changes {
                let photo = self
                    .photos
                    .get(&change.photo_id)
                    .ok_or(RepositoryError::Failure("missing photo"))?;
                let in_scope = match scope {
                    DecisionScope::Session(id) => photo.session_id.as_deref() == Some(id),
                    DecisionScope::Photos(ids) => ids.contains(&change.photo_id),
                };
                if !in_scope {
                    return Err(RepositoryError::Failure("photo outside scope"));
                }
            }
            for change in changes {
                self.photos.get_mut(&change.photo_id).unwrap().state = change.state;
            }
            self.revision += 1;
            Ok(self.revision)
        }
    }

    #[test]
    fn reject_correct_to_keep_and_reload_without_changing_originals() {
        let mut repo = FakeRepository::fixture();
        let scope = DecisionScope::Session("s1".into());
        let originals: BTreeMap<_, _> = repo
            .photos
            .iter()
            .map(|(id, photo)| (id.clone(), photo.original_bytes.clone()))
            .collect();

        let rev = apply_decisions(
            &mut repo,
            &scope,
            4,
            &[DecisionChange {
                photo_id: "p1".into(),
                state: DecisionState::RejectTechnical,
            }],
        )
        .unwrap();
        let rev = apply_decisions(
            &mut repo,
            &scope,
            rev,
            &[DecisionChange {
                photo_id: "p1".into(),
                state: DecisionState::Keep,
            }],
        )
        .unwrap();

        // Reload through the repository port and ensure the change stayed in s1.
        let reloaded = repo.load_scope(&scope).unwrap();
        assert_eq!(rev, 6);
        assert_eq!(reloaded.len(), 2);
        assert_eq!(
            reloaded.iter().find(|p| p.photo_id == "p1").unwrap().state,
            DecisionState::Keep
        );
        assert!(!reloaded.iter().any(|p| p.photo_id == "p3"));
        assert_eq!(
            originals,
            repo.photos
                .iter()
                .map(|(id, photo)| (id.clone(), photo.original_bytes.clone()))
                .collect()
        );
    }

    #[test]
    fn stale_revision_preserves_confirmed_decision_and_original() {
        let mut repo = FakeRepository::fixture();
        repo.photos.get_mut("p1").unwrap().state = DecisionState::RejectRedundant;
        let original = repo.photos["p1"].original_bytes.clone();
        let err = apply_decisions(
            &mut repo,
            &DecisionScope::Session("s1".into()),
            3,
            &[DecisionChange {
                photo_id: "p1".into(),
                state: DecisionState::Keep,
            }],
        )
        .unwrap_err();
        assert_eq!(
            err,
            DecisionError::RevisionConflict {
                expected: 3,
                actual: 4
            }
        );
        assert_eq!(repo.photos["p1"].state, DecisionState::RejectRedundant);
        assert_eq!(repo.photos["p1"].original_bytes, original);
    }

    #[test]
    fn bulk_updates_are_atomic_and_scope_checked() {
        let mut repo = FakeRepository::fixture();
        let err = apply_decisions(
            &mut repo,
            &DecisionScope::Session("s1".into()),
            4,
            &[
                DecisionChange {
                    photo_id: "p1".into(),
                    state: DecisionState::Keep,
                },
                DecisionChange {
                    photo_id: "p3".into(),
                    state: DecisionState::Keep,
                },
            ],
        )
        .unwrap_err();
        assert_eq!(err, DecisionError::MissingPhoto("p3".into()));
        assert_eq!(repo.photos["p1"].state, DecisionState::Review);
        assert_eq!(repo.revision, 4);
    }

    #[test]
    fn explicit_photo_scope_rejects_out_of_scope_and_duplicate_ids() {
        let mut repo = FakeRepository::fixture();
        let scope = DecisionScope::Photos(vec!["p1".into()]);
        assert_eq!(
            apply_decisions(
                &mut repo,
                &scope,
                4,
                &[DecisionChange {
                    photo_id: "p2".into(),
                    state: DecisionState::Keep
                }]
            ),
            Err(DecisionError::PhotoOutsideScope("p2".into()))
        );
        assert_eq!(
            apply_decisions(
                &mut repo,
                &scope,
                4,
                &[
                    DecisionChange {
                        photo_id: "p1".into(),
                        state: DecisionState::Keep
                    },
                    DecisionChange {
                        photo_id: "p1".into(),
                        state: DecisionState::Best
                    },
                ]
            ),
            Err(DecisionError::DuplicatePhotoId("p1".into()))
        );
    }
}
