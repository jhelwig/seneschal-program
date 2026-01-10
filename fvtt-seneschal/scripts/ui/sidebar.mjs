/**
 * Seneschal sidebar tab - main interface in right sidebar
 */

import { MODULE_ID } from "../constants.mjs";
import { buildUserContext, parseMarkdown } from "../utils.mjs";
import { BackendClient } from "../clients/backend.mjs";
import { ConversationSession } from "../clients/session.mjs";
import { ToolExecutor } from "../tools/index.mjs";
import { DocumentManagementDialog } from "./dialogs/documents.mjs";

/**
 * Seneschal sidebar tab - main interface in right sidebar
 * Note: We don't extend SidebarTab because it's not available as a global in Foundry VTT.
 * Instead, we create and manage DOM elements directly.
 */
export class SeneschalSidebarTab {
  constructor() {
    this.session = new ConversationSession();
    this.backendClient = new BackendClient();
    this.isProcessing = false;
    this.isThinking = false;
    this.isPaused = false;
    this.toolStatus = null;
    this.pauseMessage = null;
    this.documentDialog = null;
    this._element = null; // Store reference to our DOM element
  }

  /**
   * Re-render the tab content
   */
  async render() {
    if (!this._element) return;

    const data = this.getData();
    const templatePath = `modules/${MODULE_ID}/templates/panel.hbs`;
    const content = await renderTemplate(templatePath, data);
    this._element.html(content);
    this.activateListeners(this._element);
  }

  /**
   * Get template data
   */
  getData() {
    return {
      messages: this.session.messages.map((m) => ({
        role: m.role,
        content: m.role === "assistant" ? parseMarkdown(m.content) : m.content,
      })),
      isProcessing: this.isProcessing,
      isThinking: this.isThinking,
      isPaused: this.isPaused,
      toolStatus: this.toolStatus,
      pauseMessage: this.pauseMessage,
    };
  }

