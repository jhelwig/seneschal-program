/**
 * WebSocket client for real-time updates from the backend
 */

import { MODULE_ID, SETTINGS } from "../constants.mjs";
import { getSetting, buildUserContext } from "../utils.mjs";
import { ToolExecutor } from "../tools/index.mjs";

/**
 * WebSocket client for real-time updates from the backend
 * Handles document processing status and other live updates
 */
export class WebSocketClient {
  constructor() {
    this.socket = null;
    this.sessionId = null;
    this.reconnectAttempts = 0;
    this.maxReconnectAttempts = 10;
    this.reconnectDelay = 1000;
    this.listeners = new Map(); // event type -> Set of callbacks
    this.authenticated = false;
    this.pingInterval = null;
    this.connectionPromise = null;
    this.chatHandlers = new Map(); // conversation_id -> handlers object
  }

  /**
   * Get the WebSocket URL from the backend URL
   * @returns {string|null}
   */
  get wsUrl() {
    const httpUrl = getSetting(SETTINGS.BACKEND_URL);
    if (!httpUrl) return null;
    return httpUrl.replace(/^http/, "ws") + "/ws";
  }

  /**
   * Connect to the WebSocket server
   * @returns {Promise<void>}
   */
  async connect() {
    if (this.socket?.readyState === WebSocket.OPEN) return;
    if (this.connectionPromise) return this.connectionPromise;

    this.connectionPromise = new Promise((resolve, reject) => {
      const wsUrl = this.wsUrl;
      if (!wsUrl) {
        reject(new Error("Backend URL not configured"));
        this.connectionPromise = null;
        return;
      }

      console.log(`${MODULE_ID} | Connecting to WebSocket at ${wsUrl}`);
      this.socket = new WebSocket(wsUrl);

      this.socket.onopen = () => {
        console.log(`${MODULE_ID} | WebSocket connected`);
        this.reconnectAttempts = 0;
        this.reconnectDelay = 1000;
        this._authenticate();
        this.connectionPromise = null;
        resolve();
      };

      this.socket.onmessage = (event) => {
        try {
          const msg = JSON.parse(event.data);
          this._handleMessage(msg);
        } catch (e) {
          console.error(`${MODULE_ID} | Failed to parse WebSocket message:`, e);
        }
      };

      this.socket.onclose = (event) => {
        console.log(`${MODULE_ID} | WebSocket closed:`, event.code, event.reason);
        this.authenticated = false;
        this._clearPingInterval();
        this.connectionPromise = null;
        this._scheduleReconnect();
      };

      this.socket.onerror = (error) => {
        console.error(`${MODULE_ID} | WebSocket error:`, error);
        this.connectionPromise = null;
        reject(error);
      };
    });

    return this.connectionPromise;
  }

  /**
   * Send authentication message with user context
   * @private
   */
  _authenticate() {
    const ctx = buildUserContext();
    this.send({
      type: "auth",
      user_id: ctx.user_id,
      user_name: ctx.user_name,
      role: ctx.role,
      session_id: this.sessionId,
    });
  }

