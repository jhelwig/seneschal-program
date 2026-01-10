/**
 * Conversation session management
 */

import { generateId } from "../utils.mjs";

/**
 * Manages a conversation session
 */
export class ConversationSession {
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
