/**
 * Seneschal - AI-powered assistant for Foundry VTT
 *
 * Main module entry point
 */

import { MODULE_ID, SETTINGS } from "./constants.mjs";
import { getSetting } from "./utils.mjs";
import { BackendClient } from "./clients/backend.mjs";
import { WebSocketClient } from "./clients/websocket.mjs";
import { FvttApiWrapper } from "./api/index.mjs";
import { ToolExecutor } from "./tools/index.mjs";
import { DocumentManagementDialog } from "./ui/dialogs/documents.mjs";
import { ImageBrowserDialog } from "./ui/dialogs/images.mjs";
import { BackendSettingsDialog } from "./ui/dialogs/settings.mjs";

// Re-export for advanced usage
export {
  DocumentManagementDialog,
  ImageBrowserDialog,
  BackendSettingsDialog,
  BackendClient,
  FvttApiWrapper,
  ToolExecutor,
};
export { buildUserContext, canUseModule } from "./utils.mjs";
export { saveImageToFVTT } from "./utils.mjs";

// ============================================================================
// Settings Registration
// ============================================================================

/**
 * Register module settings
 */
function registerSettings() {
  game.settings.register(MODULE_ID, SETTINGS.BACKEND_URL, {
    name: game.i18n.localize("SENESCHAL.Settings.BackendUrl"),
    hint: game.i18n.localize("SENESCHAL.Settings.BackendUrlHint"),
    scope: "world",
    config: true,
    type: String,
    default: "",
  });

  // Register settings menu for backend configuration
  game.settings.registerMenu(MODULE_ID, "backendSettings", {
    name: game.i18n.localize("SENESCHAL.Settings.Backend.MenuName"),
    label: game.i18n.localize("SENESCHAL.Settings.Backend.MenuLabel"),
    hint: game.i18n.localize("SENESCHAL.Settings.Backend.MenuHint"),
    icon: "fas fa-cogs",
    type: BackendSettingsDialog,
    restricted: true,
  });

  game.settings.register(MODULE_ID, SETTINGS.ENABLE_PLAYER_ACCESS, {
    name: game.i18n.localize("SENESCHAL.Settings.EnablePlayerAccess"),
    hint: game.i18n.localize("SENESCHAL.Settings.EnablePlayerAccessHint"),
    scope: "world",
    config: true,
    type: Boolean,
    default: false,
  });

  game.settings.register(MODULE_ID, SETTINGS.MAX_ACTIONS_PER_REQUEST, {
    name: game.i18n.localize("SENESCHAL.Settings.MaxActionsPerRequest"),
    hint: game.i18n.localize("SENESCHAL.Settings.MaxActionsPerRequestHint"),
    scope: "world",
    config: true,
    type: Number,
    default: 5,
    range: {
      min: 1,
      max: 20,
      step: 1,
    },
  });

  // Register document management menu
  game.settings.registerMenu(MODULE_ID, "documentManagement", {
    name: game.i18n.localize("SENESCHAL.Documents.MenuName"),
    label: game.i18n.localize("SENESCHAL.Documents.MenuLabel"),
    hint: game.i18n.localize("SENESCHAL.Documents.MenuHint"),
    icon: "fas fa-folder-open",
    type: DocumentManagementDialog,
    restricted: true,
  });
}

// ============================================================================
// Hooks
// ============================================================================

Hooks.once("init", () => {
  console.log(`${MODULE_ID} | Initializing Seneschal`);
  registerSettings();
});

Hooks.once("ready", async () => {
  console.log(`${MODULE_ID} | Seneschal ready`);

  // Check if backend is configured
  const backendUrl = getSetting(SETTINGS.BACKEND_URL);
  if (!backendUrl && game.user.isGM) {
    ui.notifications.warn(game.i18n.localize("SENESCHAL.Notifications.NotConfigured"));
    return;
  }

  // Initialize WebSocket client for real-time updates (needed for MCP external tools)
  if (backendUrl) {
    globalThis.seneschalWS = new WebSocketClient();
    try {
      await globalThis.seneschalWS.connect();
      console.log(`${MODULE_ID} | WebSocket connected successfully`);
    } catch (error) {
      console.error(`${MODULE_ID} | WebSocket connection failed:`, error);
      // Will auto-reconnect in the background
    }
  }
});
