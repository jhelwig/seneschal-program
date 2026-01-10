//! FVTT asset path generation and filename utilities.

use std::path::PathBuf;

/// Sanitize a string for use as a filename
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_whitespace() => '_',
            c => c,
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

/// Generate the FVTT-relative path for an extracted image.
///
/// Returns a path like `seneschal/{title}/page_{num}.webp`.
/// The `assets/` prefix is added when returning paths to the LLM.
pub fn fvtt_image_path(
    document_title: &str,
    page_number: i32,
    description: Option<&str>,
) -> PathBuf {
    let sanitized_title = sanitize_filename(document_title);
    let sanitized_desc = description
        .map(|d| {
            format!(
                "_{}",
                sanitize_filename(&d.chars().take(30).collect::<String>())
            )
        })
        .unwrap_or_default();

    PathBuf::from(format!(
        "seneschal/{}/page_{}{}.webp",
        sanitized_title, page_number, sanitized_desc
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Hello World"), "Hello_World");
        assert_eq!(sanitize_filename("File/Name:Test"), "File_Name_Test");
        assert_eq!(sanitize_filename("  spaces  "), "spaces");
    }

    #[test]
    fn test_fvtt_image_path() {
        // Returns path relative to FVTT assets directory (no assets/ prefix)
        // The assets/ prefix is added when returning the path to the LLM
        let path = fvtt_image_path("Core Rulebook", 42, Some("starship map"));
        assert_eq!(
            path.to_string_lossy(),
            "seneschal/Core_Rulebook/page_42_starship_map.webp"
        );

        let path_no_desc = fvtt_image_path("Test Doc", 1, None);
        assert_eq!(
            path_no_desc.to_string_lossy(),
            "seneschal/Test_Doc/page_1.webp"
        );
    }
}