  /**
   * Handle incoming WebSocket message
   * @param {Object} msg - Parsed message
   * @private
   */
  _handleMessage(msg) {
    switch (msg.type) {
      case "auth_response":
        this.authenticated = msg.success;
        this.sessionId = msg.session_id;
        if (msg.success) {
          this._startPingInterval();
          this._emit("connected", {});
          console.log(`${MODULE_ID} | WebSocket authenticated, session: ${msg.session_id}`);
        } else {
          console.error(`${MODULE_ID} | WebSocket authentication failed:`, msg.message);
        }
        break;
      case "document_progress":
        this._emit("document_progress", msg);
        break;
      case "pong":
        // Keepalive acknowledged
        break;
      case "error":
        console.error(`${MODULE_ID} | WebSocket server error:`, msg);
        this._emit("error", msg);
        break;

      // Chat message types
      case "chat_started": {
        const handlers = this.chatHandlers.get(msg.conversation_id);
        if (handlers?.onStarted) handlers.onStarted(msg.conversation_id);
        break;
      }
      case "chat_content": {
        const handlers = this.chatHandlers.get(msg.conversation_id);
        if (handlers?.onChunk) handlers.onChunk(msg.text);
        break;
      }
      case "chat_tool_call": {
        // Always execute tool calls and send results back
        // Handlers are notified for UI updates but don't handle execution
        const handlers = this.chatHandlers.get(msg.conversation_id);
        this._executeToolCall(msg.conversation_id, msg.id, msg.tool, msg.args, handlers);
        break;
      }
      case "chat_tool_status": {
        const handlers = this.chatHandlers.get(msg.conversation_id);
        if (handlers?.onToolStatus) handlers.onToolStatus(msg.message);
        break;
      }
      case "chat_tool_result": {
        const handlers = this.chatHandlers.get(msg.conversation_id);
        if (handlers?.onToolStatus) {
          handlers.onToolStatus(`${msg.tool}: ${msg.summary}`);
        }
        break;
      }
      case "chat_paused": {
        const handlers = this.chatHandlers.get(msg.conversation_id);
        if (handlers?.onPause) {
          handlers.onPause(msg.reason, msg.tool_calls_made, msg.elapsed_seconds, msg.message);
        }
        break;
      }
      case "chat_turn_complete": {
        const handlers = this.chatHandlers.get(msg.conversation_id);
        if (handlers?.onComplete) {
          handlers.onComplete({
            prompt_tokens: msg.prompt_tokens,
            completion_tokens: msg.completion_tokens,
          });
        }
        // Clean up handlers after completion
        this.chatHandlers.delete(msg.conversation_id);
        break;
      }
      case "chat_error": {
        const handlers = this.chatHandlers.get(msg.conversation_id);
        if (handlers?.onError) {
          const error = new Error(msg.message);
          error.recoverable = msg.recoverable ?? false;
          handlers.onError(error);
        }
        // Clean up handlers if not recoverable
        if (!msg.recoverable) {
          this.chatHandlers.delete(msg.conversation_id);
        }
        break;
      }

      default:
        console.warn(`${MODULE_ID} | Unknown WebSocket message type:`, msg.type);
    }
  }

  /**
   * Send a message to the server
   * @param {Object} msg - Message to send
   */
  send(msg) {
    if (this.socket?.readyState === WebSocket.OPEN) {
      this.socket.send(JSON.stringify(msg));
    }
  }

  /**
   * Subscribe to document processing updates
   */
  subscribeToDocuments() {
    this.send({ type: "subscribe_documents" });
  }

  /**
   * Unsubscribe from document processing updates
   */
  unsubscribeFromDocuments() {
    this.send({ type: "unsubscribe_documents" });
  }

  /**
   * Register handlers for a chat conversation
   * @param {string} conversationId - Conversation ID
   * @param {Object} handlers - Event handlers
   * @param {Function} [handlers.onStarted] - Called when chat starts
   * @param {Function} [handlers.onChunk] - Called with each text chunk
   * @param {Function} [handlers.onToolCall] - Called when tool execution starts (for UI updates); receives (tool)
   * @param {Function} [handlers.onToolStatus] - Called with tool status updates
   * @param {Function} [handlers.onPause] - Called when loop pauses
   * @param {Function} [handlers.onComplete] - Called when done
   * @param {Function} [handlers.onError] - Called on error
   */
  registerChatHandlers(conversationId, handlers) {
    this.chatHandlers.set(conversationId, handlers);
  }

  /**
   * Unregister handlers for a chat conversation
   * @param {string} conversationId - Conversation ID
   */
  unregisterChatHandlers(conversationId) {
    this.chatHandlers.delete(conversationId);
  }

  /**
   * Start a chat session via WebSocket
   * @param {Object} options - Chat options
   * @param {string|null} options.conversationId - Existing conversation ID or null for new
   * @param {string} options.message - User message
   * @param {string|null} options.model - Model to use
   * @param {Array<string>|null} options.enabledTools - Tools to enable
   */
  startChat(options) {
    this.send({
      type: "chat_message",
      conversation_id: options.conversationId,
      message: options.message,
      model: options.model,
      enabled_tools: options.enabledTools,
    });
  }

