/**
 * Image browser dialog
 */

import { MODULE_ID } from "../../constants.mjs";
import { saveImageToFVTT } from "../../utils.mjs";
import { BackendClient } from "../../clients/backend.mjs";

/**
 * Dialog for browsing and managing images from a document
 */
export class ImageBrowserDialog extends Application {
  static get defaultOptions() {
    return foundry.utils.mergeObject(super.defaultOptions, {
      id: "seneschal-images",
      title: game.i18n.localize("SENESCHAL.Images.Title"),
      template: `modules/${MODULE_ID}/templates/images.hbs`,
      width: 800,
      height: 600,
      resizable: true,
      classes: ["seneschal", "seneschal-images-app"],
    });
  }

  constructor(documentId, documentTitle, options = {}) {
    super(options);
    this.documentId = documentId;
    this.documentTitle = documentTitle;
    this.backendClient = new BackendClient();
    this.images = [];
    this.isLoading = false;
    this.error = null;
    this.copyingImage = null;
  }

  /**
   * Get template data
   */
  getData() {
    return {
      documentId: this.documentId,
      documentTitle: this.documentTitle,
      images: this.images,
      isLoading: this.isLoading,
      error: this.error,
      copyingImage: this.copyingImage,
      backendUrl: this.backendClient.baseUrl,
    };
  }

  /**
   * Called when the application is rendered
   */
  async _render(force = false, options = {}) {
    await super._render(force, options);
    if (force) {
      this._loadImages();
    }
  }

  /**
   * Activate listeners
   */
  activateListeners(html) {
    super.activateListeners(html);

    // View full image
    html.find(".seneschal-image-item img").click(this._onViewImage.bind(this));

    // Copy to FVTT
    html.find(".seneschal-image-copy").click(this._onCopyImage.bind(this));

    // Delete image
    html.find(".seneschal-image-delete").click(this._onDeleteImage.bind(this));
  }

  /**
   * Load images from backend
   */
  async _loadImages() {
    if (!this.backendClient.isConfigured()) {
      this.error = game.i18n.localize("SENESCHAL.Notifications.NotConfigured");
      this.render(false);
      return;
    }

    this.isLoading = true;
    this.error = null;
    this.render(false);

    try {
      const response = await this.backendClient.getDocumentImages(this.documentId);
      this.images = response.images || [];
      this.isLoading = false;
      this.render(false);
    } catch (error) {
      console.error("Failed to load images:", error);
      this.isLoading = false;
      this.error = error.message;
      this.render(false);
    }
  }

  /**
   * Handle viewing full image
   */
  _onViewImage(event) {
    event.preventDefault();
    event.stopPropagation();

    const card = event.currentTarget.closest(".seneschal-image-item");
    const imageId = card.dataset.imageId;
    const imgSrc = `${this.backendClient.baseUrl}/api/images/${imageId}/data`;

    // Create lightbox
    const lightbox = document.createElement("div");
    lightbox.className = "seneschal-lightbox";
    lightbox.innerHTML = `
      <button class="seneschal-lightbox-close"><i class="fas fa-times"></i></button>
      <img src="${imgSrc}" alt="Full size image" />
    `;

    // Close on click outside or escape
    lightbox.addEventListener("click", (e) => {
      if (e.target === lightbox || e.target.closest(".seneschal-lightbox-close")) {
        lightbox.remove();
      }
    });

    document.addEventListener(
      "keydown",
      (e) => {
        if (e.key === "Escape") {
          lightbox.remove();
        }
      },
      { once: true }
    );

    document.body.appendChild(lightbox);
  }

  /**
   * Handle copying image to FVTT assets
   */
  async _onCopyImage(event) {
    event.preventDefault();

    const card = event.currentTarget.closest(".seneschal-image-item");
    const imageId = card.dataset.imageId;
    const image = this.images.find((img) => img.id === imageId);
    if (!image) return;

    this.copyingImage = imageId;
    this.render(false);

    try {
      // Request delivery from backend
      const deliveryResult = await this.backendClient.deliverImage(imageId);

      if (deliveryResult.mode === "direct") {
        // Backend copied directly - show success
        ui.notifications.info(
          `${game.i18n.localize("SENESCHAL.Images.CopySuccess")}: ${deliveryResult.fvtt_path}`
        );
      } else {
        // Shuttle mode - we need to fetch and upload
        const targetPath = deliveryResult.suggested_path;
        const fvttPath = await saveImageToFVTT(imageId, targetPath, this.backendClient);
        ui.notifications.info(`${game.i18n.localize("SENESCHAL.Images.CopySuccess")}: ${fvttPath}`);
      }
    } catch (error) {
      console.error("Failed to copy image:", error);
      ui.notifications.error(
        `${game.i18n.localize("SENESCHAL.Images.CopyError")}: ${error.message}`
      );
    } finally {
      this.copyingImage = null;
      this.render(false);
    }
  }

  /**
   * Handle deleting an image
   */
  async _onDeleteImage(event) {
    event.preventDefault();

    const card = event.currentTarget.closest(".seneschal-image-item");
    const imageId = card.dataset.imageId;
    const image = this.images.find((img) => img.id === imageId);
    if (!image) return;

    const confirmed = await Dialog.confirm({
      title: game.i18n.localize("SENESCHAL.Images.Delete"),
      content: `<p>${game.i18n.localize("SENESCHAL.Images.DeleteConfirm")}</p><p><strong>${game.i18n.localize("SENESCHAL.Images.Page")} ${image.page_number}</strong></p>`,
      yes: () => true,
      no: () => false,
    });

    if (!confirmed) return;

    try {
      await this.backendClient.deleteImage(imageId);
      ui.notifications.info(game.i18n.localize("SENESCHAL.Images.DeleteSuccess"));

      // Remove from local array and re-render
      this.images = this.images.filter((img) => img.id !== imageId);
      this.render(false);
    } catch (error) {
      console.error("Failed to delete image:", error);
      ui.notifications.error(
        `${game.i18n.localize("SENESCHAL.Images.DeleteError")}: ${error.message}`
      );
    }
  }
}
