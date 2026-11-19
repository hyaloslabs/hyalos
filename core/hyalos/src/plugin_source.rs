//! Fetching from a plugin index (ADR-0090): `index.json` and the files beside it, from a directory
//! (an offline bundle) or over HTTPS (a release's assets). What is fetched is untrusted: it is read
//! within a size limit, an archive is used only once its SHA-256 is the digest the index gave and its
//! Sigstore bundle verifies (ADR-0093), and an error names the file asked for, never what came back.

use std::io::Read;
use std::path::Path;

use hyalos_broker::model::{HttpTransport, Upstream};
use hyalos_types::IndexLocation;

/// The most redirects followed for one file. Release assets on GitHub answer with one, to storage.
pub const MAX_REDIRECTS: usize = 5;

/// The bytes of `file`, a plain file name, beside the index at `location`, refused past `limit`.
pub fn fetch(location: &IndexLocation, file: &str, limit: u64) -> Result<Vec<u8>, String> {
    fetch_optional(location, file, limit)?.ok_or_else(|| {
        if location.is_dir() {
            format!("{file}: not in the index's directory")
        } else {
            format!("{file}: not found at the index")
        }
    })
}

/// As [`fetch`], with `None` for a file the location does not have: absent from the directory, or
/// answered 404. Any other failure is an error.
pub fn fetch_optional(
    location: &IndexLocation,
    file: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>, String> {
    if location.is_dir() {
        return read_file(&Path::new(location.as_str()).join(file), file, limit);
    }
    let mut url = format!("{}/{file}", location.as_str().trim_end_matches('/'));
    for _ in 0..=MAX_REDIRECTS {
        match get(&url, file, limit)? {
            Got::Body(bytes) => return Ok(Some(bytes)),
            Got::NotFound => return Ok(None),
            Got::Redirect(next) => url = next,
        }
    }
    Err(format!("{file}: more than {MAX_REDIRECTS} redirects"))
}

fn read_file(path: &Path, file: &str, limit: u64) -> Result<Option<Vec<u8>>, String> {
    // Not following a link: a bundle's files are its own.
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(format!("{file}: cannot be read")),
    };
    if !meta.is_file() {
        return Err(format!("{file}: not a plain file"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|_| format!("{file}: cannot be read"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("{file}: larger than {limit} bytes"));
    }
    Ok(Some(bytes))
}

enum Got {
    Body(Vec<u8>),
    NotFound,
    Redirect(String),
}

/// One GET of `url`. A redirect is followed only to a location an index could be at itself: HTTPS,
/// or HTTP on loopback.
fn get(url: &str, file: &str, limit: u64) -> Result<Got, String> {
    let (base, target) =
        split(url).ok_or_else(|| format!("{file}: not a URL that can be fetched"))?;
    let transport = HttpTransport::new(base).map_err(|e| format!("{file}: {e}"))?;
    let response = transport
        .request(
            "GET",
            &Upstream {
                target: target.to_string(),
                headers: vec![
                    ("accept".into(), "application/octet-stream".into()),
                    ("user-agent".into(), "hyalos".into()),
                ],
                body: Vec::new(),
            },
        )
        .map_err(|e| format!("{file}: could not be fetched ({e})"))?;
    match response.status {
        200 => {
            let mut bytes = Vec::new();
            response
                .body
                .take(limit + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| format!("{file}: the answer could not be read"))?;
            if bytes.len() as u64 > limit {
                return Err(format!("{file}: larger than {limit} bytes"));
            }
            Ok(Got::Body(bytes))
        }
        301 | 302 | 303 | 307 | 308 => {
            let next = response
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("location"))
                .map(|(_, value)| match value.strip_prefix('/') {
                    // A path on the same host.
                    Some(path) if !path.starts_with('/') => format!("{base}/{path}"),
                    _ => value.clone(),
                })
                .filter(|next| may_redirect_to(next))
                .ok_or_else(|| format!("{file}: redirected somewhere an index may not be"))?;
            Ok(Got::Redirect(next))
        }
        404 => Ok(Got::NotFound),
        status => Err(format!("{file}: the index answered {status}")),
    }
}

/// The longest redirect followed. Storage URLs carry a signature in their query.
const MAX_REDIRECT_LEN: usize = 8192;

/// Whether a redirect may be followed to `next`: an absolute HTTPS URL, or HTTP on loopback, with no
/// space or control character.
fn may_redirect_to(next: &str) -> bool {
    next.len() <= MAX_REDIRECT_LEN
        && !next.bytes().any(|b| b.is_ascii_control() || b == b' ')
        && (next.starts_with("https://") || hyalos_types::plugin::loopback_http(next))
}

/// `scheme://authority` and the path with its query, which is `/` if there is none.
fn split(url: &str) -> Option<(&str, String)> {
    let scheme_end = url.find("://")? + 3;
    let path_at = url[scheme_end..].find(['/', '?']).map(|i| i + scheme_end);
    Some(match path_at {
        Some(i) if url[i..].starts_with('/') => (&url[..i], url[i..].to_string()),
        Some(i) => (&url[..i], format!("/{}", &url[i..])),
        None => (url, "/".to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_splits_into_its_base_and_its_path() {
        assert_eq!(
            split("https://github.com/o/r/releases/download/v1/index.json"),
            Some((
                "https://github.com",
                "/o/r/releases/download/v1/index.json".to_string()
            ))
        );
        assert_eq!(
            split("https://h.example?x=1"),
            Some(("https://h.example", "/?x=1".to_string()))
        );
        assert_eq!(
            split("http://127.0.0.1:9/"),
            Some(("http://127.0.0.1:9", "/".to_string()))
        );
        assert_eq!(split("nothing"), None);
    }

    #[test]
    fn a_redirect_is_followed_only_to_https_or_loopback() {
        let long = format!("https://objects.example/a?sig={}", "x".repeat(2000));
        for good in [long.as_str(), "http://127.0.0.1:8/x", "https://h/"] {
            assert!(may_redirect_to(good), "{good}");
        }
        let too_long = format!("https://h/{}", "x".repeat(MAX_REDIRECT_LEN));
        for bad in [
            "/relative",
            "http://example.com/x",
            "http://localhost.evil/x",
            "http://localhost:1@evil.example/x",
            "http://127.0.0.1@evil.example/",
            "file:///etc/passwd",
            "https://h/a b",
            too_long.as_str(),
        ] {
            assert!(!may_redirect_to(bad), "{bad}");
        }
    }
}
