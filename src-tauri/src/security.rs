//! Navigation and webview policy for the desktop trust boundary.

use tauri::Url;

/// The only webview that receives application command permissions.
pub const MAIN_WINDOW_LABEL: &str = "main";

/// Returns whether a navigation remains inside the bundled application document.
pub fn allows_navigation(url: &Url) -> bool {
    allows_navigation_with_development_server(url, cfg!(dev))
}

fn allows_navigation_with_development_server(url: &Url, allow_development_server: bool) -> bool {
    is_bundled_application_url(url)
        || (allow_development_server && is_development_application_url(url))
}

fn is_bundled_application_url(url: &Url) -> bool {
    is_application_document(url)
        && ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
            || (matches!(url.scheme(), "http" | "https")
                && url.host_str() == Some("tauri.localhost")
                && url.port().is_none()))
}

fn is_development_application_url(url: &Url) -> bool {
    is_application_document(url)
        && url.scheme() == "http"
        && url.host_str() == Some("localhost")
        && url.port() == Some(1420)
}

fn is_application_document(url: &Url) -> bool {
    matches!(url.path(), "" | "/" | "/index.html")
        && url.query().is_none()
        && url.fragment().is_none()
}

#[cfg(test)]
mod tests {
    use super::{MAIN_WINDOW_LABEL, allows_navigation_with_development_server};
    use tauri::Url;

    fn url(value: &str) -> Url {
        value.parse().unwrap()
    }

    #[test]
    fn permits_only_the_bundled_application_document() {
        for value in [
            "tauri://localhost/",
            "tauri://localhost/index.html",
            "http://tauri.localhost/",
            "https://tauri.localhost/index.html",
        ] {
            assert!(allows_navigation_with_development_server(
                &url(value),
                false
            ));
        }

        assert_eq!(MAIN_WINDOW_LABEL, "main");
    }

    #[test]
    fn development_server_requires_an_explicit_debug_policy() {
        let development_url = url("http://localhost:1420/");

        assert!(allows_navigation_with_development_server(
            &development_url,
            true
        ));
        assert!(!allows_navigation_with_development_server(
            &development_url,
            false
        ));
    }

    #[test]
    fn rejects_remote_content_and_unexpected_application_routes() {
        for value in [
            "https://example.com/",
            "http://localhost:1421/",
            "file:///tmp/cipher.html",
            "data:text/html,unexpected",
            "tauri://localhost/settings",
            "http://tauri.localhost/settings",
            "tauri://localhost/?next=https://example.com",
            "tauri://localhost/#settings",
        ] {
            assert!(
                !allows_navigation_with_development_server(&url(value), true),
                "{value} must not be allowed"
            );
        }
    }
}
