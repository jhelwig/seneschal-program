//! Traveller Map API integration.
//!
//! This module provides tools for querying the Traveller Map web service
//! (https://travellermap.com) to retrieve sector data, world information,
//! jump routes, and more.

mod client;
mod error;
mod options;
mod responses;
mod tool;

pub use client::TravellerMapClient;
pub use options::{JumpMapOptions, PosterOptions};
pub use responses::WorldData;
pub use tool::TravellerMapTool;

/// Sanitize a string for use in a filename
pub(crate) fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            ' ' => '-',
            _ => c,
        })
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_poster_url_generation() {
        let client = TravellerMapClient::default();
        let options = PosterOptions {
            subsector: Some("Regina".to_string()),
            style: Some("poster".to_string()),
            ..Default::default()
        };
        let url = client.poster_url("Spinward Marches", &options);
        assert!(url.contains("sector=Spinward%20Marches"));
        assert!(url.contains("subsector=Regina"));
        assert!(url.contains("style=poster"));
    }

    #[test]
    fn test_jump_map_url_generation() {
        let client = TravellerMapClient::default();
        let options = JumpMapOptions::default();
        let url = client.jump_map_url("Spinward Marches", "1910", 2, &options);
        assert!(url.contains("sector=Spinward%20Marches"));
        assert!(url.contains("hex=1910"));
        assert!(url.contains("jump=2"));
    }
}
