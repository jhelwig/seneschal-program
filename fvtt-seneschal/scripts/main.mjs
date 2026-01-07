/**
 * Seneschal - AI-powered assistant for Foundry VTT
 *
 * Main module entry point
 */

// ============================================================================
// Constants & Configuration
// ============================================================================

const MODULE_ID = "fvtt-seneschal";

const SETTINGS = {
  BACKEND_URL: "backendUrl",
  SELECTED_MODEL: "selectedModel",
  VISION_MODEL: "visionModel",
  ENABLE_PLAYER_ACCESS: "enablePlayerAccess",
  MAX_ACTIONS_PER_REQUEST: "maxActionsPerRequest",
  CHAT_COMMAND_PREFIX: "chatCommandPrefix",
};

// ============================================================================
// Utility Functions
// ============================================================================

/**
 * Get a module setting
 * @param {string} key - Setting key
 * @returns {*} Setting value
 */
function getSetting(key) {
  return game.settings.get(MODULE_ID, key);
}

/**
 * Build user context from current game state
 * @returns {Object} User context for backend requests
 */
function buildUserContext() {
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
function canUseModule() {
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
function generateId() {
  return foundry.utils.randomID(16);
}

/**
 * Parse markdown to HTML (basic implementation)
 * @param {string} text - Markdown text
 * @returns {string} HTML
 */
function parseMarkdown(text) {
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

// ============================================================================
// Backend Client
// ============================================================================

/**
 * Client for communicating with the Seneschal backend service
 */
class BackendClient {
  constructor() {
    this.abortController = null;
  }

  /**
   * Get the backend URL
   * @returns {string}
   */
  get baseUrl() {
    return getSetting(SETTINGS.BACKEND_URL);
  }

  /**
   * Get request headers
   * @returns {Object}
   */
  get headers() {
    return {
      "Content-Type": "application/json",
    };
  }

  /**
   * Check if backend is configured
   * @returns {boolean}
   */
  isConfigured() {
    return !!this.baseUrl;
  }

  /**
   * Health check
   * @returns {Promise<Object>}
   */
  async healthCheck() {
    const response = await fetch(`${this.baseUrl}/health`, {
      method: "GET",
      headers: this.headers,
    });
    return response.json();
  }

  /**
   * Get available models
   * @returns {Promise<Array>}
   */
  async getModels() {
    const response = await fetch(`${this.baseUrl}/api/models`, {
      method: "GET",
      headers: this.headers,
    });
    if (!response.ok) {
      const errorBody = await response.json().catch(() => ({}));
      throw new Error(`${response.status}: ${errorBody.message || response.statusText}`);
    }
    return response.json();
  }

  /**
   * Get the selected model
   * @returns {string|null}
   */
  getSelectedModel() {
    const model = getSetting(SETTINGS.SELECTED_MODEL);
    return model || null;
  }

  /**
   * Send chat request (non-streaming)
   * @param {Object} options - Chat options
   * @returns {Promise<Object>}
   */
  async chat(options) {
    const model = this.getSelectedModel();
    const response = await fetch(`${this.baseUrl}/api/chat`, {
      method: "POST",
      headers: this.headers,
      body: JSON.stringify({
        ...options,
        model: model || undefined,
        stream: false,
      }),
    });
    if (!response.ok) {
      const errorBody = await response.json().catch(() => ({}));
      throw new Error(`${response.status}: ${errorBody.message || response.statusText}`);
    }
    return response.json();
  }

  /**
   * Send streaming chat request
   * @param {Object} options - Chat options
   * @param {Function} options.onChunk - Called with each text chunk
   * @param {Function} options.onToolCall - Called when tool call is needed
   * @param {Function} options.onToolStatus - Called with tool status updates
   * @param {Function} options.onPause - Called when loop pauses
   * @param {Function} options.onComplete - Called when done
   * @param {Function} options.onError - Called on error
   */
  async streamChat(options) {
    const {
      messages,
      userContext,
      conversationId,
      tools,
      onChunk,
      onToolCall,
      onToolStatus,
      onPause,
      onComplete,
      onError,
    } = options;

    this.abortController = new AbortController();
    const model = this.getSelectedModel();

    try {
      const response = await fetch(`${this.baseUrl}/api/chat`, {
        method: "POST",
        headers: this.headers,
        body: JSON.stringify({
          messages,
          model: model || undefined,
          user_context: userContext,
          conversation_id: conversationId,
          tools,
          stream: true,
        }),
        signal: this.abortController.signal,
      });

      if (!response.ok) {
        const errorBody = await response.json().catch(() => ({}));
        throw new Error(`${response.status}: ${errorBody.message || response.statusText}`);
      }

      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      let fullContent = "";
      const allToolCalls = [];

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split("\n");
        buffer = lines.pop() || "";

        for (const line of lines) {
          if (line.startsWith("data: ")) {
            const data = line.slice(6);
            if (data === "[DONE]") continue;

            try {
              const event = JSON.parse(data);
              await this._handleStreamEvent(event, {
                onChunk,
                onToolCall,
                onToolStatus,
                onPause,
                fullContent: (text) => {
                  fullContent += text;
                },
                toolCalls: (tc) => {
                  allToolCalls.push(tc);
                },
              });
            } catch (e) {
              console.warn("Failed to parse SSE event:", data, e);
            }
          }
        }
      }

      if (onComplete) {
        onComplete(fullContent, allToolCalls);
      }
    } catch (error) {
      if (error.name === "AbortError") {
        console.log("Chat request aborted");
      } else if (onError) {
        onError(error);
      }
    } finally {
      this.abortController = null;
    }
  }

  /**
   * Handle a stream event
   * @private
   */
  async _handleStreamEvent(event, handlers) {
    const { onChunk, onToolCall, onToolStatus, onPause, fullContent, toolCalls } = handlers;

    switch (event.type) {
      case "content":
        if (onChunk) onChunk(event.text);
        fullContent(event.text);
        break;

      case "tool_call":
        if (onToolCall) {
          await onToolCall(event.id, event.tool, event.args);
        }
        toolCalls({ id: event.id, tool: event.tool, args: event.args });
        break;

      case "tool_status":
        if (onToolStatus) onToolStatus(event.message);
        break;

      case "tool_result":
        // Internal tool completed - optional transparency event
        // Display status message showing what tool completed
        if (onToolStatus && event.summary) {
          onToolStatus(`${event.tool}: ${event.summary}`);
        }
        break;

      case "pause":
        if (onPause) {
          onPause(event.reason, event.tool_calls_made, event.elapsed_seconds, event.message);
        }
        break;

      case "error": {
        const error = new Error(event.message);
        error.recoverable = event.recoverable ?? false;
        throw error;
      }

      case "done":
        // Handled in streamChat
        break;
    }
  }

  /**
   * Send tool result and process the continuation SSE stream
   * @param {string} conversationId
   * @param {string} toolCallId
   * @param {*} result
   * @param {Object} handlers - Event handlers for the continuation stream
   */
  async sendToolResult(conversationId, toolCallId, result, handlers = {}) {
    const { onChunk, onToolCall, onToolStatus, onPause, onComplete, onError: _onError } = handlers;

    const response = await fetch(`${this.baseUrl}/api/tool_result`, {
      method: "POST",
      headers: this.headers,
      body: JSON.stringify({
        conversation_id: conversationId,
        tool_call_id: toolCallId,
        result,
      }),
    });

    if (!response.ok) {
      const errorBody = await response.json().catch(() => ({}));
      throw new Error(`${response.status}: ${errorBody.message || response.statusText}`);
    }

    // Process the SSE stream continuation
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    let fullContent = "";
    const allToolCalls = [];

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split("\n");
      buffer = lines.pop() || "";

      for (const line of lines) {
        if (line.startsWith("data: ")) {
          const data = line.slice(6);
          if (data === "[DONE]") continue;

          try {
            const event = JSON.parse(data);
            await this._handleStreamEvent(event, {
              onChunk,
              onToolCall,
              onToolStatus,
              onPause,
              fullContent: (text) => {
                fullContent += text;
              },
              toolCalls: (tc) => {
                allToolCalls.push(tc);
              },
            });
          } catch (e) {
            console.warn("Failed to parse SSE event:", data, e);
          }
        }
      }
    }

    if (onComplete) {
      onComplete(fullContent, allToolCalls);
    }
  }

  /**
   * Continue after pause
   * @param {string} conversationId
   * @param {string} action - "continue" or "cancel"
   */
  async continueChat(conversationId, action) {
    const response = await fetch(`${this.baseUrl}/api/chat/continue`, {
      method: "POST",
      headers: this.headers,
      body: JSON.stringify({
        conversation_id: conversationId,
        action,
      }),
    });
    if (!response.ok) {
      const errorBody = await response.json().catch(() => ({}));
      throw new Error(`${response.status}: ${errorBody.message || response.statusText}`);
    }
    return response.json();
  }

  /**
   * Abort current request
   */
  abort() {
    if (this.abortController) {
      this.abortController.abort();
    }
  }

  /**
   * List documents
   * @returns {Promise<Array>}
   */
  async listDocuments() {
    const response = await fetch(`${this.baseUrl}/api/documents`, {
      method: "GET",
      headers: this.headers,
    });
    if (!response.ok) {
      throw new Error(`Failed to list documents: ${response.statusText}`);
    }
    return response.json();
  }

  /**
   * Upload a document
   * @param {File} file - The file to upload
   * @param {Object} metadata - Document metadata
   * @param {Function} onProgress - Progress callback
   * @returns {Promise<Object>}
   */
  async uploadDocument(file, metadata, onProgress) {
    const formData = new FormData();
    formData.append("file", file);
    formData.append("title", metadata.title);
    formData.append("access_level", metadata.accessLevel);
    if (metadata.tags) {
      formData.append("tags", metadata.tags);
    }
    if (metadata.visionModel) {
      formData.append("vision_model", metadata.visionModel);
    }

    return new Promise((resolve, reject) => {
      const xhr = new XMLHttpRequest();
      xhr.open("POST", `${this.baseUrl}/api/documents`);

      // Set a long timeout for PDF processing (5 minutes)
      // Vision model captioning can take a while
      xhr.timeout = 300000;

      xhr.upload.addEventListener("progress", (event) => {
        if (event.lengthComputable && onProgress) {
          const percent = Math.round((event.loaded / event.total) * 100);
          onProgress(percent);
        }
      });

      xhr.addEventListener("load", () => {
        console.debug("Upload response:", xhr.status, xhr.statusText, xhr.responseText);
        if (xhr.status >= 200 && xhr.status < 300) {
          try {
            resolve(JSON.parse(xhr.responseText));
          } catch {
            resolve({ success: true });
          }
        } else {
          // Try to extract error message from response body
          let errorMessage = `HTTP ${xhr.status}`;
          try {
            const errorBody = JSON.parse(xhr.responseText);
            if (errorBody.message) {
              errorMessage = errorBody.message;
            } else if (errorBody.error) {
              errorMessage = errorBody.error;
            }
          } catch {
            // If we can't parse JSON, use status text or response text
            if (xhr.statusText) {
              errorMessage = xhr.statusText;
            } else if (xhr.responseText) {
              errorMessage = xhr.responseText.substring(0, 200);
            }
          }
          reject(new Error(`Upload failed: ${errorMessage}`));
        }
      });

      xhr.addEventListener("error", () => {
        reject(new Error("Upload failed: Network error"));
      });

      xhr.addEventListener("timeout", () => {
        reject(new Error("Upload failed: Request timed out (server may still be processing)"));
      });

      xhr.send(formData);
    });
  }

  /**
   * Delete a document
   * @param {string} documentId
   * @returns {Promise<void>}
   */
  async deleteDocument(documentId) {
    const response = await fetch(`${this.baseUrl}/api/documents/${documentId}`, {
      method: "DELETE",
      headers: this.headers,
    });
    if (!response.ok) {
      throw new Error(`Failed to delete document: ${response.statusText}`);
    }
  }

  /**
   * Get images for a document
   * @param {string} documentId
   * @returns {Promise<Object>} Document images response
   */
  async getDocumentImages(documentId) {
    const response = await fetch(`${this.baseUrl}/api/documents/${documentId}/images`, {
      method: "GET",
      headers: this.headers,
    });
    if (!response.ok) {
      throw new Error(`Failed to get document images: ${response.statusText}`);
    }
    return response.json();
  }

  /**
   * Delete all images for a document
   * @param {string} documentId
   * @returns {Promise<Object>} Delete result with count
   */
  async deleteDocumentImages(documentId) {
    const response = await fetch(`${this.baseUrl}/api/documents/${documentId}/images`, {
      method: "DELETE",
      headers: this.headers,
    });
    if (!response.ok) {
      throw new Error(`Failed to delete document images: ${response.statusText}`);
    }
    return response.json();
  }

  /**
   * Re-extract images from a document
   * @param {string} documentId
   * @param {string} [visionModel] - Optional vision model for captioning
   * @returns {Promise<Object>} Extract result with count
   */
  async reextractDocumentImages(documentId, visionModel = null) {
    const response = await fetch(`${this.baseUrl}/api/documents/${documentId}/images/extract`, {
      method: "POST",
      headers: {
        ...this.headers,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ vision_model: visionModel }),
    });
    if (!response.ok) {
      throw new Error(`Failed to re-extract document images: ${response.statusText}`);
    }
    return response.json();
  }

  /**
   * List images from the backend
   * @param {Object} params - Query parameters
   * @param {number} [params.user_role] - User role for access filtering
   * @param {string} [params.document_id] - Filter by document ID
   * @param {number} [params.page_number] - Filter by page number
   * @param {number} [params.limit] - Maximum number of results
   * @returns {Promise<Array>} List of images
   */
  async listImages(params = {}) {
    const query = new URLSearchParams();
    if (params.user_role) query.set("user_role", params.user_role);
    if (params.document_id) query.set("document_id", params.document_id);
    if (params.page_number) query.set("page_number", params.page_number);
    if (params.limit) query.set("limit", params.limit);

    const url = `${this.baseUrl}/api/images${query.toString() ? "?" + query.toString() : ""}`;
    const response = await fetch(url, { headers: this.headers });
    if (!response.ok) {
      throw new Error(`Failed to list images: ${response.statusText}`);
    }
    const data = await response.json();
    return data.images;
  }

  /**
   * Get image metadata
   * @param {string} imageId - Image ID
   * @returns {Promise<Object>} Image metadata
   */
  async getImage(imageId) {
    const response = await fetch(`${this.baseUrl}/api/images/${imageId}`, {
      headers: this.headers,
    });
    if (!response.ok) {
      throw new Error(`Failed to get image: ${response.statusText}`);
    }
    return response.json();
  }

  /**
   * Get raw image data as a Blob
   * @param {string} imageId - Image ID
   * @returns {Promise<Blob>} Image blob
   */
  async getImageData(imageId) {
    const response = await fetch(`${this.baseUrl}/api/images/${imageId}/data`, {
      headers: this.headers,
    });
    if (!response.ok) {
      throw new Error(`Failed to get image data: ${response.statusText}`);
    }
    return response.blob();
  }

  /**
   * Request delivery of an image to FVTT assets
   * @param {string} imageId - Image ID
   * @param {string} [targetPath] - Optional target path in FVTT assets
   * @returns {Promise<Object>} Delivery result (mode: "direct" or "shuttle")
   */
  async deliverImage(imageId, targetPath = null) {
    const response = await fetch(`${this.baseUrl}/api/images/${imageId}/deliver`, {
      method: "POST",
      headers: {
        ...this.headers,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ target_path: targetPath }),
    });
    if (!response.ok) {
      throw new Error(`Failed to deliver image: ${response.statusText}`);
    }
    return response.json();
  }
}

