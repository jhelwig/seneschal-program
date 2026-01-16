/**
 * Seneschal - AI-powered assistant for Foundry VTT
 *
 * Main module entry point
 */

import { MODULE_ID, SETTINGS } from "./constants.mjs";
import { getSetting, buildUserContext, canUseModule, parseMarkdown } from "./utils.mjs";
import { BackendClient } from "./clients/backend.mjs";
import { WebSocketClient } from "./clients/websocket.mjs";
import { ConversationSession } from "./clients/session.mjs";
import { FvttApiWrapper } from "./api/index.mjs";
import { ToolExecutor } from "./tools/index.mjs";
import { SeneschalSidebarTab } from "./ui/sidebar.mjs";
import { DocumentManagementDialog } from "./ui/dialogs/documents.mjs";
import { ImageBrowserDialog } from "./ui/dialogs/images.mjs";
import { BackendSettingsDialog } from "./ui/dialogs/settings.mjs";

// Re-export for advanced usage
export {
  SeneschalSidebarTab,
  DocumentManagementDialog,
  ImageBrowserDialog,
  BackendSettingsDialog,
  BackendClient,
  ConversationSession,
  FvttApiWrapper,
  ToolExecutor,
};
export { buildUserContext, canUseModule } from "./utils.mjs";
export { saveImageToFVTT } from "./utils.mjs";

// ============================================================================
// One-Shot Chat Command
// ============================================================================

/**
 * Handle one-shot AI query from chat
 */
