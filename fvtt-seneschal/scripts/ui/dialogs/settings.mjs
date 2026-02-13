/**
 * Backend settings dialog for managing FVTT-controlled backend configuration
 */

import { MODULE_ID, SETTINGS } from "../../constants.mjs";
import { getSetting } from "../../utils.mjs";
import { BackendClient } from "../../clients/backend.mjs";

/**
 * Settings categories and their fields
 */
const SETTING_CATEGORIES = {
  models: {
    label: "SENESCHAL.Settings.Backend.Section.Models",
    fields: {
      "ollama.default_model": {
        type: "select",
        label: "SENESCHAL.Settings.Backend.Models.ChatModel",
        hint: "SENESCHAL.Settings.Backend.Models.ChatModelHint",
        options: "models",
      },
      "ollama.vision_model": {
        type: "select",
        label: "SENESCHAL.Settings.Backend.Models.VisionModel",
        hint: "SENESCHAL.Settings.Backend.Models.VisionModelHint",
        options: "models",
        allowEmpty: true,
      },
      "embeddings.model": {
        type: "select",
        label: "SENESCHAL.Settings.Backend.Models.EmbeddingModel",
        hint: "SENESCHAL.Settings.Backend.Models.EmbeddingModelHint",
        options: "models",
      },
    },
  },
  llm: {
    label: "SENESCHAL.Settings.Backend.Section.LLM",
    fields: {
      "ollama.base_url": {
        type: "text",
        label: "SENESCHAL.Settings.Backend.Ollama.BaseUrl",
        hint: "SENESCHAL.Settings.Backend.Ollama.BaseUrlHint",
      },
      "ollama.temperature": {
        type: "number",
        label: "SENESCHAL.Settings.Backend.Ollama.Temperature",
        hint: "SENESCHAL.Settings.Backend.Ollama.TemperatureHint",
        min: 0,
        max: 2,
        step: 0.1,
      },
      "ollama.request_timeout_secs": {
        type: "number",
        label: "SENESCHAL.Settings.Backend.Ollama.RequestTimeout",
        hint: "SENESCHAL.Settings.Backend.Ollama.RequestTimeoutHint",
        min: 10,
        max: 600,
        step: 1,
      },
    },
  },
  embeddings: {
    label: "SENESCHAL.Settings.Backend.Section.Embeddings",
    fields: {
      "embeddings.chunk_size": {
        type: "number",
        label: "SENESCHAL.Settings.Backend.Embeddings.ChunkSize",
        hint: "SENESCHAL.Settings.Backend.Embeddings.ChunkSizeHint",
        min: 128,
        max: 2048,
        step: 64,
      },
      "embeddings.chunk_overlap": {
        type: "number",
        label: "SENESCHAL.Settings.Backend.Embeddings.ChunkOverlap",
        hint: "SENESCHAL.Settings.Backend.Embeddings.ChunkOverlapHint",
        min: 0,
        max: 512,
        step: 16,
      },
    },
  },
  agentic: {
    label: "SENESCHAL.Settings.Backend.Section.Agentic",
    fields: {
      "agentic_loop.hard_timeout_secs": {
        type: "number",
        label: "SENESCHAL.Settings.Backend.Agentic.HardTimeout",
        hint: "SENESCHAL.Settings.Backend.Agentic.HardTimeoutHint",
        min: 60,
        max: 1800,
        step: 30,
      },
      "agentic_loop.external_tool_timeout_secs": {
        type: "number",
        label: "SENESCHAL.Settings.Backend.Agentic.ExternalToolTimeout",
        hint: "SENESCHAL.Settings.Backend.Agentic.ExternalToolTimeoutHint",
        min: 5,
        max: 300,
        step: 5,
      },
      "agentic_loop.tool_call_pause_threshold": {
        type: "number",
        label: "SENESCHAL.Settings.Backend.Agentic.ToolCallPauseThreshold",
        hint: "SENESCHAL.Settings.Backend.Agentic.ToolCallPauseThresholdHint",
        min: 1,
        max: 4294967295,
        step: 1,
      },
    },
  },
  limits: {
    label: "SENESCHAL.Settings.Backend.Section.Limits",
    fields: {
      "limits.max_document_size_bytes": {
        type: "number",
        label: "SENESCHAL.Settings.Backend.Limits.MaxDocumentSize",
        hint: "SENESCHAL.Settings.Backend.Limits.MaxDocumentSizeHint",
        min: 1048576,
        max: 1073741824,
        step: 1048576,
      },
    },
  },
  advanced: {
    label: "SENESCHAL.Settings.Backend.Section.Advanced",
    fields: {
      "mcp.enabled": {
        type: "checkbox",
        label: "SENESCHAL.Settings.Backend.Advanced.McpEnabled",
        hint: "SENESCHAL.Settings.Backend.Advanced.McpEnabledHint",
      },
      "mcp.path": {
        type: "text",
        label: "SENESCHAL.Settings.Backend.Advanced.McpPath",
        hint: "SENESCHAL.Settings.Backend.Advanced.McpPathHint",
      },
      "traveller_map.base_url": {
        type: "text",
        label: "SENESCHAL.Settings.Backend.Advanced.TravellerMapUrl",
        hint: "SENESCHAL.Settings.Backend.Advanced.TravellerMapUrlHint",
      },
      "traveller_map.timeout_secs": {
        type: "number",
        label: "SENESCHAL.Settings.Backend.Advanced.TravellerMapTimeout",
        hint: "SENESCHAL.Settings.Backend.Advanced.TravellerMapTimeoutHint",
        min: 5,
        max: 120,
        step: 5,
      },
      "traveller_worlds.base_url": {
        type: "text",
        label: "SENESCHAL.Settings.Backend.Advanced.TravellerWorldsUrl",
        hint: "SENESCHAL.Settings.Backend.Advanced.TravellerWorldsUrlHint",
      },
      "traveller_worlds.chrome_path": {
        type: "text",
        label: "SENESCHAL.Settings.Backend.Advanced.TravellerWorldsChromePath",
        hint: "SENESCHAL.Settings.Backend.Advanced.TravellerWorldsChromePathHint",
      },
    },
  },
};

