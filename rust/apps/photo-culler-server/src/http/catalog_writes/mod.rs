//! Catalog write adapter. The domain service owns validation, persistence and
//! atomic revision checks. HTTP/Serde wiring awaits the shared API contract.

pub mod decision;

#[derive(Clone, Debug, PartialEq)]
pub enum Write {
    Decision {
        photo_id: String,
        state: String,
    },
    RenameSession {
        session_id: String,
        name: String,
    },
    DeleteSession {
        session_id: String,
    },
    SaveProfile {
        profile_id: String,
        document: String,
    },
    DeleteProfile {
        profile_id: String,
    },
    RestoreProfile {
        profile_id: String,
    },
    SaveRecipe {
        photo_id: String,
        document: String,
    },
    UndoRecipe {
        photo_id: String,
    },
    RedoRecipe {
        photo_id: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub request_id: String,
    pub expected_revision: u64,
    pub write: Write,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Receipt {
    pub revision: u64,
    /// Domain-owned representation, to be replaced with the frozen DTO.
    pub document: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DomainError {
    InvalidState,
    VersionConflict,
    NotFound,
    Unavailable,
}

/// Implementations must check the revision atomically with persistence. The
/// request ID is passed through for domain-level retry deduplication.
pub trait CatalogService {
    fn apply(&mut self, request: &Request) -> Result<Receipt, DomainError>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: &'static str,
    pub request_id: String,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    Receipt(Receipt),
    Error(ErrorBody),
}
#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub status: u16,
    pub body: Body,
}

pub fn handle(service: &mut impl CatalogService, request: Request) -> Response {
    match service.apply(&request) {
        Ok(receipt) => Response {
            status: 200,
            body: Body::Receipt(receipt),
        },
        Err(error) => {
            let (status, code, message) = match error {
                DomainError::InvalidState => (
                    409,
                    "invalid_state",
                    "Operation is invalid in the current state",
                ),
                DomainError::VersionConflict => {
                    (409, "version_conflict", "Revision does not match")
                }
                DomainError::NotFound => (404, "not_found", "Resource not found"),
                DomainError::Unavailable => (503, "unavailable", "Catalog unavailable"),
            };
            Response {
                status,
                body: Body::Error(ErrorBody {
                    code,
                    message,
                    request_id: request.request_id,
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    #[derive(Default)]
    struct Fake {
        calls: Vec<Write>,
        revision: u64,
        completed: HashMap<String, (Write, u64, Receipt)>,
    }
    impl CatalogService for Fake {
        fn apply(&mut self, r: &Request) -> Result<Receipt, DomainError> {
            if let Some((write, revision, receipt)) = self.completed.get(&r.request_id) {
                return if *write == r.write && *revision == r.expected_revision {
                    Ok(receipt.clone())
                } else {
                    Err(DomainError::InvalidState)
                };
            }
            if r.expected_revision != self.revision {
                return Err(DomainError::VersionConflict);
            }
            self.calls.push(r.write.clone());
            self.revision += 1;
            let receipt = Receipt {
                revision: self.revision,
                document: "saved".into(),
            };
            self.completed.insert(
                r.request_id.clone(),
                (r.write.clone(), r.expected_revision, receipt.clone()),
            );
            Ok(receipt)
        }
    }
    fn req(id: &str, revision: u64, write: Write) -> Request {
        Request {
            request_id: id.into(),
            expected_revision: revision,
            write,
        }
    }
    #[test]
    fn acceptance_calls_conflict_and_retry() {
        let mut fake = Fake::default();
        let writes = [
            Write::Decision {
                photo_id: "p".into(),
                state: "KEEP".into(),
            },
            Write::RenameSession {
                session_id: "s".into(),
                name: "Concert".into(),
            },
            Write::UndoRecipe {
                photo_id: "p".into(),
            },
        ];
        for (i, write) in writes.iter().enumerate() {
            let request = req(&format!("r{i}"), i as u64, write.clone());
            let first = handle(&mut fake, request.clone());
            assert_eq!(first.status, 200);
            assert_eq!(handle(&mut fake, request), first);
        }
        assert_eq!(fake.calls, writes);
        let stale = handle(&mut fake, req("stale", 0, writes[0].clone()));
        assert_eq!(
            stale,
            Response {
                status: 409,
                body: Body::Error(ErrorBody {
                    code: "version_conflict",
                    message: "Revision does not match",
                    request_id: "stale".into()
                })
            }
        );
        assert_eq!(fake.calls.len(), 3);
        assert_eq!(
            handle(&mut fake, req("r0", 0, writes[1].clone())).status,
            409
        );
    }
    #[test]
    fn errors_are_uniform_and_correlated() {
        struct Failing(DomainError);
        impl CatalogService for Failing {
            fn apply(&mut self, _: &Request) -> Result<Receipt, DomainError> {
                Err(self.0.clone())
            }
        }
        for (error, status, code) in [
            (DomainError::InvalidState, 409, "invalid_state"),
            (DomainError::VersionConflict, 409, "version_conflict"),
            (DomainError::NotFound, 404, "not_found"),
            (DomainError::Unavailable, 503, "unavailable"),
        ] {
            let response = handle(
                &mut Failing(error),
                req(
                    "trace",
                    0,
                    Write::UndoRecipe {
                        photo_id: "p".into(),
                    },
                ),
            );
            assert_eq!(response.status, status);
            match response.body {
                Body::Error(body) => {
                    assert_eq!(body.code, code);
                    assert_eq!(body.request_id, "trace");
                }
                _ => panic!("expected error"),
            }
        }
    }
}