async function handleOneShotQuery(query) {
  const backendClient = new BackendClient();

  if (!backendClient.isConfigured()) {
    ui.notifications.error(game.i18n.localize("SENESCHAL.Notifications.NotConfigured"));
    return;
  }

  // Show "thinking" indicator
  const thinkingMsg = await ChatMessage.create({
    content: `<div class="seneschal-chat-result"><div class="seneschal-header"><i class="fas fa-hat-wizard"></i> <strong>${game.i18n.localize("SENESCHAL.Name")}</strong></div><div class="seneschal-content seneschal-thinking">${game.i18n.localize("SENESCHAL.Thinking")}</div></div>`,
    speaker: { alias: game.i18n.localize("SENESCHAL.Name") },
    whisper: game.user.role < CONST.USER_ROLES.GAMEMASTER ? [game.user.id] : [],
  });

  try {
    const userContext = buildUserContext();
    const response = await backendClient.chat({
      messages: [{ role: "user", content: query }],
      user_context: userContext,
      stream: false,
    });

    // Update message with result
    await thinkingMsg.update({
      content: `<div class="seneschal-chat-result"><div class="seneschal-header"><i class="fas fa-hat-wizard"></i> <strong>${game.i18n.localize("SENESCHAL.Name")}</strong></div><div class="seneschal-content">${parseMarkdown(response.content)}</div></div>`,
    });
  } catch (error) {
    await thinkingMsg.update({
      content: `<div class="seneschal-chat-result"><div class="seneschal-header"><i class="fas fa-hat-wizard"></i> <strong>${game.i18n.localize("SENESCHAL.Name")}</strong></div><div class="seneschal-content seneschal-error">${game.i18n.localize("SENESCHAL.Error")}: ${error.message}</div></div>`,
    });
  }
}

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

  game.settings.register(MODULE_ID, SETTINGS.CHAT_COMMAND_PREFIX, {
    name: game.i18n.localize("SENESCHAL.Settings.ChatCommandPrefix"),
    hint: game.i18n.localize("SENESCHAL.Settings.ChatCommandPrefixHint"),
    scope: "world",
    config: true,
    type: String,
    default: "/sen-ai",
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

  // Initialize WebSocket client for real-time updates
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

// Use Hooks.once to run only when sidebar first renders
Hooks.once("renderSidebar", () => {
  // Only add if user can use the module
  if (!canUseModule()) {
    console.log(`${MODULE_ID} | User cannot use module, skipping sidebar tab`);
    return;
  }

  console.log(`${MODULE_ID} | Adding Seneschal sidebar tab`);

  // Get sidebar elements for Foundry v13 (same selectors as Traveller Toolkit v13 code path)
  const sidebarContent = document.querySelector("#sidebar-content");
  const tabsMenu = document.querySelector("#sidebar-tabs menu");

  if (!sidebarContent || !tabsMenu) {
    console.error(`${MODULE_ID} | Could not find sidebar elements`, { sidebarContent, tabsMenu });
    return;
  }

  console.log(`${MODULE_ID} | Found sidebar elements`, { sidebarContent, tabsMenu });

  // Create the tab button structure for Foundry v13 (based on Traveller Toolkit):
  // <li>
  //   <button type="button" class="ui-control plain icon fas fa-hat-wizard"
  //           data-action="tab" data-tab="seneschal" data-tooltip=""
  //           data-group="primary" role="tab" aria-pressed="false"
  //           aria-label="Seneschal Program" aria-controls="seneschal">
  //   </button>
  //   <div class="notification-pip"></div>
  // </li>
  // NOTE: The icon class goes on the button itself, no <i> element inside
  const tooltip = game.i18n.localize("SENESCHAL.PanelTitle");

  const tabLi = document.createElement("li");

  const tabButton = document.createElement("button");
  tabButton.type = "button";
  tabButton.className = "ui-control plain icon fas fa-hat-wizard";
  tabButton.dataset.action = "tab";
  tabButton.dataset.tab = "seneschal";
  tabButton.dataset.tooltip = "";
  tabButton.dataset.group = "primary";
  tabButton.setAttribute("role", "tab");
  tabButton.setAttribute("aria-pressed", "false");
  tabButton.setAttribute("aria-label", tooltip);
  tabButton.setAttribute("aria-controls", "seneschal");
  // No innerHTML - the icon is rendered via the class on the button itself

  const notificationPip = document.createElement("div");
  notificationPip.className = "notification-pip";

  tabLi.appendChild(tabButton);
  tabLi.appendChild(notificationPip);

  // Find the card stacks button and insert after it
  // Card stacks button has class "fa-solid fa-cards" or "fa-cards"
  const cardStacksButton = tabsMenu.querySelector("button.fa-cards, button.fa-solid.fa-cards");
  if (cardStacksButton) {
    const cardStacksLi = cardStacksButton.closest("li");
    if (cardStacksLi && cardStacksLi.nextSibling) {
      tabsMenu.insertBefore(tabLi, cardStacksLi.nextSibling);
    } else if (cardStacksLi) {
      tabsMenu.appendChild(tabLi);
    } else {
      tabsMenu.appendChild(tabLi);
    }
  } else {
    // Fallback: insert before the expand/collapse button (first <li> without data-tab button)
    const allLis = tabsMenu.querySelectorAll("li");
    let expandLi = null;
    for (const li of allLis) {
      const btn = li.querySelector("button[data-tab]");
      if (!btn) {
        expandLi = li;
        break;
      }
    }
    if (expandLi) {
      tabsMenu.insertBefore(tabLi, expandLi);
    } else {
      tabsMenu.appendChild(tabLi);
    }
  }

  // Create the content section
  const contentSection = document.createElement("section");
  contentSection.classList.add("tab", "sidebar-tab", "seneschal-sidebar", "directory", "flexcol");
  contentSection.id = "seneschal";
  contentSection.dataset.tab = "seneschal";
  contentSection.dataset.group = "primary";

  // Add content section to sidebar-content
  sidebarContent.appendChild(contentSection);

  // Create our tab instance and store it on ui
  ui.seneschal = new SeneschalSidebarTab();

  // Render initial content
  async function renderContent() {
    const data = ui.seneschal.getData();
    const templatePath = `modules/${MODULE_ID}/templates/panel.hbs`;
    const content = await renderTemplate(templatePath, data);
    contentSection.innerHTML = content;
    ui.seneschal._element = $(contentSection);
    ui.seneschal.activateListeners($(contentSection));
  }

  // Handle tab clicks - use Foundry's built-in method
  tabButton.addEventListener("click", async (event) => {
    // Let Foundry handle the tab switching via _onLeftClickTab
    if (ui.sidebar?._onLeftClickTab) {
      ui.sidebar._onLeftClickTab(event);
    }

    // Render our content when tab becomes active
    await renderContent();
  });

  console.log(`${MODULE_ID} | Sidebar tab added successfully`);
});

Hooks.on("chatMessage", (_chatLog, message, _chatData) => {
  const prefix = getSetting(SETTINGS.CHAT_COMMAND_PREFIX);
  if (!message.startsWith(prefix + " ")) return true;

  if (!canUseModule()) {
    ui.notifications.warn(game.i18n.localize("SENESCHAL.Notifications.PlayerAccessDisabled"));
    return false;
  }

  const query = message.slice(prefix.length + 1);
  handleOneShotQuery(query);
  return false;
});