/**
 * Backend Settings Dialog
 */
export class BackendSettingsDialog extends FormApplication {
  static get defaultOptions() {
    return foundry.utils.mergeObject(super.defaultOptions, {
      id: "seneschal-backend-settings",
      title: game.i18n.localize("SENESCHAL.Settings.Backend.Title"),
      template: `modules/${MODULE_ID}/templates/backend-settings.hbs`,
      width: 550,
      height: 600,
      closeOnSubmit: true,
      scrollY: [".settings-sections"],
    });
  }

  constructor(options = {}) {
    super({}, options);
    this.settings = {};
    this.overridden = [];
    this.models = [];
    this.isLoading = true;
    this.error = null;
    this.pendingChanges = {};
  }

  /**
   * Round a number to match the step precision to avoid floating-point display issues
   */
  _roundToStep(value, step) {
    if (typeof value !== "number" || typeof step !== "number" || step <= 0) {
      return value;
    }
    // Calculate decimal places from step (e.g., step=0.1 -> 1 decimal place)
    const decimalPlaces = Math.max(0, -Math.floor(Math.log10(step)));
    return Number(value.toFixed(decimalPlaces));
  }

  async getData() {
    // Build categories with localized labels and current values
    const categories = {};
    for (const [key, category] of Object.entries(SETTING_CATEGORIES)) {
      categories[key] = {
        label: game.i18n.localize(category.label),
        fields: {},
      };
      for (const [fieldKey, field] of Object.entries(category.fields)) {
        let value = this.settings[fieldKey];

        // Round number values to match step precision to avoid floating-point issues
        if (field.type === "number" && field.step && typeof value === "number") {
          value = this._roundToStep(value, field.step);
        }

        categories[key].fields[fieldKey] = {
          ...field,
          label: game.i18n.localize(field.label),
          hint: game.i18n.localize(field.hint),
          value: value,
          isOverridden: this.overridden.includes(fieldKey),
          allowEmpty: field.allowEmpty || false,
          // For select fields with models, pass the models list
          options:
            field.options === "models"
              ? this.models.map((m) => ({ value: m.name, label: m.name }))
              : null,
        };
      }
    }

    return {
      categories,
      isLoading: this.isLoading,
      error: this.error,
    };
  }