/**
 * Save an image to FVTT via shuttle mode (FilePicker.uploadPersistent)
 * Used when the backend cannot write directly to FVTT assets
 * @param {string} imageId - Image ID from backend
 * @param {string} targetPath - Target path within FVTT assets
 * @param {BackendClient} client - Backend client instance
 * @returns {Promise<string>} The FVTT path where the image was saved
 */
async function saveImageToFVTT(imageId, targetPath, client) {
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

// ============================================================================
// Conversation Session
// ============================================================================

/**
 * Manages a conversation session
 */
class ConversationSession {
  constructor() {
    this.id = generateId();
    this.messages = [];
    this.createdAt = new Date();
    this.lastActivityAt = new Date();
    this.totalTokensEstimate = 0;
    this.maxContextTokens = 128000; // Default, updated from backend
    this.activeDocumentIds = [];
    this.activeActorIds = [];
  }

  /**
   * Add a message to the session
   * @param {Object} message
   */
  addMessage(message) {
    const msg = {
      ...message,
      timestamp: new Date(),
      tokenEstimate: this._estimateTokens(message.content),
    };
    this.messages.push(msg);
    this.totalTokensEstimate += msg.tokenEstimate;
    this.lastActivityAt = new Date();
  }

  /**
   * Get messages formatted for context
   * @returns {Array}
   */
  getMessagesForContext() {
    // Simple implementation: return all messages
    // Could be enhanced with summarization for long conversations
    return this.messages.map((m) => ({
      role: m.role,
      content: m.content,
    }));
  }

  /**
   * Clear the session
   */
  clear() {
    this.id = generateId();
    this.messages = [];
    this.totalTokensEstimate = 0;
    this.activeDocumentIds = [];
    this.activeActorIds = [];
  }

  /**
   * Estimate token count (rough approximation)
   * @private
   */
  _estimateTokens(text) {
    // Rough estimate: ~4 characters per token
    return Math.ceil((text?.length || 0) / 4);
  }
}

// ============================================================================
// FVTT API Wrapper
// ============================================================================

/**
 * Wrapper for FVTT API calls with permission checking
 */
class FvttApiWrapper {
  /**
   * Check if user can access a document
   * @param {Document} document
   * @param {Object} userContext
   * @param {string} requiredLevel - "OBSERVER", "LIMITED", "OWNER"
   * @returns {boolean}
   */
  static canAccess(document, userContext, requiredLevel = "OBSERVER") {
    const user = game.users.get(userContext.user_id);
    if (!user) return false;
    if (userContext.role >= CONST.USER_ROLES.GAMEMASTER) return true;
    return document.testUserPermission(user, requiredLevel);
  }

  /**
   * Read a FVTT document
   * @param {string} documentType - "actor", "item", "journal", etc.
   * @param {string} documentId
   * @param {Object} userContext
   * @returns {Object|null}
   */
  static read(documentType, documentId, userContext) {
    const collection = this._getCollection(documentType);
    if (!collection) return null;

    const doc = collection.get(documentId);
    if (!doc) return null;

    if (!this.canAccess(doc, userContext)) {
      return { error: "Permission denied" };
    }

    return doc.toObject();
  }

  /**
   * Query FVTT documents
   * @param {string} documentType
   * @param {Object} filters
   * @param {Object} userContext
   * @returns {Array}
   */
  static query(documentType, filters, userContext) {
    const collection = this._getCollection(documentType);
    if (!collection) return [];

    return collection
      .filter((doc) => this.canAccess(doc, userContext))
      .filter((doc) => this._matchesFilters(doc, filters))
      .map((doc) => ({
        id: doc.id,
        name: doc.name,
        type: doc.type,
      }));
  }

  /**
   * Write/create a FVTT document
   * @param {string} documentType
   * @param {string} operation - "create", "update", "delete"
   * @param {Object} data
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async write(documentType, operation, data, userContext) {
    const collection = this._getCollection(documentType);
    if (!collection) {
      return { error: `Unknown document type: ${documentType}` };
    }

    // Check if user is GM for write operations
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      // For non-GM, check if they own the document
      if (operation !== "create" && data.id) {
        const doc = collection.get(data.id);
        if (!doc || !this.canAccess(doc, userContext, "OWNER")) {
          return { error: "Permission denied" };
        }
      }
    }

    try {
      switch (operation) {
        case "create": {
          const cls = this._getDocumentClass(documentType);
          const newDoc = await cls.create(data);
          return { success: true, id: newDoc.id };
        }
        case "update": {
          const doc = collection.get(data.id);
          if (!doc) return { error: "Document not found" };
          await doc.update(data);
          return { success: true };
        }
        case "delete": {
          const doc = collection.get(data.id);
          if (!doc) return { error: "Document not found" };
          await doc.delete();
          return { success: true };
        }
        default:
          return { error: `Unknown operation: ${operation}` };
      }
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Roll dice using FVTT's dice system
   * @param {string} formula
   * @param {string} label
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async rollDice(formula, label, userContext) {
    try {
      const roll = new Roll(formula);
      await roll.evaluate();

      // Post to chat if user has permission
      if (userContext.role >= CONST.USER_ROLES.PLAYER) {
        await roll.toMessage({
          flavor: label,
          speaker: ChatMessage.getSpeaker({ user: game.users.get(userContext.user_id) }),
        });
      }

      return {
        formula: roll.formula,
        total: roll.total,
        dice: roll.dice.map((d) => ({
          faces: d.faces,
          results: d.results.map((r) => r.result),
        })),
      };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Get game system capabilities
   * @returns {Object}
   */
  static getSystemCapabilities() {
    const capabilities = {
      system: game.system.id,
      systemTitle: game.system.title,
      actorTypes: game.documentTypes.Actor || [],
      itemTypes: game.documentTypes.Item || [],
    };

    // Add schemas if available
    if (CONFIG.Actor.dataModels) {
      capabilities.actorSchemas = {};
      for (const type of capabilities.actorTypes) {
        const model = CONFIG.Actor.dataModels[type];
        if (model?.schema) {
          capabilities.actorSchemas[type] = this._describeSchema(model.schema);
        }
      }
    }

    if (CONFIG.Item.dataModels) {
      capabilities.itemSchemas = {};
      for (const type of capabilities.itemTypes) {
        const model = CONFIG.Item.dataModels[type];
        if (model?.schema) {
          capabilities.itemSchemas[type] = this._describeSchema(model.schema);
        }
      }
    }

    // Add mgt2e enhancements if applicable
    if (game.system.id === "mgt2e") {
      capabilities.mgt2eEnhancements = MGT2E_ENHANCEMENTS;
    }

    return capabilities;
  }

  /**
   * Get document collection
   * @private
   */
  static _getCollection(documentType) {
    const collections = {
      actor: game.actors,
      item: game.items,
      journal: game.journal,
      scene: game.scenes,
      rolltable: game.tables,
      macro: game.macros,
      playlist: game.playlists,
    };
    return collections[documentType.toLowerCase()];
  }

  /**
   * Get document class
   * @private
   */
  static _getDocumentClass(documentType) {
    const classes = {
      actor: Actor,
      item: Item,
      journal: JournalEntry,
      scene: Scene,
      rolltable: RollTable,
      macro: Macro,
      playlist: Playlist,
    };
    return classes[documentType.toLowerCase()];
  }

  /**
   * Check if document matches filters
   * @private
   */
  static _matchesFilters(doc, filters) {
    if (!filters) return true;
    for (const [key, value] of Object.entries(filters)) {
      const docValue = foundry.utils.getProperty(doc, key);
      if (docValue !== value) return false;
    }
    return true;
  }

  /**
   * Describe a data schema
   * @private
   */
  static _describeSchema(schema) {
    if (!schema) return null;
    const description = {};
    for (const [key, field] of Object.entries(schema.fields || {})) {
      description[key] = {
        type: field.constructor.name,
        required: field.required,
        initial: field.initial,
      };
    }
    return description;
  }
}

// MGT2E specific enhancements
const MGT2E_ENHANCEMENTS = {
  actorTypes: {
    traveller: {
      description: "Player character or NPC with full characteristics",
      characteristics: ["strength", "dexterity", "endurance", "intellect", "education", "social"],
      skillSystem: "Skills are embedded Items with value and optional speciality",
    },
    npc: {
      description: "Simplified NPC without full career history",
    },
    creature: {
      description: "Animal or alien creature with instinct-based behavior",
    },
    spacecraft: {
      description: "Starship with tonnage, jump rating, and crew positions",
    },
    vehicle: {
      description: "Ground or air vehicle",
    },
    world: {
      description: "Planet with UWP (Universal World Profile) data",
    },
  },
  itemTypes: {
    weapon: { key_fields: ["damage", "range", "traits", "tl"] },
    armour: { key_fields: ["protection", "tl", "radiation"] },
    skill: { key_fields: ["value", "speciality"] },
    term: { key_fields: ["career", "assignment", "rank"] },
  },
  uwpFormat: "Starport-Size-Atmo-Hydro-Pop-Gov-Law-TL (e.g., A867949-C)",
};

// ============================================================================
// Tool Executor
// ============================================================================

/**
 * Executes FVTT tools requested by the backend
 */
class ToolExecutor {
  /**
   * Execute a tool
   * @param {string} tool - Tool name
   * @param {Object} args - Tool arguments
   * @param {Object} userContext - User context
   * @returns {Promise<Object>}
   */
  static async execute(tool, args, userContext) {
    switch (tool) {
      case "fvtt_read":
        return FvttApiWrapper.read(args.document_type, args.document_id, userContext);

      case "fvtt_write":
        return FvttApiWrapper.write(args.document_type, args.operation, args.data, userContext);

      case "fvtt_query":
        return FvttApiWrapper.query(args.document_type, args.filters, userContext);

      case "dice_roll":
        return FvttApiWrapper.rollDice(args.formula, args.label, userContext);

      case "system_schema":
        return FvttApiWrapper.getSystemCapabilities();

      default:
        return { error: `Unknown tool: ${tool}` };
    }
  }
}

// ============================================================================
// Document Management Dialog
// ============================================================================

/**
 * Dialog for managing documents in the Seneschal backend
 */
class DocumentManagementDialog extends Application {
  static get defaultOptions() {
    return foundry.utils.mergeObject(super.defaultOptions, {
      id: "seneschal-documents",
      title: game.i18n.localize("SENESCHAL.Documents.Title"),
      template: `modules/${MODULE_ID}/templates/documents.hbs`,
      width: 600,
      height: 650,
      resizable: true,
      classes: ["seneschal", "seneschal-documents-app"],
    });
  }

  constructor(options = {}) {
    super(options);
    this.backendClient = new BackendClient();
    this.documents = [];
    this.isLoading = false;
    this.error = null;
    this.uploadProgress = null;
    this.processingDoc = null; // Document ID currently being re-processed (images)
    this._pollTimer = null; // Timer for auto-refresh when documents are processing
  }

  /**
   * Get template data
   */
  getData() {
    // Enhance documents with isPdf flag
    const documentsEnhanced = this.documents.map((doc) => ({
      ...doc,
      isPdf: doc.file_path?.toLowerCase().endsWith(".pdf"),
    }));

    return {
      documents: documentsEnhanced,
      isLoading: this.isLoading,
      error: this.error,
      uploadProgress: this.uploadProgress,
      processingDoc: this.processingDoc,
    };
  }

  /**
   * Called when the application is rendered
   */
  async _render(force = false, options = {}) {
    await super._render(force, options);
    if (force) {
      this._loadDocuments();
    }
  }

  /**
   * Activate listeners
   */
  activateListeners(html) {
    super.activateListeners(html);

    // Upload form
    html.find(".seneschal-upload-form").on("submit", this._onUpload.bind(this));

    // Delete document buttons
    html.find(".seneschal-delete-doc").click(this._onDelete.bind(this));

    // Delete images buttons
    html.find(".seneschal-delete-images").click(this._onDeleteImages.bind(this));

    // Re-extract images buttons
    html.find(".seneschal-reextract-images").click(this._onReextractImages.bind(this));
  }

  /**
   * Load document list from backend
   */
  async _loadDocuments() {
    if (!this.backendClient.isConfigured()) {
      this.error = game.i18n.localize("SENESCHAL.Notifications.NotConfigured");
      this.render(false);
      return;
    }

    this.isLoading = true;
    this.error = null;
    this.render(false);

    try {
      this.documents = await this.backendClient.listDocuments();
      this.isLoading = false;
      this.render(false);

      // If any documents are still processing, start auto-refresh
      this._startProcessingPoll();
    } catch (error) {
      console.error("Failed to load documents:", error);
      this.isLoading = false;
      this.error = error.message;
      this.render(false);
    }
  }

  /**
   * Start polling for document processing status updates
   */
  _startProcessingPoll() {
    // Clear any existing timer
    if (this._pollTimer) {
      clearTimeout(this._pollTimer);
      this._pollTimer = null;
    }

    // Check if any documents are processing
    const hasProcessing = this.documents.some((doc) => doc.processing_status === "processing");
    if (!hasProcessing) return;

    // Poll every 5 seconds
    this._pollTimer = setTimeout(async () => {
      try {
        this.documents = await this.backendClient.listDocuments();
        this.render(false);
        this._startProcessingPoll(); // Continue polling if still processing
      } catch (error) {
        console.error("Failed to refresh documents:", error);
      }
    }, 5000);
  }

  /**
   * Stop polling when dialog closes
   */
  close(options) {
    if (this._pollTimer) {
      clearTimeout(this._pollTimer);
      this._pollTimer = null;
    }
    return super.close(options);
  }

  /**
   * Handle document upload
   */
  async _onUpload(event) {
    event.preventDefault();

    const form = event.currentTarget;
    const fileInput = form.querySelector('input[name="file"]');
    const file = fileInput.files[0];

    if (!file) {
      ui.notifications.warn("Please select a file to upload.");
      return;
    }

    const title = form.querySelector('input[name="title"]').value.trim();
    if (!title) {
      ui.notifications.warn("Please enter a document title.");
      return;
    }

    const accessLevel = form.querySelector('select[name="access_level"]').value;
    const tags = form.querySelector('input[name="tags"]').value.trim();

    this.uploadProgress = 0;
    this.render(false);

    try {
      // Get vision model from settings
      const visionModel = getSetting(SETTINGS.VISION_MODEL);

      await this.backendClient.uploadDocument(
        file,
        {
          title,
          accessLevel,
          tags: tags || undefined,
          visionModel: visionModel || undefined,
        },
        (progress) => {
          this.uploadProgress = progress;
          this.render(false);
        }
      );

      ui.notifications.info(game.i18n.localize("SENESCHAL.Documents.UploadSuccess"));
      this.uploadProgress = null;

      // Clear form
      form.reset();

      // Reload document list
      await this._loadDocuments();
    } catch (error) {
      console.error("Upload failed:", error);
      ui.notifications.error(
        `${game.i18n.localize("SENESCHAL.Documents.UploadError")}: ${error.message}`,
        { permanent: true }
      );
      this.uploadProgress = null;
      this.render(false);
    }
  }

  /**
   * Handle document deletion
   */
  async _onDelete(event) {
    event.preventDefault();

    const row = event.currentTarget.closest("tr");
    const documentId = row.dataset.documentId;
    const title = row.querySelector(".document-title").textContent;

    const confirmed = await Dialog.confirm({
      title: game.i18n.localize("SENESCHAL.Documents.Delete"),
      content: `<p>${game.i18n.localize("SENESCHAL.Documents.DeleteConfirm")}</p><p><strong>${title}</strong></p>`,
      yes: () => true,
      no: () => false,
    });

    if (!confirmed) return;

    try {
      await this.backendClient.deleteDocument(documentId);
      ui.notifications.info(game.i18n.localize("SENESCHAL.Documents.DeleteSuccess"));
      await this._loadDocuments();
    } catch (error) {
      console.error("Delete failed:", error);
      ui.notifications.error(
        `${game.i18n.localize("SENESCHAL.Documents.DeleteError")}: ${error.message}`,
        { permanent: true }
      );
    }
  }

  /**
   * Handle deleting images for a document
   */
  async _onDeleteImages(event) {
    event.preventDefault();

    const row = event.currentTarget.closest("tr");
    const documentId = row.dataset.documentId;
    const title = row.querySelector(".document-title").textContent.trim();
    const doc = this.documents.find((d) => d.id === documentId);
    const imageCount = doc?.image_count || 0;

    const confirmed = await Dialog.confirm({
      title: game.i18n.localize("SENESCHAL.Documents.DeleteImages"),
      content: `<p>${game.i18n.localize("SENESCHAL.Documents.DeleteImagesConfirm")}</p><p><strong>${title}</strong> (${imageCount} images)</p>`,
      yes: () => true,
      no: () => false,
    });

    if (!confirmed) return;

    try {
      const result = await this.backendClient.deleteDocumentImages(documentId);
      ui.notifications.info(
        `${game.i18n.localize("SENESCHAL.Documents.DeleteImagesSuccess")} (${result.deleted_count} images)`
      );
      // Reload documents to get updated counts
      await this._loadDocuments();
    } catch (error) {
      console.error("Delete images failed:", error);
      ui.notifications.error(
        `${game.i18n.localize("SENESCHAL.Documents.DeleteImagesError")}: ${error.message}`,
        { permanent: true }
      );
    }
  }

  /**
   * Handle re-extracting images from a document
   */
  async _onReextractImages(event) {
    event.preventDefault();

    const row = event.currentTarget.closest("tr");
    const documentId = row.dataset.documentId;
    const title = row.querySelector(".document-title").textContent;

    const confirmed = await Dialog.confirm({
      title: game.i18n.localize("SENESCHAL.Documents.ReextractImages"),
      content: `<p>${game.i18n.localize("SENESCHAL.Documents.ReextractImagesConfirm")}</p><p><strong>${title}</strong></p><p><em>${game.i18n.localize("SENESCHAL.Documents.ReextractImagesNote")}</em></p>`,
      yes: () => true,
      no: () => false,
    });

    if (!confirmed) return;

    this.processingDoc = documentId;
    this.render(false);

    try {
      // Get vision model from settings
      const visionModel = getSetting(SETTINGS.VISION_MODEL);
      const result = await this.backendClient.reextractDocumentImages(
        documentId,
        visionModel || null
      );
      ui.notifications.info(
        `${game.i18n.localize("SENESCHAL.Documents.ReextractImagesSuccess")} (${result.extracted_count} images)`
      );
      // Reload documents to get updated counts
      await this._loadDocuments();
    } catch (error) {
      console.error("Re-extract images failed:", error);
      ui.notifications.error(
        `${game.i18n.localize("SENESCHAL.Documents.ReextractImagesError")}: ${error.message}`,
        { permanent: true }
      );
    } finally {
      this.processingDoc = null;
      this.render(false);
    }
  }
}

// ============================================================================
// Seneschal Sidebar Tab
// ============================================================================

/**
 * Seneschal sidebar tab - main interface in right sidebar
 * Note: We don't extend SidebarTab because it's not available as a global in Foundry VTT.
 * Instead, we create and manage DOM elements directly.
 */
class SeneschalSidebarTab {
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
  async _onSendMessage() {
    if (this.isProcessing) return;

    const textarea = this._element.find(".seneschal-input");
    const content = textarea.val()?.trim();
    if (!content) return;

    // Check configuration
    if (!this.backendClient.isConfigured()) {
      ui.notifications.error(game.i18n.localize("SENESCHAL.Notifications.NotConfigured"));
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

    const userContext = buildUserContext();

    try {
      await this.backendClient.streamChat({
        messages: this.session.getMessagesForContext(),
        userContext,
        conversationId: this.session.id,
        tools: [
          "document_search",
          "fvtt_read",
          "fvtt_write",
          "fvtt_query",
          "dice_roll",
          "system_schema",
        ],
        onChunk: (text) => this._onChunk(text),
        onToolCall: (id, tool, args) => this._onToolCall(id, tool, args, userContext),
        onToolStatus: (message) => this._onToolStatus(message),
        onPause: (reason, toolCalls, elapsed, message) =>
          this._onPause(reason, toolCalls, elapsed, message),
        onComplete: (fullResponse, toolCalls) => this._onComplete(fullResponse, toolCalls),
        onError: (error) => this._onError(error),
      });
    } catch (error) {
      this._onError(error);
    }
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
  async _onToolCall(id, tool, args, userContext) {
    this.toolStatus = game.i18n.localize(`SENESCHAL.ToolStatus.Processing`);
    this.render();

    // Execute the tool
    const result = await ToolExecutor.execute(tool, args, userContext);

    // Send result back to backend and process the continuation stream
    // The continuation uses the same event handlers as the original request
    await this.backendClient.sendToolResult(this.session.id, id, result, {
      onChunk: (text) => this._onChunk(text),
      onToolCall: (nextId, nextTool, nextArgs) =>
        this._onToolCall(nextId, nextTool, nextArgs, userContext),
      onToolStatus: (message) => this._onToolStatus(message),
      onPause: (reason, toolCalls, elapsed, message) =>
        this._onPause(reason, toolCalls, elapsed, message),
      onComplete: (fullResponse, toolCalls) => this._onComplete(fullResponse, toolCalls),
      onError: (error) => this._onError(error),
    });
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
  async _onContinue(action) {
    this.isPaused = false;
    this.pauseMessage = null;

    if (action === "continue") {
      this.isProcessing = true;
      this.isThinking = true;
      this.render();

      await this.backendClient.continueChat(this.session.id, action);
    } else {
      this.isProcessing = false;
      this.render();
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

// ============================================================================
// One-Shot Chat Command
// ============================================================================

/**
 * Handle one-shot AI query from chat
 */
async function handleOneShotQuery(query, _chatData) {
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
// Module Registration
// ============================================================================

/**
 * Model Selection Dialog
 */
class ModelSelectionDialog extends FormApplication {
  static get defaultOptions() {
    return foundry.utils.mergeObject(super.defaultOptions, {
      id: "seneschal-model-selection",
      title: game.i18n.localize("SENESCHAL.Settings.ModelSelection.Title"),
      template: `modules/${MODULE_ID}/templates/model-selection.hbs`,
      width: 500,
      height: "auto",
      closeOnSubmit: true,
    });
  }

  constructor(options = {}) {
    super({}, options);
    this.models = [];
    this.isLoading = false;
    this.error = null;
  }

  async getData() {
    return {
      models: this.models,
      selectedModel: getSetting(SETTINGS.SELECTED_MODEL),
      visionModel: getSetting(SETTINGS.VISION_MODEL),
      isLoading: this.isLoading,
      error: this.error,
    };
  }

  async _render(force = false, options = {}) {
    await super._render(force, options);
    if (force && this.models.length === 0) {
      await this._loadModels();
    }
  }

  async _loadModels() {
    const backendUrl = getSetting(SETTINGS.BACKEND_URL);
    if (!backendUrl) {
      this.error = game.i18n.localize("SENESCHAL.Notifications.NotConfigured");
      this.render(false);
      return;
    }

    // Check for mixed content issue (HTTPS page loading HTTP resource)
    const pageProtocol = window.location.protocol;
    const backendProtocol = new URL(backendUrl).protocol;
    if (pageProtocol === "https:" && backendProtocol === "http:") {
      this.error = game.i18n.localize("SENESCHAL.Notifications.MixedContent");
      this.render(false);
      return;
    }

    this.isLoading = true;
    this.error = null;
    this.render(false);

    try {
      const client = new BackendClient();
      this.models = await client.getModels();
      this.isLoading = false;
      this.render(false);
    } catch (error) {
      console.error("Failed to load models:", error);
      this.isLoading = false;
      this.error = error.message;
      this.render(false);
    }
  }

  activateListeners(html) {
    super.activateListeners(html);
    html.find(".seneschal-refresh-models").click(() => this._loadModels());
  }

  async _updateObject(_event, formData) {
    const selectedModel = formData.selectedModel;
    const visionModel = formData.visionModel;
    await game.settings.set(MODULE_ID, SETTINGS.SELECTED_MODEL, selectedModel);
    await game.settings.set(MODULE_ID, SETTINGS.VISION_MODEL, visionModel);
    ui.notifications.info(game.i18n.localize("SENESCHAL.Settings.ModelSelection.Saved"));
  }
}

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

  game.settings.register(MODULE_ID, SETTINGS.SELECTED_MODEL, {
    name: game.i18n.localize("SENESCHAL.Settings.SelectedModel"),
    hint: game.i18n.localize("SENESCHAL.Settings.SelectedModelHint"),
    scope: "world",
    config: false, // Not shown in main config, accessed via menu
    type: String,
    default: "",
  });

  game.settings.register(MODULE_ID, SETTINGS.VISION_MODEL, {
    name: game.i18n.localize("SENESCHAL.Settings.VisionModel"),
    hint: game.i18n.localize("SENESCHAL.Settings.VisionModelHint"),
    scope: "world",
    config: false, // Not shown in main config, accessed via menu
    type: String,
    default: "",
  });

  // Register settings menu for model selection
  game.settings.registerMenu(MODULE_ID, "modelSelection", {
    name: game.i18n.localize("SENESCHAL.Settings.ModelSelection.Name"),
    label: game.i18n.localize("SENESCHAL.Settings.ModelSelection.Label"),
    hint: game.i18n.localize("SENESCHAL.Settings.ModelSelection.Hint"),
    icon: "fas fa-robot",
    type: ModelSelectionDialog,
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

Hooks.once("ready", () => {
  console.log(`${MODULE_ID} | Seneschal ready`);

  // Check if backend is configured
  const backendUrl = getSetting(SETTINGS.BACKEND_URL);
  if (!backendUrl && game.user.isGM) {
    ui.notifications.warn(game.i18n.localize("SENESCHAL.Notifications.NotConfigured"));
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

Hooks.on("chatMessage", (chatLog, message, chatData) => {
  const prefix = getSetting(SETTINGS.CHAT_COMMAND_PREFIX);
  if (!message.startsWith(prefix + " ")) return true;

  if (!canUseModule()) {
    ui.notifications.warn(game.i18n.localize("SENESCHAL.Notifications.PlayerAccessDisabled"));
    return false;
  }

  const query = message.slice(prefix.length + 1);
  handleOneShotQuery(query, chatData);
  return false;
});

// Export for advanced usage
export {
  SeneschalSidebarTab,
  DocumentManagementDialog,
  ModelSelectionDialog,
  BackendClient,
  ConversationSession,
  FvttApiWrapper,
  ToolExecutor,
  buildUserContext,
  canUseModule,
  saveImageToFVTT,
};
