//! Well-known filesystem locations and path conventions.

use std::path::PathBuf;

pub(crate) fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
}

/// Returns the part of `path` that follows a home-directory prefix.
fn tilde_relative_part(path: &str) -> Option<&str> {
    let rest = path.strip_prefix('~')?;
    if let Some(relative) = rest.strip_prefix('/') {
        return Some(relative);
    }

    // Backslash is a legal file name character outside Windows.
    #[cfg(windows)]
    if let Some(relative) = rest.strip_prefix('\\') {
        return Some(relative);
    }

    None
}

/// Expands a leading `~/` against the user's home directory. Returns `None`
/// only when the home directory is needed but cannot be determined.
pub(crate) fn expand_tilde(path: &str) -> Option<PathBuf> {
    match tilde_relative_part(path) {
        Some(relative) => Some(home_dir()?.join(relative)),
        None => Some(PathBuf::from(path)),
    }
}

/// Converts a `file://` URL into a native path.
pub(crate) fn path_from_file_url(url: &str) -> Option<PathBuf> {
    if !is_file_url(url) {
        return None;
    }
    let encoded = url[7..].split(['?', '#']).next()?;
    let encoded_path = if encoded.starts_with('/') {
        encoded.to_string()
    } else {
        let (host, path) = encoded.split_once('/')?;
        if host.is_empty() || path.is_empty() {
            return None;
        }

        if host.eq_ignore_ascii_case("localhost") {
            format!("/{path}")
        } else {
            let host = percent_encoding::percent_decode_str(host)
                .decode_utf8()
                .ok()?;
            let path = percent_encoding::percent_decode_str(path)
                .decode_utf8()
                .ok()?;
            #[cfg(windows)]
            return Some(PathBuf::from(format!(
                r"\\{}\{}",
                host,
                path.replace('/', r"\")
            )));
            #[cfg(not(windows))]
            return Some(PathBuf::from(format!("//{host}/{path}")));
        }
    };

    let decoded = percent_encoding::percent_decode_str(&encoded_path)
        .decode_utf8()
        .ok()?;
    let decoded = decoded.as_ref();

    // `file:///C:/x` carries a leading slash that is not part of the path.
    #[cfg(windows)]
    let decoded = decoded.trim_start_matches('/');

    (!decoded.is_empty()).then(|| PathBuf::from(decoded))
}

pub(crate) fn is_file_url(value: &str) -> bool {
    value
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file://"))
}

/// Directories searched for bundled assets, in priority order.
pub(crate) fn asset_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(executable_path) = std::env::current_exe() {
        if let Some(executable_dir) = executable_path.parent() {
            roots.push(executable_dir.to_path_buf());

            // `Moeka.app/Contents/MacOS/moeka` sits next to `Contents/Resources`.
            #[cfg(target_os = "macos")]
            if let Some(contents_dir) = executable_dir.parent() {
                roots.push(contents_dir.join("Resources"));
            }

            #[cfg(windows)]
            roots.push(executable_dir.join("resources"));
        }
    }

    #[cfg(target_os = "linux")]
    roots.push(PathBuf::from("/usr/share/moeka"));

    if let Ok(current_dir) = std::env::current_dir() {
        roots.push(current_dir.clone());
        roots.push(current_dir.join("editor"));
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    roots.push(manifest_dir.clone());
    if let Some(parent) = manifest_dir.parent() {
        roots.push(parent.to_path_buf());
    }

    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    const URL_DIR: &str = "/tmp/";
    #[cfg(not(windows))]
    const PATH_DIR: &str = "/tmp/";

    // A Windows file URL carries the drive letter after the third slash.
    #[cfg(windows)]
    const URL_DIR: &str = "/C:/tmp/";
    #[cfg(windows)]
    const PATH_DIR: &str = "C:/tmp/";

    fn file_url(name: &str) -> String {
        format!("file://{URL_DIR}{name}")
    }

    fn native_path(name: &str) -> PathBuf {
        PathBuf::from(format!("{PATH_DIR}{name}"))
    }

    #[test]
    fn leaves_paths_without_a_home_prefix_untouched() {
        assert_eq!(
            expand_tilde("/absolute/note.md"),
            Some(PathBuf::from("/absolute/note.md"))
        );
        assert_eq!(
            expand_tilde("relative/note.md"),
            Some(PathBuf::from("relative/note.md"))
        );
        // `~user` names another account, not the current home.
        assert_eq!(
            expand_tilde("~other/note.md"),
            Some(PathBuf::from("~other/note.md"))
        );
    }

    #[test]
    fn expands_home_prefix() {
        let home = home_dir().expect("home directory");
        assert_eq!(expand_tilde("~/note.md"), Some(home.join("note.md")));
    }

    #[test]
    fn decodes_percent_escaped_file_urls() {
        assert_eq!(
            path_from_file_url(&file_url("my%20note.md")),
            Some(native_path("my note.md"))
        );
        // Multi-byte characters must survive as one character, not one per byte.
        assert_eq!(
            path_from_file_url(&file_url("%E7%AC%94%E8%AE%B0.md")),
            Some(native_path("笔记.md"))
        );
        assert_eq!(
            path_from_file_url(&format!("file://localhost{URL_DIR}note.md")),
            Some(native_path("note.md"))
        );
        assert_eq!(
            path_from_file_url(&file_url("note.md?download=1#section")),
            Some(native_path("note.md"))
        );
        assert_eq!(
            path_from_file_url(&file_url("literal%23name.md")),
            Some(native_path("literal#name.md"))
        );
        assert_eq!(
            path_from_file_url(&file_url("note.md").replacen("file", "FILE", 1)),
            Some(native_path("note.md"))
        );
    }

    #[test]
    fn rejects_urls_that_are_not_absolute_file_paths() {
        assert_eq!(path_from_file_url("https://moeka.app/"), None);
        assert_eq!(path_from_file_url("file://localhostfoo/note.md"), {
            #[cfg(windows)]
            {
                Some(PathBuf::from(r"\\localhostfoo\note.md"))
            }
            #[cfg(not(windows))]
            {
                Some(PathBuf::from("//localhostfoo/note.md"))
            }
        });
        assert_eq!(path_from_file_url("/tmp/note.md"), None);
        assert_eq!(path_from_file_url("file://"), None);
    }

    #[test]
    fn converts_file_url_authorities_to_network_paths() {
        #[cfg(windows)]
        assert_eq!(
            path_from_file_url("file://server/share/%E7%AC%94%E8%AE%B0.md"),
            Some(PathBuf::from(r"\\server\share\笔记.md"))
        );
        #[cfg(not(windows))]
        assert_eq!(
            path_from_file_url("file://server/share/%E7%AC%94%E8%AE%B0.md"),
            Some(PathBuf::from("//server/share/笔记.md"))
        );
    }
}
