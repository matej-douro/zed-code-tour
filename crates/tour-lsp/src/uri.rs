use std::{
    path::{Path, PathBuf},
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
    let url = Url::from_file_path(path).ok()?;
    Uri::from_str(url.as_str()).ok()
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
    use std::{path::Path, str::FromStr};

    use lsp_types::Uri;

    use super::{file_path, file_uri, is_tour_document};

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
    fn rejects_non_file_uris() {
        assert_eq!(file_path(&uri("untitled:Untitled-1")), None);
    }
}
