//! Auto-import directory watcher and processor.
//!
//! Recursively watches a configured directory for new document files and
//! automatically imports them into the system. Successfully imported files
//! are deleted (since they're now stored in the documents directory). Failed
//! imports are moved to a `failed/` subdirectory.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::error::{ProcessingError, ServiceError, ServiceResult};
use crate::ingestion::hash::compute_file_hash;
use crate::service::SeneschalService;
use crate::tools::AccessLevel;

/// Supported file extensions for auto-import
const SUPPORTED_EXTENSIONS: &[&str] = &["pdf", "epub", "md", "markdown", "txt", "text"];

/// Directory to skip when scanning (case-insensitive)
const FAILED_DIRECTORY: &str = "failed";

/// Interval between directory scans (in seconds)
const POLL_INTERVAL_SECS: u64 = 10;

/// Start the auto-import worker.
///
/// This should be called once on server startup if `auto_import_dir` is configured.
/// The worker polls the directory recursively for new files and processes them one at a time.
pub fn start_auto_import_worker(service: Arc<SeneschalService>, auto_import_dir: PathBuf) {
    tokio::spawn(async move {
        info!(path = %auto_import_dir.display(), "Auto-import worker started");

        // Ensure failed directory exists
        if let Err(e) = std::fs::create_dir_all(auto_import_dir.join(FAILED_DIRECTORY)) {
            error!(error = %e, "Failed to create auto-import failed directory, worker stopping");
            return;
        }

        loop {
            match scan_and_process_one(&service, &auto_import_dir).await {
                Ok(Some(filename)) => {
                    info!(file = %filename, "Auto-import processed file");
                    // Continue immediately to check for more files
                    continue;
                }
                Ok(None) => {
                    // No files to process, wait before next scan
                    tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
                }
                Err(e) => {
                    error!(error = %e, "Auto-import scan error");
                    tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
                }
            }
        }
    });
}

/// Recursively collect all supported files from a directory, skipping the failed/ directory.
fn collect_files_recursive(dir: &Path, base_dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let entries = std::fs::read_dir(dir)?;

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();

        if path.is_dir() {
            // Skip the failed directory at the root level
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && path.parent() == Some(base_dir)
                && name.eq_ignore_ascii_case(FAILED_DIRECTORY)
            {
                continue;
            }
            // Recurse into subdirectory
            collect_files_recursive(&path, base_dir, files)?;
        } else if path.is_file() && is_supported_format(&path) {
            files.push(path);
        }
    }

    Ok(())
}

/// Scan the directory recursively and process one file (sorted by path for determinism).
async fn scan_and_process_one(
    service: &SeneschalService,
    auto_import_dir: &Path,
) -> ServiceResult<Option<String>> {
    let mut files = Vec::new();
    collect_files_recursive(auto_import_dir, auto_import_dir, &mut files)
        .map_err(|e| ServiceError::Processing(ProcessingError::Io(e)))?;

    if files.is_empty() {
        return Ok(None);
    }

    // Sort by path for deterministic ordering
    files.sort();

    // Process the first file
    let file_path = &files[0];
    let display_path = file_path
        .strip_prefix(auto_import_dir)
        .unwrap_or(file_path)
        .display()
        .to_string();

    debug!(file = %display_path, "Processing auto-import file");

    match process_file(service, file_path).await {
        Ok(ProcessResult::Imported) => {
            // Delete the original file - it's now stored in the documents directory
            if let Err(e) = std::fs::remove_file(file_path) {
                warn!(file = %display_path, error = %e, "Failed to delete imported file");
            }
            // Clean up empty parent directories (but not the auto-import root)
            cleanup_empty_dirs(file_path.parent(), auto_import_dir);
            Ok(Some(display_path))
        }
        Ok(ProcessResult::Duplicate { existing_id }) => {
            info!(
                file = %display_path,
                existing_doc_id = %existing_id,
                "Skipped duplicate file (deleted)"
            );
            // Delete the duplicate - it's already imported
            if let Err(e) = std::fs::remove_file(file_path) {
                warn!(file = %display_path, error = %e, "Failed to delete duplicate file");
            }
            cleanup_empty_dirs(file_path.parent(), auto_import_dir);
            Ok(Some(display_path))
        }
        Err(e) => {
            error!(file = %display_path, error = %e, "Auto-import failed");
            // Move to failed/ directory, preserving relative path structure
            move_to_failed(file_path, auto_import_dir);
            cleanup_empty_dirs(file_path.parent(), auto_import_dir);
            // Return Ok so worker continues
            Ok(Some(display_path))
        }
    }
}

