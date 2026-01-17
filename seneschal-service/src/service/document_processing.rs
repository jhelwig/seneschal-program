//! Document processing workflows including upload, chunking, embedding, and captioning.
//!
//! This module coordinates document lifecycle operations:
//! - Upload and hash backfill
//! - Background processing workers
//! - Image captioning
//! - Progress broadcasting
//! - Cancellation management
//! - CRUD operations

mod cancellation;
mod captioning;
mod crud;
mod processing;
mod progress;
mod upload;
mod workers;