  /**
   * Activate listeners
   */
  activateListeners(html) {
    // Store reference to our element
    this._element = html;

    // Send button
    html.find(".seneschal-send").click(this._onSendMessage.bind(this));

    // Input textarea
    const textarea = html.find(".seneschal-input");
    textarea.on("keydown", (e) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        this._onSendMessage();
      }
    });

    // Auto-resize textarea
    textarea.on("input", function () {
      this.style.height = "auto";
      this.style.height = Math.min(this.scrollHeight, 128) + "px";
    });

    // New conversation
    html.find(".seneschal-new-conversation").click(this._onNewConversation.bind(this));

    // Clear history
    html.find(".seneschal-clear-history").click(this._onClearHistory.bind(this));

    // Document management
    html.find(".seneschal-documents").click(this._onOpenDocuments.bind(this));

    // Pause controls
    html.find(".seneschal-continue").click(() => this._onContinue("continue"));
    html.find(".seneschal-cancel").click(() => this._onContinue("cancel"));

    // Scroll to bottom
    this._scrollToBottom(html);
  }

  /**
   * Handle send message
   */
  _onSendMessage() {
    if (this.isProcessing) return;

    const textarea = this._element.find(".seneschal-input");
    const content = textarea.val()?.trim();
    if (!content) return;

    // Check configuration
    if (!this.backendClient.isConfigured()) {
      ui.notifications.error(game.i18n.localize("SENESCHAL.Notifications.NotConfigured"));
      return;
    }

    // Check WebSocket is connected
    if (!globalThis.seneschalWS?.authenticated) {
      ui.notifications.error(game.i18n.localize("SENESCHAL.Notifications.WebSocketNotConnected"));
      return;
    }

    // Clear input
    textarea.val("");
    textarea.css("height", "auto");

    // Add user message
    this.session.addMessage({ role: "user", content });
    this.isProcessing = true;
    this.isThinking = true;
    this.render();

    // Store user context for tool calls
    this._currentUserContext = buildUserContext();

    // Start chat via WebSocket (not async - just sends message and registers handlers)
    this.backendClient.streamChat({
      messages: this.session.getMessagesForContext(),
      conversationId: this.session.id,
      tools: [
        "document_search",
        "fvtt_read",
        "fvtt_write",
        "fvtt_query",
        "dice_roll",
        "system_schema",
        "create_scene",
      ],
      onChunk: (text) => this._onChunk(text),
      onToolCall: (id, tool, args) => this._onToolCall(id, tool, args),
      onToolStatus: (message) => this._onToolStatus(message),
      onPause: (reason, toolCalls, elapsed, message) =>
        this._onPause(reason, toolCalls, elapsed, message),
      onComplete: (fullResponse, toolCalls) => this._onComplete(fullResponse, toolCalls),
      onError: (error) => this._onError(error),
    });
  }

  /**
   * Handle content chunk
   */
  _onChunk(text) {
    this.isThinking = false;
    this.toolStatus = null;

    // Get or create response element
    let responseEl = this._element.find(".seneschal-response-streaming");
    if (responseEl.length === 0) {
      // Add new message element
      const messagesEl = this._element.find(".seneschal-messages");
      messagesEl.append(
        `<div class="seneschal-message assistant seneschal-response-streaming"><div class="seneschal-message-content"></div></div>`
      );
      responseEl = this._element.find(".seneschal-response-streaming");
    }

    const contentEl = responseEl.find(".seneschal-message-content");
    const currentContent = contentEl.data("raw") || "";
    const newContent = currentContent + text;
    contentEl.data("raw", newContent);
    contentEl.html(parseMarkdown(newContent));

    this._scrollToBottom();
  }

  /**
   * Handle tool call request
   */
  async _onToolCall(id, tool, args) {
    this.toolStatus = game.i18n.localize(`SENESCHAL.ToolStatus.Processing`);
    this.render();

    // Execute the tool using stored user context
    const result = await ToolExecutor.execute(tool, args, this._currentUserContext);

    // Send result back to backend via WebSocket
    // The agentic loop will continue automatically
    this.backendClient.sendToolResult(this.session.id, id, result);
  }

  /**
   * Handle tool status update
   */
  _onToolStatus(message) {
    this.toolStatus = message;
    this.render();
  }

  /**
   * Handle pause event
   */
  _onPause(reason, toolCalls, elapsed, message) {
    this.isPaused = true;
    this.pauseMessage = message;
    this.isProcessing = false;
    this.render();
  }

  /**
   * Handle continue/cancel after pause
   */
  _onContinue(action) {
    this.isPaused = false;
    this.pauseMessage = null;

    if (action === "continue") {
      this.isProcessing = true;
      this.isThinking = true;
      this.render();

      // Continue via WebSocket (not async)
      this.backendClient.continueChat(this.session.id, action);
    } else {
      this.isProcessing = false;
      this.render();

      // Cancel via WebSocket
      this.backendClient.continueChat(this.session.id, action);
    }
  }

  /**
   * Handle completion
   */
  _onComplete(fullResponse, toolCalls) {
    this.isProcessing = false;
    this.isThinking = false;
    this.toolStatus = null;

    // Remove streaming class
    const responseEl = this._element.find(".seneschal-response-streaming");
    responseEl.removeClass("seneschal-response-streaming");

    // Add to session
    this.session.addMessage({
      role: "assistant",
      content: fullResponse,
      toolCalls,
    });

    this.render();
  }

  /**
   * Handle error
   */
  _onError(error) {
    this.isProcessing = false;
    this.isThinking = false;
    this.toolStatus = null;

    console.error("Seneschal error:", error);

    // Add error message
    this.session.addMessage({
      role: "error",
      content: `${game.i18n.localize("SENESCHAL.Error")}: ${error.message}`,
    });

    this.render();
  }

  /**
   * Handle new conversation
   */
  _onNewConversation() {
    this.session.clear();
    this.isProcessing = false;
    this.isThinking = false;
    this.isPaused = false;
    this.toolStatus = null;
    this.render();
  }

  /**
   * Handle clear history
   */
  _onClearHistory() {
    this._onNewConversation();
  }

  /**
   * Handle opening document management dialog
   */
  _onOpenDocuments() {
    if (!this.documentDialog) {
      this.documentDialog = new DocumentManagementDialog();
    }
    this.documentDialog.render(true);
  }

  /**
   * Scroll messages to bottom
   */
  _scrollToBottom(html) {
    const messagesEl = (html || this._element).find(".seneschal-messages")[0];
    if (messagesEl) {
      messagesEl.scrollTop = messagesEl.scrollHeight;
    }
  }
}
