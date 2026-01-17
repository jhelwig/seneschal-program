//! Error types for Traveller Map API.

#[derive(Debug, thiserror::Error)]
pub enum TravellerMapError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("API error (status {status}): {message}")]
    ApiError { status: u16, message: String },

    #[error("No route found between {start} and {end}")]
    NoRouteFound { start: String, end: String },

    #[error("Not found: {message}")]
    NotFound { message: String },
}
