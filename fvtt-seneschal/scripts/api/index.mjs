/**
 * FVTT API wrapper for document operations
 */

import { SETTINGS } from "../constants.mjs";
import { getSetting } from "../utils.mjs";
import { MGT2E_ENHANCEMENTS } from "../system/mgt2e.mjs";

/**
 * Wrapper for FVTT API calls with permission checking
 */
export class FvttApiWrapper {
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
   * Create a scene with a background image
   * @param {string} name - Scene name
   * @param {string} imagePath - Path to background image
   * @param {number} width - Scene width (optional, defaults to image width)
   * @param {number} height - Scene height (optional, defaults to image height)
   * @param {number} gridSize - Grid size in pixels (optional, default 100)
   * @param {string|null} folder - Name of folder to place the scene in
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async createScene(name, imagePath, width, height, gridSize, folder, userContext) {
    // Check GM permission
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can create scenes" };
    }

    try {
      // Load image to get dimensions if not provided
      let sceneWidth = width;
      let sceneHeight = height;

      if (!sceneWidth || !sceneHeight) {
        const img = await loadTexture(imagePath);
        sceneWidth = sceneWidth || img.width;
        sceneHeight = sceneHeight || img.height;
      }

      const sceneData = {
        name: name,
        width: sceneWidth,
        height: sceneHeight,
        background: {
          src: imagePath,
        },
        grid: {
          size: gridSize || 100,
          type: CONST.GRID_TYPES.SQUARE,
        },
        padding: 0,
      };

      // Add folder if specified
      if (folder) {
        const folderDoc = game.folders.find((f) => f.name === folder && f.type === "Scene");
        if (folderDoc) sceneData.folder = folderDoc.id;
      }

      const scene = await Scene.create(sceneData);
      return {
        success: true,
        id: scene.id,
        name: scene.name,
        width: scene.width,
        height: scene.height,
      };
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

  /**
   * Browse files in FVTT's file system
   * @param {string} path - Path to browse (defaults to root)
   * @param {string} source - Storage source ('data', 'public', 's3')
   * @param {string[]|null} extensions - Filter by file extensions
   * @param {boolean} recursive - Whether to list recursively (not implemented yet)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async browseAssets(
    path = "",
    source = "data",
    extensions = null,
    _recursive = false,
    userContext
  ) {
    // Check GM permission for security
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can browse assets" };
    }

    try {
      const result = await FilePicker.browse(source, path);
      let files = result.files;

      // Filter by extensions if specified
      if (extensions && extensions.length > 0) {
        files = files.filter((f) =>
          extensions.some((ext) => f.toLowerCase().endsWith(ext.toLowerCase()))
        );
      }

      // TODO: Handle _recursive if needed (would require walking subdirectories)

      return {
        path: path,
        directories: result.dirs.map((d) => d.split("/").pop()),
        files: files.map((f) => ({
          name: f.split("/").pop(),
          path: f,
        })),
      };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Fetch image data for vision model description
   * @param {string} imagePath - FVTT path to the image
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async fetchImageForDescription(imagePath, userContext) {
    // Check GM permission
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can describe images" };
    }

    try {
      // Fetch the image
      const response = await fetch(imagePath);
      if (!response.ok) {
        return { error: `Failed to fetch image: ${response.statusText}` };
      }

      // Convert to base64
      const blob = await response.blob();
      const buffer = await blob.arrayBuffer();
      const base64 = btoa(String.fromCharCode(...new Uint8Array(buffer)));

      // Get vision model from FVTT settings
      const visionModel = getSetting(SETTINGS.VISION_MODEL);

      return {
        image_path: imagePath,
        image_data: base64,
        mime_type: blob.type,
        size: blob.size,
        vision_model: visionModel || null,
      };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * List all folders for a specific document type
   * @param {string} documentType - Type of documents the folders contain
   * @param {string|null} parentFolder - Filter to only show folders inside this parent
   * @param {Object} userContext
   * @returns {Object}
   */
  static listFolders(documentType, parentFolder, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can list folders" };
    }

    const folders = game.folders.filter((f) => f.type === documentType);

    let result = folders.map((f) => ({
      id: f.id,
      name: f.name,
      parent: f.folder?.name || null,
      depth: f.depth,
      color: f.color,
    }));

    if (parentFolder) {
      result = result.filter((f) => f.parent === parentFolder);
    }

    return { folders: result };
  }

