//! Transport-independent session gate. See README.md for integration requirements.

pub mod runtime;

/// Roles are deliberately explicit: ordinary editors cannot administer a NAS.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Reader,
    Editor,
    NasAdministrator,
}

/// Every protected route must choose an operation; there is no implicit default.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Read,
    Edit,
    AdministerNas,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    pub role: Role,
    /// Absolute Unix timestamp, in seconds. A session is invalid at this instant.
    pub expires_at: u64,
}

/// The implementation owns credential generation, storage and revocation.
/// Lookup must compare the complete secret and must not log it. The launcher
/// provisions an unpredictable, short-lived credential in this same store.
pub trait SessionStore {
    type Error;
    fn lookup(&self, credential: &str) -> Result<Option<Session>, Self::Error>;
}

/// Keep storage failures distinct internally while denying access in all cases.
#[derive(Debug, Eq, PartialEq)]
pub enum AuthorizationError<E> {
    MissingCredential,
    InvalidCredential,
    ExpiredCredential,
    Forbidden,
    Store(E),
}

/// Authenticate first, including for reads, then authorize the requested operation.
/// `now` is supplied by a trusted server clock, never by the HTTP client.
pub fn authorize<S: SessionStore>(
    store: &S,
    credential: Option<&str>,
    operation: Operation,
    now: u64,
) -> Result<Session, AuthorizationError<S::Error>> {
    let credential = credential
        .filter(|value| !value.is_empty())
        .ok_or(AuthorizationError::MissingCredential)?;
    let session = store
        .lookup(credential)
        .map_err(AuthorizationError::Store)?
        .ok_or(AuthorizationError::InvalidCredential)?;
    if now >= session.expires_at {
        return Err(AuthorizationError::ExpiredCredential);
    }
    let allowed = match operation {
        Operation::Read => true,
        Operation::Edit => matches!(session.role, Role::Editor | Role::NasAdministrator),
        Operation::AdministerNas => session.role == Role::NasAdministrator,
    };
    if !allowed {
        return Err(AuthorizationError::Forbidden);
    }
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct FakeStore(HashMap<&'static str, Session>);
    impl SessionStore for FakeStore {
        type Error = ();
        fn lookup(&self, credential: &str) -> Result<Option<Session>, Self::Error> {
            Ok(self.0.get(credential).cloned())
        }
    }

    fn store() -> FakeStore {
        FakeStore(HashMap::from([
            (
                "reader",
                Session {
                    role: Role::Reader,
                    expires_at: 100,
                },
            ),
            (
                "editor",
                Session {
                    role: Role::Editor,
                    expires_at: 100,
                },
            ),
            (
                "admin",
                Session {
                    role: Role::NasAdministrator,
                    expires_at: 100,
                },
            ),
            (
                "expired",
                Session {
                    role: Role::NasAdministrator,
                    expires_at: 42,
                },
            ),
        ]))
    }

    #[test]
    fn expired_launcher_credential_is_rejected_even_for_admin() {
        assert_eq!(
            authorize(&store(), Some("expired"), Operation::AdministerNas, 42),
            Err(AuthorizationError::ExpiredCredential)
        );
        assert_eq!(
            authorize(&store(), Some("expired"), Operation::Read, 43),
            Err(AuthorizationError::ExpiredCredential)
        );
    }

    #[test]
    fn reader_can_read_but_cannot_write() {
        assert!(authorize(&store(), Some("reader"), Operation::Read, 99).is_ok());
        assert_eq!(
            authorize(&store(), Some("reader"), Operation::Edit, 99),
            Err(AuthorizationError::Forbidden)
        );
        assert_eq!(
            authorize(&store(), Some("reader"), Operation::AdministerNas, 99),
            Err(AuthorizationError::Forbidden)
        );
    }

    #[test]
    fn only_administrator_can_change_nas_parameters() {
        assert!(authorize(&store(), Some("editor"), Operation::Edit, 99).is_ok());
        assert_eq!(
            authorize(&store(), Some("editor"), Operation::AdministerNas, 99),
            Err(AuthorizationError::Forbidden)
        );
        assert!(authorize(&store(), Some("admin"), Operation::AdministerNas, 99).is_ok());
    }

    #[test]
    fn absent_empty_and_unknown_credentials_are_denied() {
        assert_eq!(
            authorize(&store(), None, Operation::Read, 1),
            Err(AuthorizationError::MissingCredential)
        );
        assert_eq!(
            authorize(&store(), Some(""), Operation::Read, 1),
            Err(AuthorizationError::MissingCredential)
        );
        assert_eq!(
            authorize(&store(), Some("unknown"), Operation::Read, 1),
            Err(AuthorizationError::InvalidCredential)
        );
    }

    #[test]
    fn store_failure_fails_closed() {
        struct Failing;
        impl SessionStore for Failing {
            type Error = &'static str;
            fn lookup(&self, _: &str) -> Result<Option<Session>, Self::Error> {
                Err("unavailable")
            }
        }
        assert_eq!(
            authorize(&Failing, Some("secret"), Operation::Read, 1),
            Err(AuthorizationError::Store("unavailable"))
        );
    }
}
