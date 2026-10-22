//! What the app says about itself (ADR 0014): its version and where new
//! versions are published. «Buscar actualizaciones» only opens that page in
//! the browser: Silex makes no network request of its own.

/// The version of this build, from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The page that lists the published versions.
pub const RELEASES_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/releases");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_releases_page_is_on_the_repository_over_https() {
        assert_eq!(RELEASES_URL, "https://github.com/enshetinin/silex/releases");
        assert!(!VERSION.is_empty());
    }
}
