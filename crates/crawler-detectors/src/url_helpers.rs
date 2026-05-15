//! Internal: URL helpers shared across header-based detectors.
//!
//! Pure string parsing, zero deps. Used by `x_frame_options`,
//! `coop`, and any future header detector that needs to apply
//! the localhost exemption.

/// Extract the lowercase host (no port, no IPv6 brackets) from a
/// URL string. Returns `None` on a malformed URL or one without
/// a `://` scheme separator.
///
/// BUG ASSUMPTION: a `user:pass@` prefix is supported but the
/// credentials are stripped — only the host is returned.
pub(crate) fn url_host(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://")?.1;
    let after_user = match after_scheme.split_once('@') {
        Some((_, rest)) => rest,
        None => after_scheme,
    };
    let host_with_port = after_user
        .split(|c: char| matches!(c, '/' | '?' | '#'))
        .next()
        .unwrap_or("");
    if host_with_port.is_empty() {
        return None;
    }
    let host = if let Some(rest) = host_with_port.strip_prefix('[') {
        rest.split_once(']').map(|(h, _)| h.to_string())?
    } else {
        host_with_port
            .rsplit_once(':')
            .map(|(h, _)| h.to_string())
            .unwrap_or_else(|| host_with_port.to_string())
    };
    Some(host.to_ascii_lowercase())
}

/// Detect a localhost / loopback URL: `127.0.0.1`, `::1`,
/// `localhost`, and `*.localhost`.
pub(crate) fn is_localhost(url: &str) -> bool {
    let Some(host) = url_host(url) else { return false };
    matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1") || host.ends_with(".localhost")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_host_strips_scheme_port_path() {
        assert_eq!(url_host("https://example.com:443/foo"), Some("example.com".into()));
        assert_eq!(url_host("http://acme.test/x?y=1#z"), Some("acme.test".into()));
    }

    #[test]
    fn url_host_handles_user_info() {
        assert_eq!(
            url_host("https://user:pw@example.com/x"),
            Some("example.com".into())
        );
    }

    #[test]
    fn url_host_handles_ipv6_brackets() {
        assert_eq!(url_host("https://[::1]:8080/x"), Some("::1".into()));
    }

    #[test]
    fn url_host_rejects_malformed() {
        assert!(url_host("not a url").is_none());
        // No "://" → None.
        assert!(url_host("file:foo").is_none());
    }

    #[test]
    fn is_localhost_matches_loopbacks() {
        assert!(is_localhost("https://localhost/"));
        assert!(is_localhost("http://127.0.0.1:8080/"));
        assert!(is_localhost("https://[::1]/"));
        assert!(is_localhost("https://app.localhost/"));
    }

    #[test]
    fn is_localhost_rejects_others() {
        assert!(!is_localhost("https://example.com/"));
        assert!(!is_localhost("https://localhost.evil.com/"));
        assert!(!is_localhost("malformed"));
    }
}