  /**
   * Send a tool result via WebSocket
   * @param {string} conversationId - Conversation ID
   * @param {string} toolCallId - Tool call ID
   * @param {*} result - Tool result
   */
  sendToolResult(conversationId, toolCallId, result) {
    this.send({
      type: "tool_result",
      conversation_id: conversationId,
      tool_call_id: toolCallId,
      result,
    });
  }

  /**
   * Continue a paused chat
   * @param {string} conversationId - Conversation ID
   */
  continueChat(conversationId) {
    this.send({
      type: "continue_chat",
      conversation_id: conversationId,
    });
  }

  /**
   * Cancel an active chat
   * @param {string} conversationId - Conversation ID
   */
  cancelChat(conversationId) {
    this.send({
      type: "cancel_chat",
      conversation_id: conversationId,
    });
    this.chatHandlers.delete(conversationId);
  }

  /**
   * Add an event listener
   * @param {string} event - Event type
   * @param {Function} callback - Callback function
   * @returns {Function} Unsubscribe function
   */
  on(event, callback) {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set());
    }
    this.listeners.get(event).add(callback);
    return () => this.off(event, callback);
  }

  /**
   * Remove an event listener
   * @param {string} event - Event type
   * @param {Function} callback - Callback function
   */
  off(event, callback) {
    this.listeners.get(event)?.delete(callback);
  }

  /**
   * Emit an event to all listeners
   * @param {string} event - Event type
   * @param {*} data - Event data
   * @private
   */
  _emit(event, data) {
    this.listeners.get(event)?.forEach((cb) => {
      try {
        cb(data);
      } catch (e) {
        console.error(`${MODULE_ID} | Error in WebSocket event handler:`, e);
      }
    });
  }

  /**
   * Execute a tool call and send the result back to the server
   * @param {string} conversationId - Conversation or MCP request ID
   * @param {string} toolCallId - Tool call ID
   * @param {string} tool - Tool name
   * @param {Object} args - Tool arguments
   * @param {Object} [handlers] - Optional handlers for UI notifications
   * @private
   */
  async _executeToolCall(conversationId, toolCallId, tool, args, handlers) {
    console.log(`${MODULE_ID} | Executing tool call: ${tool}`, args);

    // Notify handler that tool execution is starting (for UI updates)
    if (handlers?.onToolCall) {
      handlers.onToolCall(tool);
    }

    try {
      // Build user context from current FVTT user
      const userContext = buildUserContext();

      // Execute the tool
      const result = await ToolExecutor.execute(tool, args, userContext);

      // Send result back to server
      this.sendToolResult(conversationId, toolCallId, result);

      console.log(`${MODULE_ID} | Tool call completed: ${tool}`);
    } catch (error) {
      console.error(`${MODULE_ID} | Tool call failed: ${tool}`, error);

      // Send error result back to server
      this.sendToolResult(conversationId, toolCallId, {
        error: true,
        message: error.message || "Tool execution failed",
      });
    }
  }

  /**
   * Start the ping interval for keepalive
   * @private
   */
  _startPingInterval() {
    this._clearPingInterval();
    this.pingInterval = setInterval(() => {
      this.send({ type: "ping" });
    }, 30000);
  }

  /**
   * Clear the ping interval
   * @private
   */
  _clearPingInterval() {
    if (this.pingInterval) {
      clearInterval(this.pingInterval);
      this.pingInterval = null;
    }
  }

  /**
   * Schedule a reconnection attempt with exponential backoff
   * @private
   */
  _scheduleReconnect() {
    if (this.reconnectAttempts >= this.maxReconnectAttempts) {
      console.error(`${MODULE_ID} | Max WebSocket reconnection attempts reached`);
      this._emit("disconnected", { permanent: true });
      return;
    }

    this.reconnectAttempts++;
    const delay = Math.min(this.reconnectDelay * Math.pow(2, this.reconnectAttempts - 1), 30000);

    console.log(
      `${MODULE_ID} | Scheduling WebSocket reconnect in ${delay}ms (attempt ${this.reconnectAttempts})`
    );

    setTimeout(() => {
      this.connect().catch(() => {});
    }, delay);
  }

  /**
   * Close the WebSocket connection
   */
  close() {
    this._clearPingInterval();
    if (this.socket) {
      this.socket.close(1000, "Client closing");
      this.socket = null;
    }
    this.authenticated = false;
  }
}
