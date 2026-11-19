//! WARAS web UI: static assets (HTML/CSS/JS + Lucide PNG icons + logo)
//! embedded in the binary via rust-embed.
//!
//! The API crate serves these files: `/` -> `index.html` (Login),
//! `/dashboard` -> `dashboard.html`, `/registrasi` -> `registrasi.html`,
//! `/rme` -> `rme.html`, `/farmasi` -> `farmasi.html`,
//! `/billing` -> `billing.html`, everything else under `assets/` as-is
//! (`/css/app.css`, `/js/*.js`, `/icons/*.png`, `/waras-tile-sharp.png`).
//!
//! All clinical data on the pages is loaded from the API at runtime
//! (see `assets/js/*.js`); the HTML contains only static labels.

use rust_embed::RustEmbed;

/// All files under `assets/` (relative to this crate root).
#[derive(RustEmbed)]
#[folder = "assets"]
pub struct Assets;

/// Get an embedded asset by path relative to `assets/`, e.g. `index.html`
/// or `icons/house-CBD5E1.png`. Returns `None` when the path is not embedded.
pub fn get(path: &str) -> Option<rust_embed::EmbeddedFile> {
    Assets::get(path)
}

/// MIME type for an asset path (by file extension). Unknown extensions
/// fall back to `application/octet-stream`.
pub fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// Does this path exist in the embedded assets?
pub fn exists(path: &str) -> bool {
    Assets::get(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_pages_are_embedded() {
        for page in [
            "index.html",
            "dashboard.html",
            "registrasi.html",
            "rme.html",
            "farmasi.html",
            "billing.html",
        ] {
            assert!(exists(page), "missing embedded page: {page}");
        }
    }

    #[test]
    fn shared_assets_are_embedded() {
        for asset in [
            "css/app.css",
            "js/app.js",
            "js/login.js",
            "js/dashboard.js",
            "js/registrasi.js",
            "js/rme.js",
            "js/farmasi.js",
            "js/billing.js",
            "waras-tile-sharp.png",
        ] {
            assert!(exists(asset), "missing embedded asset: {asset}");
        }
    }

    #[test]
    fn mime_types() {
        assert_eq!(mime_for("index.html"), "text/html; charset=utf-8");
        assert_eq!(mime_for("css/app.css"), "text/css; charset=utf-8");
        assert_eq!(mime_for("js/app.js"), "text/javascript; charset=utf-8");
        assert_eq!(mime_for("icons/house-CBD5E1.png"), "image/png");
        assert_eq!(mime_for("nope.bin"), "application/octet-stream");
    }
}
