use std::{
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use lsp_types::Uri;
use url::Url;

/// Convert an LSP URI to a local file path.
pub fn file_path(uri: &Uri) -> Option<PathBuf> {
    Url::parse(uri.as_str()).ok()?.to_file_path().ok()
}

/// Convert a local file path to an LSP URI.
pub fn file_uri(path: &Path) -> Option<Uri> {
    let url = Url::from_file_path(normalize_path(path)).ok()?;
    Uri::from_str(url.as_str()).ok()
}

/// Lexically normalize a path by removing `.` segments and resolving `..`
/// against the preceding normal component.
///
/// The filesystem is never consulted, so missing files normalize the same way
/// as existing ones and targets resolve identically on every platform.
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match normalized.components().next_back() {
                Some(Component::Normal(_)) => {
                    normalized.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => normalized.push(component.as_os_str()),
            },
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }

    normalized
}

/// Whether two URIs name the same file after lexical path normalization.
pub fn same_file_uri(left: &Uri, right: &Uri) -> bool {
    if left == right {
        return true;
    }

    match (file_path(left), file_path(right)) {
        (Some(left), Some(right)) => normalize_path(&left) == normalize_path(&right),
        _ => false,
    }
}

/// Whether the URI names a `.tour.md` document.
///
/// The filter is based on the path only, so query strings and fragments do not
/// affect it. Other Markdown buffers are intentionally ignored.
pub fn is_tour_document(uri: &Uri) -> bool {
    let path = uri.as_str().split(['?', '#']).next().unwrap_or_default();
    path.ends_with(".tour.md")
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        str::FromStr,
    };

    use lsp_types::Uri;

    use super::{file_path, file_uri, is_tour_document, normalize_path, same_file_uri};

    fn uri(value: &str) -> Uri {
        Uri::from_str(value).expect("valid URI")
    }

    #[test]
    fn filters_tour_documents_by_suffix() {
        assert!(is_tour_document(&uri("file:///tours/auth.tour.md")));
        assert!(is_tour_document(&uri(
            "file:///tours/auth.tour.md?version=1"
        )));
        assert!(is_tour_document(&uri(
            "file:///tours/auth.tour.md#L1:1-L1:2"
        )));
        assert!(!is_tour_document(&uri("file:///tours/auth.md")));
        assert!(!is_tour_document(&uri("file:///tours/auth.tour.md.txt")));
        assert!(!is_tour_document(&uri("file:///tours/tour.md.bak")));
    }

    #[test]
    fn converts_between_file_paths_and_uris() {
        let path = Path::new("/tours/src/main.rs");
        let uri = file_uri(path).expect("file URI");
        assert_eq!(uri.as_str(), "file:///tours/src/main.rs");
        assert_eq!(file_path(&uri).as_deref(), Some(path));
    }

    #[test]
    fn normalizes_dot_segments_without_the_filesystem() {
        assert_eq!(
            normalize_path(Path::new("/tours/examples/../crates/./src/main.rs")),
            PathBuf::from("/tours/crates/src/main.rs")
        );
        assert_eq!(
            normalize_path(Path::new("../crates/../docs/./file.md")),
            PathBuf::from("../docs/file.md")
        );
        assert_eq!(normalize_path(Path::new("a/b/../../c")), PathBuf::from("c"));
        assert_eq!(normalize_path(Path::new("/..")), PathBuf::from("/"));
    }

    #[test]
    fn normalizes_file_uris_with_dot_segments() {
        let unnormalized = Path::new("/tours/examples/../crates/tour-lsp/src/main.rs");
        let normalized_uri = file_uri(unnormalized).expect("file URI");

        assert_eq!(
            normalized_uri.as_str(),
            "file:///tours/crates/tour-lsp/src/main.rs"
        );
        assert_eq!(
            file_path(&normalized_uri).as_deref(),
            Some(Path::new("/tours/crates/tour-lsp/src/main.rs"))
        );
        assert_eq!(
            file_path(&uri(
                "file:///tours/examples/../crates/tour-lsp/src/main.rs"
            ))
            .as_deref(),
            Some(Path::new("/tours/crates/tour-lsp/src/main.rs"))
        );
    }

    #[test]
    fn compares_file_uris_by_normalized_path() {
        let normalized = uri("file:///tours/crates/tour-lsp/src/main.rs");
        let unnormalized = uri("file:///tours/examples/../crates/tour-lsp/src/main.rs");

        assert!(same_file_uri(&normalized, &normalized));
        assert!(same_file_uri(&normalized, &unnormalized));
        assert!(same_file_uri(&unnormalized, &normalized));
        assert!(!same_file_uri(
            &normalized,
            &uri("file:///tours/src/main.rs")
        ));
        assert!(!same_file_uri(&normalized, &uri("untitled:Untitled-1")));
    }

    #[test]
    fn rejects_non_file_uris() {
        assert_eq!(file_path(&uri("untitled:Untitled-1")), None);
    }
}
