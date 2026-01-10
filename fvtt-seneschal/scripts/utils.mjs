/**
 * Utility functions for the Seneschal module
 */

import { MODULE_ID, SETTINGS } from "./constants.mjs";

/**
 * Get a module setting
 * @param {string} key - Setting key
 * @returns {*} Setting value
 */
export function getSetting(key) {
  return game.settings.get(MODULE_ID, key);
}

/**
 * Build user context from current game state
 * @returns {Object} User context for backend requests
 */
export function buildUserContext() {
  const user = game.user;
  const character = user.character;

  return {
    user_id: user.id,
    user_name: user.name,
    role: user.role,
    owned_actor_ids:
      game.actors?.filter((a) => a.testUserPermission(user, "OWNER")).map((a) => a.id) ?? [],
    character_id: character?.id ?? null,
  };
}

/**
 * Check if current user can use Seneschal
 * @returns {boolean}
 */
export function canUseModule() {
  const user = game.user;
  // GM can always use
  if (user.role >= CONST.USER_ROLES.GAMEMASTER) return true;
  // Players need setting enabled
  return getSetting(SETTINGS.ENABLE_PLAYER_ACCESS);
}

/**
 * Generate a unique ID
 * @returns {string}
 */
export function generateId() {
  return foundry.utils.randomID(16);
}

/**
 * Parse markdown to HTML (basic implementation)
 * @param {string} text - Markdown text
 * @returns {string} HTML
 */
export function parseMarkdown(text) {
  // Use marked if available, otherwise basic conversion
  if (typeof marked !== "undefined") {
    return marked.parse(text);
  }

  // Basic markdown conversion
  return (
    text
      // Bold
      .replace(/\*\*(.*?)\*\*/g, "<strong>$1</strong>")
      // Italic
      .replace(/\*(.*?)\*/g, "<em>$1</em>")
      // Code blocks
      .replace(/```(\w*)\n([\s\S]*?)```/g, "<pre><code>$2</code></pre>")
      // Inline code
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      // Headers
      .replace(/^### (.*$)/gm, "<h3>$1</h3>")
      .replace(/^## (.*$)/gm, "<h2>$1</h2>")
      .replace(/^# (.*$)/gm, "<h1>$1</h1>")
      // Lists
      .replace(/^\* (.*$)/gm, "<li>$1</li>")
      .replace(/^- (.*$)/gm, "<li>$1</li>")
      // Paragraphs
      .replace(/\n\n/g, "</p><p>")
      .replace(/^(.+)$/gm, (match) => {
        if (match.startsWith("<")) return match;
        return `<p>${match}</p>`;
      })
  );
}

/**
 * Save an image to FVTT via shuttle mode (FilePicker.uploadPersistent)
 * Used when the backend cannot write directly to FVTT assets
 * @param {string} imageId - Image ID from backend
 * @param {string} targetPath - Target path within FVTT assets
 * @param {Object} client - Backend client instance
 * @returns {Promise<string>} The FVTT path where the image was saved
 */
export async function saveImageToFVTT(imageId, targetPath, client) {
  // Fetch image data from backend
  const blob = await client.getImageData(imageId);

  // Convert to File object
  const filename = targetPath.split("/").pop();
  const file = new File([blob], filename, { type: "image/webp" });

  // Extract directory path (everything before the filename)
  const dirPath = targetPath.replace(/\/[^/]+$/, "");

  // Upload to module storage
  const result = await FilePicker.uploadPersistent(MODULE_ID, dirPath, file);

  return result.path;
}