/// Move a file to the failed/ directory, preserving its relative path structure.
fn move_to_failed(file_path: &Path, base_dir: &Path) {
    let relative = file_path.strip_prefix(base_dir).unwrap_or(file_path);
    let dest = base_dir.join(FAILED_DIRECTORY).join(relative);

    // Create parent directories in failed/
    if let Some(parent) = dest.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        warn!(
            dest = %parent.display(),
            error = %e,
            "Failed to create directory in failed/"
        );
        return;
    }

    if let Err(e) = std::fs::rename(file_path, &dest) {
        warn!(
            file = %file_path.display(),
            dest = %dest.display(),
            error = %e,
            "Failed to move file to failed/, attempting copy"
        );
        // If rename fails (e.g., cross-filesystem), try copy+delete
        if let Err(copy_err) = std::fs::copy(file_path, &dest) {
            warn!(
                file = %file_path.display(),
                error = %copy_err,
                "Failed to copy file to failed/, leaving in place"
            );
            return;
        }
        if let Err(del_err) = std::fs::remove_file(file_path) {
            warn!(
                file = %file_path.display(),
                error = %del_err,
                "Failed to delete original file after copy"
            );
        }
    }
}

/// Remove empty directories up to (but not including) the base directory.
fn cleanup_empty_dirs(start: Option<&Path>, base_dir: &Path) {
    let Some(mut dir) = start else { return };

    while dir != base_dir && dir.starts_with(base_dir) {
        // Don't remove the failed directory
        if let Some(name) = dir.file_name().and_then(|n| n.to_str())
            && name.eq_ignore_ascii_case(FAILED_DIRECTORY)
        {
            break;
        }

        match std::fs::remove_dir(dir) {
            Ok(_) => {
                debug!(dir = %dir.display(), "Removed empty directory");
            }
            Err(_) => {
                // Directory not empty or other error, stop climbing
                break;
            }
        }

        dir = match dir.parent() {
            Some(p) => p,
            None => break,
        };
    }
}

/// Check if a file has a supported extension.
fn is_supported_format(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| SUPPORTED_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Result of processing a file.
enum ProcessResult {
    /// File was successfully imported
    Imported,
    /// File was a duplicate of an existing document
    Duplicate { existing_id: String },
}

/// Process a single file for import.
async fn process_file(service: &SeneschalService, file_path: &Path) -> ServiceResult<ProcessResult> {
    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    // Compute hash for duplicate detection
    let file_hash =
        compute_file_hash(file_path).map_err(|e| ServiceError::Processing(ProcessingError::Io(e)))?;

    // Check for duplicate
    if let Some(existing_id) = service.db.get_document_by_hash(&file_hash)? {
        return Ok(ProcessResult::Duplicate { existing_id });
    }

    // Read file content
    let content =
        std::fs::read(file_path).map_err(|e| ServiceError::Processing(ProcessingError::Io(e)))?;

    // Derive title from filename (without extension)
    let title = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename)
        .to_string();

    // Use upload_document with default settings:
    // - access_level: GmOnly (as per requirements)
    // - tags: empty (as per requirements)
    // - vision_model: None (no captioning for auto-import)
    let document = service
        .upload_document(&content, filename, &title, AccessLevel::GmOnly, vec![], None)
        .await?;

    info!(
        doc_id = %document.id,
        title = %title,
        hash = %file_hash,
        "Auto-imported document queued for processing"
    );

    Ok(ProcessResult::Imported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_is_supported_format() {
        assert!(is_supported_format(&PathBuf::from("test.pdf")));
        assert!(is_supported_format(&PathBuf::from("test.PDF")));
        assert!(is_supported_format(&PathBuf::from("test.epub")));
        assert!(is_supported_format(&PathBuf::from("test.md")));
        assert!(is_supported_format(&PathBuf::from("test.markdown")));
        assert!(is_supported_format(&PathBuf::from("test.txt")));
        assert!(is_supported_format(&PathBuf::from("test.text")));

        assert!(!is_supported_format(&PathBuf::from("test.doc")));
        assert!(!is_supported_format(&PathBuf::from("test.docx")));
        assert!(!is_supported_format(&PathBuf::from("test.jpg")));
        assert!(!is_supported_format(&PathBuf::from("test")));
    }
}
