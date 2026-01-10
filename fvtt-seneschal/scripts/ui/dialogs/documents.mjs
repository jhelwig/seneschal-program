/**
 * Document management dialog
 */

import { MODULE_ID, SETTINGS } from "../../constants.mjs";
import { getSetting } from "../../utils.mjs";
import { BackendClient } from "../../clients/backend.mjs";
import { ImageBrowserDialog } from "./images.mjs";

/**
 * Dialog for managing documents in the Seneschal backend
 */
export class DocumentManagementDialog extends Application {
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
    this._wsUnsubscribe = null; // WebSocket event unsubscribe function
    this._imageBrowserDialogs = new Map(); // Track open image browser dialogs
  }

  /**
   * Get template data
   */
  getData() {
    // Map access level number to string
    const accessLevelToStr = (level) => {
      switch (level) {
        case 1:
          return "player";
        case 2:
          return "trusted";
        case 3:
          return "assistant";
        case 4:
        default:
          return "gm_only";
      }
    };

    // Enhance documents with isPdf flag and string representations
    const documentsEnhanced = this.documents.map((doc) => ({
      ...doc,
      isPdf: doc.file_path?.toLowerCase().endsWith(".pdf"),
      access_level_str: accessLevelToStr(doc.access_level),
      tags_str: Array.isArray(doc.tags) ? doc.tags.join(", ") : "",
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
      this._subscribeToUpdates();
    }
  }

  /**
   * Subscribe to WebSocket document updates
   * @private
   */
  _subscribeToUpdates() {
    // Unsubscribe from any previous subscription
    if (this._wsUnsubscribe) {
      this._wsUnsubscribe();
      this._wsUnsubscribe = null;
    }

    // Check if WebSocket is available and authenticated
    if (!globalThis.seneschalWS?.authenticated) {
      console.log(
        `${MODULE_ID} | WebSocket not available, document updates will require manual refresh`
      );
      return;
    }

    // Listen for document progress updates
    this._wsUnsubscribe = globalThis.seneschalWS.on("document_progress", (update) => {
      this._handleDocumentUpdate(update);
    });

    // Subscribe to documents channel
    globalThis.seneschalWS.subscribeToDocuments();
    console.log(`${MODULE_ID} | Subscribed to document updates via WebSocket`);
  }

  /**
   * Handle document progress update from WebSocket
   * @param {Object} update - Document progress update
   * @private
   */
  _handleDocumentUpdate(update) {
    const docIndex = this.documents.findIndex((d) => d.id === update.document_id);

    if (docIndex === -1) {
      // New document we don't know about - reload the list
      console.log(`${MODULE_ID} | Unknown document ${update.document_id}, reloading list`);
      this._loadDocuments();
      return;
    }

    // Update document in place
    const doc = this.documents[docIndex];
    doc.processing_status = update.status;
    doc.processing_phase = update.phase;
    doc.processing_progress = update.progress;
    doc.processing_total = update.total;
    doc.processing_error = update.error;
    doc.chunk_count = update.chunk_count;
    doc.image_count = update.image_count;

    // Update only the specific document row in the DOM (preserves form inputs)
    this._updateDocumentRowDOM(doc);
  }

  /**
   * Update the DOM for a specific document row without re-rendering the entire template.
   * This preserves form inputs while updating document status.
   * @param {Object} doc - The document object with updated properties
   * @private
   */
  _updateDocumentRowDOM(doc) {
    const row = this.element.find(`tr[data-document-id="${doc.id}"]`);
    if (!row.length) {
      // Row not found in DOM, fall back to full re-render
      this.render(false);
      return;
    }

    // Update row classes for processing/failed state
    row.removeClass("processing failed");
    if (doc.processing_status === "processing") {
      row.addClass("processing");
    } else if (doc.processing_status === "failed") {
      row.addClass("failed");
    }

    // Build status HTML based on processing state
    let statusHtml;
    if (doc.processing_status === "processing") {
      let phaseText;
      if (doc.processing_phase === "queued") {
        phaseText = game.i18n.localize("SENESCHAL.Documents.PhaseQueued");
      } else if (doc.processing_phase === "chunking") {
        phaseText = game.i18n.localize("SENESCHAL.Documents.PhaseChunking");
      } else if (doc.processing_phase === "embedding") {
        phaseText = `${game.i18n.localize("SENESCHAL.Documents.PhaseEmbedding")} (${doc.processing_progress}/${doc.processing_total})`;
      } else if (doc.processing_phase === "extracting_images") {
        phaseText = game.i18n.localize("SENESCHAL.Documents.PhaseExtractingImages");
      } else if (doc.processing_phase === "captioning") {
        phaseText = `${game.i18n.localize("SENESCHAL.Documents.PhaseCaptioning")} (${doc.processing_progress}/${doc.processing_total})`;
      } else if (doc.processing_phase) {
        phaseText = doc.processing_phase;
      } else {
        phaseText = game.i18n.localize("SENESCHAL.Documents.StatusProcessing");
      }
      statusHtml = `<i class="fas fa-spinner fa-spin"></i> ${phaseText}`;
    } else if (doc.processing_status === "failed") {
      statusHtml = `<i class="fas fa-times-circle"></i> ${game.i18n.localize("SENESCHAL.Documents.StatusFailed")}`;
    } else {
      statusHtml = `<i class="fas fa-check-circle"></i> ${game.i18n.localize("SENESCHAL.Documents.StatusCompleted")}`;
    }
    row.find(".document-status").html(statusHtml);

    // Update chunk count
    row.find(".document-chunks").text(doc.chunk_count ?? "");

    // Update image count (unless we're showing a reprocessing spinner)
    const imagesCell = row.find(".document-images");
    if (!imagesCell.find(".fa-spinner").length || this.processingDoc !== doc.id) {
      imagesCell.text(doc.image_count ?? "");
    }

    // Update error display in title cell
    const titleCell = row.find(".document-title");
    titleCell.find(".document-error").remove();
    if (doc.processing_error) {
      const escapedError = doc.processing_error
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;");
      titleCell.append(
        `<div class="document-error" title="${escapedError}"><i class="fas fa-exclamation-circle"></i></div>`
      );
    }
  }

  /**
   * Activate listeners
   */
  activateListeners(html) {
    super.activateListeners(html);

    // Upload form
    html.find(".seneschal-upload-form").on("submit", this._onUpload.bind(this));

    // Edit document buttons
    html.find(".seneschal-edit-doc").click(this._onEdit.bind(this));

    // Delete document buttons
    html.find(".seneschal-delete-doc").click(this._onDelete.bind(this));

    // Delete images buttons
    html.find(".seneschal-delete-images").click(this._onDeleteImages.bind(this));

    // Re-extract images buttons
    html.find(".seneschal-reextract-images").click(this._onReextractImages.bind(this));

    // Browse images buttons
    html.find(".seneschal-browse-images").click(this._onBrowseImages.bind(this));
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
    } catch (error) {
      console.error("Failed to load documents:", error);
      this.isLoading = false;
      this.error = error.message;
      this.render(false);
    }
  }

  /**
   * Cleanup when dialog closes
   */
  close(options) {
    // Unsubscribe from WebSocket updates
    if (this._wsUnsubscribe) {
      this._wsUnsubscribe();
      this._wsUnsubscribe = null;
    }
    globalThis.seneschalWS?.unsubscribeFromDocuments();

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
   * Handle document edit
   */
  async _onEdit(event) {
    event.preventDefault();

    const row = event.currentTarget.closest("tr");
    const documentId = row.dataset.documentId;
    const currentTitle = row.dataset.documentTitle;
    const currentAccess = row.dataset.documentAccess;
    const currentTags = row.dataset.documentTags;

    // Create the edit dialog content
    const content = `
      <form class="seneschal-edit-form">
        <div class="form-group">
          <label for="edit-title">${game.i18n.localize("SENESCHAL.Documents.DocumentTitle")}</label>
          <input type="text" id="edit-title" name="title" value="${currentTitle}" required />
        </div>
        <div class="form-group">
          <label for="edit-access">${game.i18n.localize("SENESCHAL.Documents.AccessLevel")}</label>
          <select id="edit-access" name="access_level">
            <option value="player" ${currentAccess === "player" ? "selected" : ""}>${game.i18n.localize("SENESCHAL.Documents.AccessPlayer")}</option>
            <option value="trusted" ${currentAccess === "trusted" ? "selected" : ""}>${game.i18n.localize("SENESCHAL.Documents.AccessTrusted")}</option>
            <option value="assistant" ${currentAccess === "assistant" ? "selected" : ""}>${game.i18n.localize("SENESCHAL.Documents.AccessAssistant")}</option>
            <option value="gm_only" ${currentAccess === "gm_only" ? "selected" : ""}>${game.i18n.localize("SENESCHAL.Documents.AccessGmOnly")}</option>
          </select>
        </div>
        <div class="form-group">
          <label for="edit-tags">${game.i18n.localize("SENESCHAL.Documents.Tags")}</label>
          <input type="text" id="edit-tags" name="tags" value="${currentTags}" placeholder="${game.i18n.localize("SENESCHAL.Documents.TagsPlaceholder")}" />
        </div>
      </form>
    `;

    const dialog = new Dialog({
      title: game.i18n.localize("SENESCHAL.Documents.Edit"),
      content,
      buttons: {
        save: {
          icon: '<i class="fas fa-save"></i>',
          label: game.i18n.localize("SENESCHAL.Documents.SaveChanges"),
          callback: async (html) => {
            const title = html.find("#edit-title").val().trim();
            const accessLevel = html.find("#edit-access").val();
            const tags = html.find("#edit-tags").val().trim();

            if (!title) {
              ui.notifications.error(game.i18n.localize("SENESCHAL.Documents.TitleRequired"));
              return;
            }

            try {
              await this.backendClient.updateDocument(documentId, {
                title,
                access_level: accessLevel,
                tags: tags || undefined,
              });
              ui.notifications.info(game.i18n.localize("SENESCHAL.Documents.EditSuccess"));
              await this._loadDocuments();
            } catch (error) {
              console.error("Edit failed:", error);
              ui.notifications.error(
                `${game.i18n.localize("SENESCHAL.Documents.EditError")}: ${error.message}`,
                { permanent: true }
              );
            }
          },
        },
        cancel: {
          icon: '<i class="fas fa-times"></i>',
          label: game.i18n.localize("SENESCHAL.Cancel"),
        },
      },
      default: "save",
    });

    dialog.render(true);
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
      await this.backendClient.reextractDocumentImages(documentId, visionModel || null);
      ui.notifications.info(game.i18n.localize("SENESCHAL.Documents.ReextractImagesQueued"));
      // Reload documents to show processing status, then start polling
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

  /**
   * Handle browsing images for a document
   */
  _onBrowseImages(event) {
    event.preventDefault();

    const row = event.currentTarget.closest("tr");
    const documentId = row.dataset.documentId;
    const title = row.querySelector(".document-title").textContent.trim();

    // Check if we already have a dialog open for this document
    let dialog = this._imageBrowserDialogs.get(documentId);
    if (dialog && dialog._state !== Application.RENDER_STATES.CLOSED) {
      dialog.bringToTop();
      return;
    }

    // Create a new dialog
    dialog = new ImageBrowserDialog(documentId, title);
    this._imageBrowserDialogs.set(documentId, dialog);
    dialog.render(true);
  }
}
