use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};
use std::collections::HashMap;
use std::sync::RwLock;
use tracing::{debug, warn};
use unic_langid::LanguageIdentifier;

/// Internationalization service using Fluent (thread-safe)
pub struct I18n {
    bundles: RwLock<HashMap<String, FluentBundle<FluentResource>>>,
    default_locale: String,
}

impl I18n {
    /// Create a new i18n service with embedded English translations
    pub fn new() -> Self {
        let i18n = Self {
            bundles: RwLock::new(HashMap::new()),
            default_locale: "en".to_string(),
        };

        // Load embedded English translations
        i18n.load_embedded_en();

        i18n
    }

    /// Add a locale with translations
    pub fn add_locale(&self, locale: &str, content: &str) -> Result<(), String> {
        let lang_id: LanguageIdentifier = locale
            .parse()
            .map_err(|e| format!("Invalid locale '{}': {}", locale, e))?;

        let resource = FluentResource::try_new(content.to_string())
            .map_err(|(_, errors)| format!("Failed to parse Fluent resource: {:?}", errors))?;

        let mut bundle = FluentBundle::new_concurrent(vec![lang_id]);
        bundle
            .add_resource(resource)
            .map_err(|errors| format!("Failed to add resource to bundle: {:?}", errors))?;

        let mut bundles = self.bundles.write().unwrap();
        bundles.insert(locale.to_string(), bundle);

        debug!(locale = %locale, "Loaded translations");

        Ok(())
    }

    /// Get a translated message
    pub fn get(&self, locale: &str, key: &str, args: Option<&FluentArgs>) -> String {
        // Try requested locale, fall back to default, fall back to key
        self.try_get(locale, key, args)
            .or_else(|| self.try_get(&self.default_locale, key, args))
            .unwrap_or_else(|| key.to_string())
    }

    /// Try to get a translation from a specific locale
    fn try_get(&self, locale: &str, key: &str, args: Option<&FluentArgs>) -> Option<String> {
        let bundles = self.bundles.read().unwrap();
        let bundle = bundles.get(locale)?;
        let message = bundle.get_message(key)?;
        let pattern = message.value()?;

        let mut errors = vec![];
        let result = bundle.format_pattern(pattern, args, &mut errors);

        if !errors.is_empty() {
            warn!(key = %key, errors = ?errors, "Fluent formatting errors");
        }

        Some(result.to_string())
    }

    /// Get a translated message with arguments
    pub fn format(&self, locale: &str, key: &str, args: &[(&str, &str)]) -> String {
        let mut fluent_args = FluentArgs::new();
        for (k, v) in args {
            fluent_args.set(*k, *v);
        }
        self.get(locale, key, Some(&fluent_args))
    }

    /// Load embedded English translations
    fn load_embedded_en(&self) {
        let en_translations = r#"
# Seneschal Program Service - English Translations

# Errors
error-permission-denied = Permission denied: { $action } on { $resource }
error-document-not-found = Document not found: { $id }
error-conversation-not-found = Conversation not found: { $id }
error-rate-limit = Rate limit exceeded. Please try again in { $seconds } seconds.
error-timeout = Request timed out
error-internal = An internal error occurred

# Chat
chat-thinking = Thinking...
chat-searching = Searching documents...
chat-executing-tool = Executing: { $tool }
chat-tool-complete = Completed: { $tool }
chat-pause-tool-limit = Seneschal Program has made { $count } tool calls. Would you like to continue?
chat-pause-time-limit = Seneschal Program has been working for { $seconds } seconds. Would you like to continue?

# Documents
doc-upload-success = Document uploaded successfully
doc-upload-processing = Processing document...
doc-delete-success = Document deleted successfully
doc-not-found = Document not found

# Search
search-no-results = No relevant documents found
search-results-count = Found { $count } relevant results

# MCP
mcp-connected = MCP client connected
mcp-disconnected = MCP client disconnected

# Health
health-status-healthy = Service is healthy
health-status-degraded = Service is degraded: { $reason }
"#;

        if let Err(e) = self.add_locale("en", en_translations) {
            warn!(error = %e, "Failed to load embedded English translations");
        }
    }
}

impl Default for I18n {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_message() {
        let i18n = I18n::new();

        let msg = i18n.get("en", "chat-thinking", None);
        assert_eq!(msg, "Thinking...");
    }

    #[test]
    fn test_format_message() {
        let i18n = I18n::new();

        let msg = i18n.format("en", "search-results-count", &[("count", "5")]);
        // Fluent adds Unicode bidi isolation characters around variables
        // U+2068 (First Strong Isolate) and U+2069 (Pop Directional Isolate)
        assert_eq!(msg, "Found \u{2068}5\u{2069} relevant results");
    }

    #[test]
    fn test_fallback_to_key() {
        let i18n = I18n::new();

        let msg = i18n.get("en", "nonexistent-key", None);
        assert_eq!(msg, "nonexistent-key");
    }

    #[test]
    fn test_fallback_to_default_locale() {
        let i18n = I18n::new();

        // Request French (not loaded), should fall back to English
        let msg = i18n.get("fr", "chat-thinking", None);
        assert_eq!(msg, "Thinking...");
    }
}