  /**
   * Create a new folder for organizing documents
   * @param {string} name - Name of the folder
   * @param {string} documentType - Type of documents this folder will contain
   * @param {string|null} parentFolder - Name of parent folder for nesting
   * @param {string|null} color - Folder color as hex code
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async createFolder(name, documentType, parentFolder, color, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can create folders" };
    }

    try {
      const folderData = {
        name: name,
        type: documentType,
      };

      if (parentFolder) {
        const parent = game.folders.find((f) => f.name === parentFolder && f.type === documentType);
        if (parent) folderData.folder = parent.id;
      }

      if (color) folderData.color = color;

      const folder = await Folder.create(folderData);
      return { success: true, id: folder.id, name: folder.name };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Update a folder's properties (rename, move, or change color)
   * @param {string} folderId - ID of the folder to update
   * @param {string|null} name - New name for the folder
   * @param {string|null} parentFolder - New parent folder name (null to move to root)
   * @param {string|null} color - New color as hex code
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async updateFolder(folderId, name, parentFolder, color, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can update folders" };
    }

    try {
      const folder = game.folders.get(folderId);
      if (!folder) return { error: "Folder not found" };

      const updateData = {};
      if (name !== undefined && name !== null) updateData.name = name;
      if (color !== undefined && color !== null) updateData.color = color;
      if (parentFolder !== undefined) {
        if (parentFolder === null) {
          updateData.folder = null;
        } else {
          const parent = game.folders.find(
            (f) => f.name === parentFolder && f.type === folder.type
          );
          if (parent) updateData.folder = parent.id;
        }
      }

      await folder.update(updateData);
      return { success: true };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Delete a folder
   * @param {string} folderId - ID of the folder to delete
   * @param {boolean} deleteContents - If true, also delete all documents inside
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async deleteFolder(folderId, deleteContents, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can delete folders" };
    }

    try {
      const folder = game.folders.get(folderId);
      if (!folder) return { error: "Folder not found" };

      await folder.delete({
        deleteSubfolders: deleteContents || false,
        deleteContents: deleteContents || false,
      });
      return { success: true };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * List documents with filtering support
   * @param {string} documentType - Type of document to list
   * @param {Object} args - Filter arguments (name, folder, limit, etc.)
   * @param {Object} userContext
   * @returns {Array}
   */
  static listDocuments(documentType, args, userContext) {
    const collection = this._getCollection(documentType);
    if (!collection) return [];

    const limit = args.limit || 20;
    let results = collection.filter((doc) => this.canAccess(doc, userContext));

    // Name filter (partial match, case-insensitive)
    if (args.name) {
      const nameLower = args.name.toLowerCase();
      results = results.filter((doc) => doc.name?.toLowerCase().includes(nameLower));
    }

    // Folder filter
    if (args.folder) {
      const folderDoc = game.folders.find(
        (f) => f.name === args.folder && f.type === this._getFolderType(documentType)
      );
      if (folderDoc) {
        results = results.filter((doc) => doc.folder?.id === folderDoc.id);
      } else {
        results = []; // Folder not found, return empty
      }
    }

    // Type filter (for actors and items)
    if (args.actor_type) {
      results = results.filter((doc) => doc.type === args.actor_type);
    }
    if (args.item_type) {
      results = results.filter((doc) => doc.type === args.item_type);
    }

    // Active filter (for scenes)
    if (args.active === true) {
      results = results.filter((doc) => doc.active);
    }

    return results.slice(0, limit).map((doc) => ({
      id: doc.id,
      name: doc.name,
      type: doc.type,
      folder: doc.folder?.name || null,
    }));
  }

  /**
   * Get folder type for a document type
   * @private
   */
  static _getFolderType(documentType) {
    const typeMap = {
      actor: "Actor",
      item: "Item",
      scene: "Scene",
      journal_entry: "JournalEntry",
      rollable_table: "RollTable",
    };
    return typeMap[documentType] || documentType;
  }

  /**
   * Resolve folder ID from folder name
   * @private
   */
  static _resolveFolderId(folderName, folderType) {
    if (!folderName) return null;
    const folder = game.folders.find((f) => f.name === folderName && f.type === folderType);
    return folder?.id || null;
  }

