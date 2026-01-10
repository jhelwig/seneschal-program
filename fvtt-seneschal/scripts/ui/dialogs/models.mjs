/**
 * Model selection dialog
 */

import { MODULE_ID, SETTINGS } from "../../constants.mjs";
import { getSetting } from "../../utils.mjs";
import { BackendClient } from "../../clients/backend.mjs";

/**
 * Model Selection Dialog
 */
export class ModelSelectionDialog extends FormApplication {
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
