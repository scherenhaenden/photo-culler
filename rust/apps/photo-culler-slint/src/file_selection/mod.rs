//! Platform-specific selection boundary.
//!
//! Browser selections are represented only by an authorized server root ID
//! and a server-relative path. Native filesystem paths exist only in the
//! desktop adapter and are never accepted by the browser wire type.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RootId(String);

impl RootId {
    pub fn new(value: impl Into<String>) -> Result<Self, SelectionError> {
        let value = value.into();
        if value.trim().is_empty()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(SelectionError::Rejected);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    Source,
    Destination,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizedRoot {
    pub id: RootId,
    pub label: String,
    pub online: bool,
    pub allows_source: bool,
    pub allows_destination: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelativePath(String);

impl RelativePath {
    pub fn parse(value: &str) -> Result<Self, SelectionError> {
        if value.is_empty()
            || value.starts_with('/')
            || value.starts_with('\\')
            || value.contains('\\')
            || value.contains(':')
            || value.contains('\0')
        {
            return Err(SelectionError::Rejected);
        }
        let segments: Vec<_> = value.split('/').collect();
        if segments
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
        {
            return Err(SelectionError::Rejected);
        }
        Ok(Self(segments.join("/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The only browser request shape. It intentionally has no field for a local
/// path, URI, or client-provided absolute path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserRequest {
    pub intent: Intent,
    pub root_id: RootId,
    pub relative_path: RelativePath,
}

impl BrowserRequest {
    pub fn encode(&self) -> String {
        // Minimal deterministic wire representation; all values are constrained
        // to safe identifier/path characters at construction time.
        format!(
            "intent={}&root_id={}&relative_path={}",
            match self.intent {
                Intent::Source => "source",
                Intent::Destination => "destination",
            },
            encode_component(self.root_id.as_str(), false),
            encode_component(self.relative_path.as_str(), true)
        )
    }
}

fn encode_component(value: &str, preserve_slash: bool) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~')
            || (preserve_slash && byte == b'/')
        {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectionError {
    Cancelled,
    RootOffline,
    Rejected,
    Unavailable,
}

impl fmt::Display for SelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cancelled => "selection cancelled",
            Self::RootOffline => "authorized root is offline",
            Self::Rejected => "selection is outside the authorized root",
            Self::Unavailable => "selection provider unavailable",
        })
    }
}

pub trait RootCatalog {
    fn root(&self, id: &RootId) -> Result<Option<AuthorizedRoot>, SelectionError>;
}

pub fn browser_request<C: RootCatalog>(
    catalog: &C,
    intent: Intent,
    root_id: RootId,
    relative_path: &str,
) -> Result<BrowserRequest, SelectionError> {
    let root = catalog.root(&root_id)?.ok_or(SelectionError::Rejected)?;
    if !root.online {
        return Err(SelectionError::RootOffline);
    }
    let permitted = match intent {
        Intent::Source => root.allows_source,
        Intent::Destination => root.allows_destination,
    };
    if !permitted {
        return Err(SelectionError::Rejected);
    }
    Ok(BrowserRequest {
        intent,
        root_id,
        relative_path: RelativePath::parse(relative_path)?,
    })
}

/// Desktop-only path value. Keep it in the native adapter; never convert it
/// into `BrowserRequest`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativePath(String);

pub trait NativePicker {
    fn pick(&mut self, intent: Intent) -> Result<Option<NativePath>, SelectionError>;
}

pub fn desktop_pick<P: NativePicker>(
    picker: &mut P,
    intent: Intent,
) -> Result<NativePath, SelectionError> {
    picker.pick(intent)?.ok_or(SelectionError::Cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FakeCatalog(Vec<AuthorizedRoot>);
    impl RootCatalog for FakeCatalog {
        fn root(&self, id: &RootId) -> Result<Option<AuthorizedRoot>, SelectionError> {
            Ok(self.0.iter().find(|root| &root.id == id).cloned())
        }
    }

    fn catalog() -> FakeCatalog {
        FakeCatalog(vec![AuthorizedRoot {
            id: RootId::new("nas-photos").unwrap(),
            label: "Photos".into(),
            online: true,
            allows_source: true,
            allows_destination: false,
        }])
    }

    #[test]
    fn browser_sends_only_server_root_and_relative_path() {
        let request = browser_request(
            &catalog(),
            Intent::Source,
            RootId::new("nas-photos").unwrap(),
            "2026/Wacken/photo.jpg",
        )
        .unwrap();
        let wire = request.encode();
        assert_eq!(
            wire,
            "intent=source&root_id=nas-photos&relative_path=2026/Wacken/photo.jpg"
        );
        assert!(!wire.contains("/Users/"));
        assert!(!wire.contains("C:"));
    }

    #[test]
    fn browser_wire_escapes_query_delimiters_in_relative_names() {
        let request = browser_request(
            &catalog(),
            Intent::Source,
            RootId::new("nas-photos").unwrap(),
            "trip/a&b=1.jpg",
        )
        .unwrap();
        assert_eq!(
            request.encode(),
            "intent=source&root_id=nas-photos&relative_path=trip/a%26b%3D1.jpg"
        );
    }

    #[test]
    fn browser_rejects_absolute_local_paths_and_traversal_before_wire_encoding() {
        let roots = catalog();
        for local in [
            "/Users/alice/Pictures/a.jpg",
            "C:/Users/alice/a.jpg",
            "\\\\host\\share\\a.jpg",
            "../../private/a.jpg",
        ] {
            assert_eq!(
                browser_request(
                    &roots,
                    Intent::Source,
                    RootId::new("nas-photos").unwrap(),
                    local,
                ),
                Err(SelectionError::Rejected),
                "accepted {local}"
            );
        }
    }

    #[test]
    fn browser_checks_authorization_availability_and_operation() {
        let roots = catalog();
        assert_eq!(
            browser_request(
                &roots,
                Intent::Destination,
                RootId::new("nas-photos").unwrap(),
                "out"
            ),
            Err(SelectionError::Rejected)
        );
        let mut offline = catalog().0;
        offline[0].online = false;
        assert_eq!(
            browser_request(
                &FakeCatalog(offline),
                Intent::Source,
                RootId::new("nas-photos").unwrap(),
                "folder"
            ),
            Err(SelectionError::RootOffline)
        );
        assert_eq!(
            browser_request(
                &roots,
                Intent::Source,
                RootId::new("unknown").unwrap(),
                "folder"
            ),
            Err(SelectionError::Rejected)
        );
    }

    struct FakePicker(RefCell<Option<NativePath>>);
    impl NativePicker for FakePicker {
        fn pick(&mut self, _: Intent) -> Result<Option<NativePath>, SelectionError> {
            Ok(self.0.borrow_mut().take())
        }
    }

    #[test]
    fn desktop_picker_supports_native_selection_and_cancellation() {
        let mut picked = FakePicker(RefCell::new(Some(NativePath("/Volumes/Photos".into()))));
        assert_eq!(
            desktop_pick(&mut picked, Intent::Source),
            Ok(NativePath("/Volumes/Photos".into()))
        );
        let mut cancelled = FakePicker(RefCell::new(None));
        assert_eq!(
            desktop_pick(&mut cancelled, Intent::Destination),
            Err(SelectionError::Cancelled)
        );
    }
}