  async _render(force = false, options = {}) {
    await super._render(force, options);
    if (force) {
      await this._loadSettings();
    }
  }

  async _loadSettings() {
    const backendUrl = getSetting(SETTINGS.BACKEND_URL);
    if (!backendUrl) {
      this.isLoading = false;
      this.error = game.i18n.localize("SENESCHAL.Notifications.NotConfigured");
      this.render(false);
      return;
    }

    // Check for mixed content issue
    const pageProtocol = window.location.protocol;
    try {
      const backendProtocol = new URL(backendUrl).protocol;
      if (pageProtocol === "https:" && backendProtocol === "http:") {
        this.isLoading = false;
        this.error = game.i18n.localize("SENESCHAL.Notifications.MixedContent");
        this.render(false);
        return;
      }
    } catch {
      this.isLoading = false;
      this.error = game.i18n.localize("SENESCHAL.Settings.Backend.InvalidBackendUrl");
      this.render(false);
      return;
    }

    this.isLoading = true;
    this.error = null;
    this.render(false);

    try {
      const client = new BackendClient();

      // Load settings and models in parallel
      const [settingsResponse, models] = await Promise.all([
        client.getSettings(),
        client.getModels(),
      ]);

      this.settings = settingsResponse.settings;
      this.overridden = settingsResponse.overridden;
      this.models = models;
      this.isLoading = false;
      this.pendingChanges = {};
      this.render(false);
    } catch (error) {
      console.error("Failed to load settings:", error);
      this.isLoading = false;
      this.error = error.message;
      this.render(false);
    }
  }

  activateListeners(html) {
    super.activateListeners(html);

    // Refresh button
    html.find(".seneschal-refresh-settings").click(() => this._loadSettings());

    // Reset to default buttons
    html.find(".reset-setting").click(async (ev) => {
      const key = ev.currentTarget.dataset.key;
      await this._resetSetting(key);
    });
  }

  async _resetSetting(key) {
    try {
      const client = new BackendClient();
      // Setting to null removes the DB override
      await client.updateSettings({ [key]: null });
      ui.notifications.info(game.i18n.format("SENESCHAL.Settings.Backend.ResetSuccess", { key }));
      await this._loadSettings();
    } catch (error) {
      ui.notifications.error(error.message);
    }
  }

  async _updateObject(_event, formData) {
    // FormData keys are flat strings like "setting.ollama.default_model"
    // Strip the "setting." prefix to get the backend key
    const updates = {};
    for (const [formKey, value] of Object.entries(formData)) {
      if (!formKey.startsWith("setting.")) continue;
      const settingKey = formKey.slice("setting.".length);

      const originalValue = this.settings[settingKey];

      let typedValue = value;
      if (typeof originalValue === "number") {
        typedValue = Number(value);
      } else if (typeof originalValue === "boolean") {
        typedValue = Boolean(value);
      }

      updates[settingKey] = typedValue;
    }

    try {
      const client = new BackendClient();
      await client.updateSettings(updates);
      ui.notifications.info(game.i18n.localize("SENESCHAL.Settings.Backend.SaveSuccess"));
    } catch (error) {
      ui.notifications.error(error.message);
    }
  }
}