  /**
   * Update a scene
   * @param {Object} args - Update arguments
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async updateScene(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can update scenes" };
    }

    try {
      const scene = game.scenes.get(args.scene_id);
      if (!scene) return { error: "Scene not found" };

      const updateData = {};
      if (args.name !== undefined) updateData.name = args.name;
      if (args.image_path !== undefined) updateData["background.src"] = args.image_path;
      if (args.width !== undefined) updateData.width = args.width;
      if (args.height !== undefined) updateData.height = args.height;
      if (args.grid_size !== undefined) updateData["grid.size"] = args.grid_size;
      if (args.data) Object.assign(updateData, args.data);

      await scene.update(updateData);
      return { success: true };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Create an actor
   * @param {Object} args - Actor creation arguments
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async createActor(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can create actors" };
    }

    try {
      const actorData = {
        name: args.name,
        type: args.actor_type,
      };

      if (args.img) actorData.img = args.img;
      if (args.data) actorData.system = args.data;

      const folderId = this._resolveFolderId(args.folder, "Actor");
      if (folderId) actorData.folder = folderId;

      const actor = await Actor.create(actorData);
      return { success: true, id: actor.id, name: actor.name };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Update an actor
   * @param {Object} args - Update arguments
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async updateActor(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can update actors" };
    }

    try {
      const actor = game.actors.get(args.actor_id);
      if (!actor) return { error: "Actor not found" };

      const updateData = {};
      if (args.name !== undefined) updateData.name = args.name;
      if (args.img !== undefined) updateData.img = args.img;
      if (args.data) updateData.system = args.data;

      await actor.update(updateData);
      return { success: true };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Create an item
   * @param {Object} args - Item creation arguments
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async createItem(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can create items" };
    }

    try {
      const itemData = {
        name: args.name,
        type: args.item_type,
      };

      if (args.img) itemData.img = args.img;
      if (args.data) itemData.system = args.data;

      const folderId = this._resolveFolderId(args.folder, "Item");
      if (folderId) itemData.folder = folderId;

      const item = await Item.create(itemData);
      return { success: true, id: item.id, name: item.name };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Update an item
   * @param {Object} args - Update arguments
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async updateItem(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can update items" };
    }

    try {
      const item = game.items.get(args.item_id);
      if (!item) return { error: "Item not found" };

      const updateData = {};
      if (args.name !== undefined) updateData.name = args.name;
      if (args.img !== undefined) updateData.img = args.img;
      if (args.data) updateData.system = args.data;

      await item.update(updateData);
      return { success: true };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Create a journal entry
   * @param {Object} args - Journal creation arguments
   * @returns {Promise<Object>}
   */
  static async createJournalEntry(args) {
    try {
      const journalData = {
        name: args.name,
      };

      if (args.img) journalData.img = args.img;

      // Handle pages - either explicit pages array or simple content
      if (args.pages) {
        journalData.pages = args.pages;
      } else if (args.content) {
        journalData.pages = [
          {
            name: args.name,
            type: "text",
            text: { content: args.content },
          },
        ];
      }

      const folderId = this._resolveFolderId(args.folder, "JournalEntry");
      if (folderId) journalData.folder = folderId;

      const journal = await JournalEntry.create(journalData);
      return { success: true, id: journal.id, name: journal.name };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Update a journal entry
   * @param {Object} args - Update arguments
   * @returns {Promise<Object>}
   */
  static async updateJournalEntry(args) {
    try {
      const journal = game.journal.get(args.journal_id);
      if (!journal) return { error: "Journal not found" };

      const updateData = {};
      if (args.name !== undefined) updateData.name = args.name;

      // For simple content updates, update the first text page
      if (args.content !== undefined) {
        const textPage = journal.pages.find((p) => p.type === "text");
        if (textPage) {
          await textPage.update({ "text.content": args.content });
        }
      }

      // For full pages replacement
      if (args.pages !== undefined) {
        // Delete existing pages and create new ones
        await journal.deleteEmbeddedDocuments(
          "JournalEntryPage",
          journal.pages.map((p) => p.id)
        );
        await journal.createEmbeddedDocuments("JournalEntryPage", args.pages);
      }

      if (Object.keys(updateData).length > 0) {
        await journal.update(updateData);
      }

      return { success: true };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Add a page to a journal
   * @param {Object} args - Page creation arguments
   * @returns {Promise<Object>}
   */
  static async addJournalPage(args) {
    try {
      const journal = game.journal.get(args.journal_id);
      if (!journal) return { error: "Journal not found" };

      const pageData = {
        name: args.name,
        type: args.page_type,
      };

      if (args.page_type === "text") {
        pageData.text = { content: args.content || "" };
      } else if (args.page_type === "image") {
        pageData.src = args.src || "";
      } else {
        return { error: "Invalid page type. Use 'text' or 'image'" };
      }

      if (args.sort !== undefined) {
        pageData.sort = args.sort;
      }

      const [page] = await journal.createEmbeddedDocuments("JournalEntryPage", [pageData]);
      return { success: true, page_id: page.id, page_name: page.name };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Update a journal page
   * @param {Object} args - Page update arguments
   * @returns {Promise<Object>}
   */
  static async updateJournalPage(args) {
    try {
      const journal = game.journal.get(args.journal_id);
      if (!journal) return { error: "Journal not found" };

      const page = journal.pages.get(args.page_id);
      if (!page) return { error: "Page not found" };

      const updateData = {};
      if (args.name !== undefined) updateData.name = args.name;
      if (args.content !== undefined) updateData["text.content"] = args.content;
      if (args.src !== undefined) updateData.src = args.src;
      if (args.sort !== undefined) updateData.sort = args.sort;

      await page.update(updateData);
      return { success: true };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Delete a journal page
   * @param {Object} args - Page deletion arguments
   * @returns {Promise<Object>}
   */
  static async deleteJournalPage(args) {
    try {
      const journal = game.journal.get(args.journal_id);
      if (!journal) return { error: "Journal not found" };

      const page = journal.pages.get(args.page_id);
      if (!page) return { error: "Page not found" };

      await journal.deleteEmbeddedDocuments("JournalEntryPage", [args.page_id]);
      return { success: true };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * List pages in a journal
   * @param {Object} args - List arguments
   * @returns {Promise<Object>}
   */
  static async listJournalPages(args) {
    try {
      const journal = game.journal.get(args.journal_id);
      if (!journal) return { error: "Journal not found" };

      const pages = journal.pages.map((p) => ({
        id: p.id,
        name: p.name,
        type: p.type,
        sort: p.sort,
      }));

      return { success: true, pages };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Create a rollable table
   * @param {Object} args - Table creation arguments
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async createRollableTable(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can create rollable tables" };
    }

    try {
      const tableData = {
        name: args.name,
        formula: args.formula,
      };

      if (args.img) tableData.img = args.img;
      if (args.description) tableData.description = args.description;

      // Convert results format to FVTT format
      if (args.results) {
        tableData.results = args.results.map((r, idx) => ({
          range: r.range || [idx + 1, idx + 1],
          text: r.text,
          weight: r.weight || 1,
          img: r.img,
        }));
      }

      const folderId = this._resolveFolderId(args.folder, "RollTable");
      if (folderId) tableData.folder = folderId;

      const table = await RollTable.create(tableData);
      return { success: true, id: table.id, name: table.name };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Update a rollable table
   * @param {Object} args - Update arguments
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async updateRollableTable(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can update rollable tables" };
    }

    try {
      const table = game.tables.get(args.table_id);
      if (!table) return { error: "Rollable table not found" };

      const updateData = {};
      if (args.name !== undefined) updateData.name = args.name;
      if (args.formula !== undefined) updateData.formula = args.formula;

      // For results replacement
      if (args.results !== undefined) {
        // Delete existing results and create new ones
        await table.deleteEmbeddedDocuments(
          "TableResult",
          table.results.map((r) => r.id)
        );
        const newResults = args.results.map((r, idx) => ({
          range: r.range || [idx + 1, idx + 1],
          text: r.text,
          weight: r.weight || 1,
          img: r.img,
        }));
        await table.createEmbeddedDocuments("TableResult", newResults);
      }

      if (Object.keys(updateData).length > 0) {
        await table.update(updateData);
      }

      return { success: true };
    } catch (error) {
      return { error: error.message };
    }
  }
}
