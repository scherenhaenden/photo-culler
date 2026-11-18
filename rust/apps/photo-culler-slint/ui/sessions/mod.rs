//! Presentation model for the catalog and session panel.
//!
//! This module is self-contained until the Slint application crate is added to
//! the workspace. The UI adapter can project these stable values to Slint
//! properties and route `selected_session_id` back to the session controller.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Online,
    Offline,
    Missing,
}

impl ConnectionState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Online => "Conectada",
            Self::Offline => "Desconectada",
            Self::Missing => "Archivo ausente",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Active,
    Archived,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub name: String,
    pub state: ConnectionState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub photo_count: u64,
    pub state: SessionState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentJob {
    pub id: String,
    pub label: String,
    pub state: String,
    /// Unix milliseconds, used only for stable newest-first presentation.
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogSnapshot {
    pub total_photos: u64,
    pub sessions: Vec<Session>,
    pub sources: Vec<Source>,
    pub recent_jobs: Vec<RecentJob>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRow {
    pub id: String,
    pub name: String,
    pub photo_count: u64,
    pub state: SessionState,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobRow {
    pub id: String,
    pub label: String,
    pub state: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogSessionsPanel {
    pub total_photos: u64,
    pub session_count: usize,
    pub connected_sources: usize,
    pub disconnected_sources: usize,
    pub missing_sources: usize,
    pub empty_message: Option<&'static str>,
    pub sessions: Vec<SessionRow>,
    pub sources: Vec<Source>,
    pub recent_jobs: Vec<JobRow>,
    pub selected_session_id: Option<String>,
}

/// Creates a stable view of the catalog. A stale requested session is cleared
/// instead of silently selecting a different session.
pub fn present(
    snapshot: &CatalogSnapshot,
    selected_session_id: Option<&str>,
) -> CatalogSessionsPanel {
    let selected = selected_session_id
        .filter(|id| snapshot.sessions.iter().any(|session| session.id == *id))
        .map(str::to_owned);
    let mut sessions = snapshot.sessions.clone();
    sessions.sort_by(|a, b| {
        (a.state != SessionState::Active)
            .cmp(&(b.state != SessionState::Active))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });
    let sessions = sessions
        .into_iter()
        .map(|session| SessionRow {
            selected: selected.as_deref() == Some(session.id.as_str()),
            id: session.id,
            name: session.name,
            photo_count: session.photo_count,
            state: session.state,
        })
        .collect::<Vec<_>>();

    let mut jobs = snapshot.recent_jobs.clone();
    jobs.sort_by(|a, b| {
        b.updated_at_ms
            .cmp(&a.updated_at_ms)
            .then_with(|| a.id.cmp(&b.id))
    });
    let recent_jobs = jobs
        .into_iter()
        .take(5)
        .map(|job| JobRow {
            id: job.id,
            label: job.label,
            state: job.state,
        })
        .collect();

    let connected_sources = snapshot
        .sources
        .iter()
        .filter(|s| s.state == ConnectionState::Online)
        .count();
    let disconnected_sources = snapshot
        .sources
        .iter()
        .filter(|s| s.state == ConnectionState::Offline)
        .count();
    let missing_sources = snapshot
        .sources
        .iter()
        .filter(|s| s.state == ConnectionState::Missing)
        .count();
    let empty_message = if snapshot.sessions.is_empty() && snapshot.total_photos == 0 {
        Some("El catálogo está vacío")
    } else {
        None
    };

    CatalogSessionsPanel {
        total_photos: snapshot.total_photos,
        session_count: snapshot.sessions.len(),
        connected_sources,
        disconnected_sources,
        missing_sources,
        empty_message,
        sessions,
        sources: snapshot.sources.clone(),
        recent_jobs,
        selected_session_id: selected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> CatalogSnapshot {
        CatalogSnapshot {
            total_photos: 12,
            sessions: vec![
                Session {
                    id: "old".into(),
                    name: "Archivo".into(),
                    photo_count: 4,
                    state: SessionState::Archived,
                },
                Session {
                    id: "current".into(),
                    name: "Hoy".into(),
                    photo_count: 8,
                    state: SessionState::Active,
                },
            ],
            sources: vec![
                Source {
                    name: "NAS".into(),
                    state: ConnectionState::Offline,
                },
                Source {
                    name: "Disco externo".into(),
                    state: ConnectionState::Missing,
                },
                Source {
                    name: "Interno".into(),
                    state: ConnectionState::Online,
                },
            ],
            recent_jobs: vec![
                RecentJob {
                    id: "j1".into(),
                    label: "Importación".into(),
                    state: "completada".into(),
                    updated_at_ms: 10,
                },
                RecentJob {
                    id: "j2".into(),
                    label: "Análisis".into(),
                    state: "en curso".into(),
                    updated_at_ms: 20,
                },
            ],
        }
    }

    #[test]
    fn empty_catalog_has_clear_state_and_zero_totals() {
        let panel = present(&CatalogSnapshot::default(), None);
        assert_eq!(panel.total_photos, 0);
        assert_eq!(panel.session_count, 0);
        assert_eq!(panel.empty_message, Some("El catálogo está vacío"));
        assert!(panel.recent_jobs.is_empty());
    }

    #[test]
    fn changing_selected_session_updates_only_selection() {
        let data = snapshot();
        let first = present(&data, Some("old"));
        let second = present(&data, Some("current"));
        assert_eq!(first.selected_session_id.as_deref(), Some("old"));
        assert!(
            first
                .sessions
                .iter()
                .find(|s| s.id == "old")
                .unwrap()
                .selected
        );
        assert!(
            second
                .sessions
                .iter()
                .find(|s| s.id == "current")
                .unwrap()
                .selected
        );
        assert_eq!(first.total_photos, second.total_photos);
        assert_eq!(
            first.sessions.iter().map(|s| &s.id).collect::<Vec<_>>(),
            second.sessions.iter().map(|s| &s.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn disconnected_and_missing_sources_are_visible_and_jobs_are_recent_first() {
        let panel = present(&snapshot(), Some("current"));
        assert_eq!(
            (
                panel.connected_sources,
                panel.disconnected_sources,
                panel.missing_sources
            ),
            (1, 1, 1)
        );
        assert_eq!(
            panel
                .sources
                .iter()
                .find(|s| s.name == "NAS")
                .unwrap()
                .state
                .label(),
            "Desconectada"
        );
        assert_eq!(panel.recent_jobs[0].id, "j2");
        assert_eq!(panel.recent_jobs[1].id, "j1");
    }

    #[test]
    fn stale_session_selection_does_not_jump_to_another_session() {
        let panel = present(&snapshot(), Some("deleted"));
        assert_eq!(panel.selected_session_id, None);
        assert!(panel.sessions.iter().all(|session| !session.selected));
    }
}
