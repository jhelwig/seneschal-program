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
   * Get a FVTT document with pack_id support
   * @param {string} documentType - "actor", "item", "journal", "scene", "rollable_table"
   * @param {string} documentId
   * @param {string} [packId] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async getDocument(documentType, documentId, packId, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can read documents" };
    }

    try {
      let doc;
      if (packId) {
        const pack = this._getCompendiumPack(packId);
        if (!pack) return { error: `Pack not found: ${packId}` };
        doc = await pack.getDocument(documentId);
      } else {
        const collection = this._getCollection(documentType);
        if (!collection) return { error: `Unknown document type: ${documentType}` };
        doc = collection.get(documentId);
      }

      if (!doc) return { error: "Document not found" };
      return doc.toObject();
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Delete a FVTT document with pack_id support
   * @param {string} documentType - "actor", "item", "journal", "scene", "rollable_table"
   * @param {string} documentId
   * @param {string} [packId] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async deleteDocument(documentType, documentId, packId, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can delete documents" };
    }

    try {
      let doc;
      if (packId) {
        const validation = this._validatePackForWrite(packId);
        if (validation.error) return validation;
        doc = await validation.pack.getDocument(documentId);
      } else {
        const collection = this._getCollection(documentType);
        if (!collection) return { error: `Unknown document type: ${documentType}` };
        doc = collection.get(documentId);
      }

      if (!doc) return { error: "Document not found" };

      await doc.delete();
      return { success: true };
    } catch (error) {
      return { error: error.message };
    }
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
   * @param {Object} args - Scene creation arguments
   * @param {string} args.name - Scene name
   * @param {string} args.image_path - Path to background image
   * @param {number} [args.width] - Scene width (optional, defaults to image width)
   * @param {number} [args.height] - Scene height (optional, defaults to image height)
   * @param {number} [args.grid_size] - Grid size in pixels (optional, default 100)
   * @param {string|null} [args.folder] - Name of folder to place the scene in
   * @param {string} [args.pack_id] - Compendium pack ID (optional, creates in world if omitted)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async createScene(args, userContext) {
    // Check GM permission
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can create scenes" };
    }

    try {
      // Validate pack if specified
      if (args.pack_id) {
        const validation = this._validatePackForWrite(args.pack_id);
        if (validation.error) return validation;
      }

      // Load image to get dimensions if not provided
      let sceneWidth = args.width;
      let sceneHeight = args.height;

      if (!sceneWidth || !sceneHeight) {
        const img = await loadTexture(args.image_path);
        sceneWidth = sceneWidth || img.width;
        sceneHeight = sceneHeight || img.height;
      }

      const sceneData = {
        name: args.name,
        width: sceneWidth,
        height: sceneHeight,
        background: {
          src: args.image_path,
        },
        grid: {
          size: args.grid_size || 100,
          type: CONST.GRID_TYPES.SQUARE,
        },
        padding: 0,
      };

      // Add folder if specified (for world documents)
      if (args.folder && !args.pack_id) {
        const folderDoc = game.folders.find((f) => f.name === args.folder && f.type === "Scene");
        if (folderDoc) sceneData.folder = folderDoc.id;
      }

      const context = args.pack_id ? { pack: args.pack_id } : {};
      const scene = await Scene.create(sceneData, context);
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
      journal_entry: game.journal,
      scene: game.scenes,
      rolltable: game.tables,
      rollable_table: game.tables,
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
      journal_entry: JournalEntry,
      scene: Scene,
      rolltable: RollTable,
      rollable_table: RollTable,
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
   * @param {string} [packId] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async listFolders(documentType, parentFolder, packId, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can list folders" };
    }

    try {
      let folders;

      if (packId) {
        const pack = this._getCompendiumPack(packId);
        if (!pack) return { error: `Pack not found: ${packId}` };
        await pack.getIndex();
        folders = pack.folders;
      } else {
        const folderType = this._getFolderType(documentType);
        folders = game.folders.filter((f) => f.type === folderType);
      }

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
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Create a new folder for organizing documents
   * @param {string} name - Name of the folder
   * @param {string} documentType - Type of documents this folder will contain
   * @param {string|null} parentFolder - Name of parent folder for nesting
   * @param {string|null} color - Folder color as hex code
   * @param {string} [packId] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async createFolder(name, documentType, parentFolder, color, packId, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can create folders" };
    }

    try {
      if (packId) {
        const validation = this._validatePackForWrite(packId);
        if (validation.error) return validation;
      }

      const folderType = this._getFolderType(documentType);
      const folderData = {
        name: name,
        type: folderType,
      };

      if (parentFolder && !packId) {
        const parent = game.folders.find((f) => f.name === parentFolder && f.type === folderType);
        if (parent) folderData.folder = parent.id;
      }

      if (color) folderData.color = color;

      const context = packId ? { pack: packId } : {};
      const folder = await Folder.create(folderData, context);
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
   * @param {string} [packId] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async updateFolder(folderId, name, parentFolder, color, packId, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can update folders" };
    }

    try {
      let folder;
      if (packId) {
        const validation = this._validatePackForWrite(packId);
        if (validation.error) return validation;
        folder = validation.pack.folders.find((f) => f.id === folderId);
      } else {
        folder = game.folders.get(folderId);
      }
      if (!folder) return { error: "Folder not found" };

      const updateData = {};
      if (name !== undefined && name !== null) updateData.name = name;
      if (color !== undefined && color !== null) updateData.color = color;
      if (parentFolder !== undefined && !packId) {
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
   * @param {string} [packId] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async deleteFolder(folderId, deleteContents, packId, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can delete folders" };
    }

    try {
      let folder;
      if (packId) {
        const validation = this._validatePackForWrite(packId);
        if (validation.error) return validation;
        folder = validation.pack.folders.find((f) => f.id === folderId);
      } else {
        folder = game.folders.get(folderId);
      }
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
   * @param {Object} args - Filter arguments (name, folder, limit, pack_id, etc.)
   * @param {Object} userContext
   * @returns {Promise<Array>}
   */
  static async listDocuments(documentType, args, userContext) {
    const limit = args.limit || 20;

    // Handle compendium pack listing
    if (args.pack_id) {
      const pack = this._getCompendiumPack(args.pack_id);
      if (!pack) return [];

      await pack.getIndex();
      let results = pack.index.contents;

      // Name filter (partial match, case-insensitive)
      if (args.name) {
        const nameLower = args.name.toLowerCase();
        results = results.filter((doc) => doc.name?.toLowerCase().includes(nameLower));
      }

      // Folder filter
      if (args.folder) {
        const folder = pack.folders.find((f) => f.name === args.folder);
        if (folder) {
          results = results.filter((doc) => doc.folder === folder.id);
        } else {
          results = [];
        }
      }

      // Type filter
      if (args.actor_type) {
        results = results.filter((doc) => doc.type === args.actor_type);
      }
      if (args.item_type) {
        results = results.filter((doc) => doc.type === args.item_type);
      }

      return results.slice(0, limit).map((doc) => ({
        id: doc._id,
        name: doc.name,
        type: doc.type || null,
        folder: doc.folder || null,
      }));
    }

    // Handle world document listing
    const collection = this._getCollection(documentType);
    if (!collection) return [];

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
      journal: "JournalEntry",
      journal_entry: "JournalEntry",
      rolltable: "RollTable",
      rollable_table: "RollTable",
    };
    return typeMap[documentType.toLowerCase()] || documentType;
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
   * Apply folder update to updateData object
   * Handles: undefined (no change), null/empty string (move to root), folder name (move to folder)
   * @param {Object} updateData - The update data object to modify
   * @param {string|null|undefined} folderValue - The folder value from args
   * @param {string} folderType - The FVTT folder type (e.g., "Actor", "JournalEntry")
   * @private
   */
  static _applyFolderUpdate(updateData, folderValue, folderType) {
    if (folderValue === undefined) {
      // Not specified, don't change folder
      return;
    }
    if (folderValue === null || folderValue === "") {
      // Explicitly set to null/empty - move to root
      updateData.folder = null;
    } else {
      // Folder name specified - resolve to ID
      const folderId = this._resolveFolderId(folderValue, folderType);
      if (folderId) {
        updateData.folder = folderId;
      }
      // If folder not found, don't change (could add error handling here)
    }
  }

  /**
   * Update a scene
   * @param {Object} args - Update arguments
   * @param {string} args.scene_id - Scene ID
   * @param {string} [args.pack_id] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async updateScene(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can update scenes" };
    }

    try {
      let scene;
      if (args.pack_id) {
        const validation = this._validatePackForWrite(args.pack_id);
        if (validation.error) return validation;
        scene = await validation.pack.getDocument(args.scene_id);
      } else {
        scene = game.scenes.get(args.scene_id);
      }
      if (!scene) return { error: "Scene not found" };

      const updateData = {};
      if (args.name !== undefined) updateData.name = args.name;
      if (args.image_path !== undefined) updateData["background.src"] = args.image_path;
      if (args.width !== undefined) updateData.width = args.width;
      if (args.height !== undefined) updateData.height = args.height;
      if (args.grid_size !== undefined) updateData["grid.size"] = args.grid_size;
      if (args.data) Object.assign(updateData, args.data);
      if (!args.pack_id) {
        this._applyFolderUpdate(updateData, args.folder, "Scene");
      }

      await scene.update(updateData);
      return { success: true };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Create an actor
   * @param {Object} args - Actor creation arguments
   * @param {string} [args.pack_id] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async createActor(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can create actors" };
    }

    try {
      if (args.pack_id) {
        const validation = this._validatePackForWrite(args.pack_id);
        if (validation.error) return validation;
      }

      const actorData = {
        name: args.name,
        type: args.actor_type,
      };

      if (args.img) actorData.img = args.img;
      if (args.data) actorData.system = args.data;

      if (!args.pack_id) {
        const folderId = this._resolveFolderId(args.folder, "Actor");
        if (folderId) actorData.folder = folderId;
      }

      const context = args.pack_id ? { pack: args.pack_id } : {};
      const actor = await Actor.create(actorData, context);
      return { success: true, id: actor.id, name: actor.name };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Update an actor
   * @param {Object} args - Update arguments
   * @param {string} [args.pack_id] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async updateActor(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can update actors" };
    }

    try {
      let actor;
      if (args.pack_id) {
        const validation = this._validatePackForWrite(args.pack_id);
        if (validation.error) return validation;
        actor = await validation.pack.getDocument(args.actor_id);
      } else {
        actor = game.actors.get(args.actor_id);
      }
      if (!actor) return { error: "Actor not found" };

      const updateData = {};
      if (args.name !== undefined) updateData.name = args.name;
      if (args.img !== undefined) updateData.img = args.img;
      if (args.data) updateData.system = args.data;
      if (!args.pack_id) {
        this._applyFolderUpdate(updateData, args.folder, "Actor");
      }

      await actor.update(updateData);
      return { success: true };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Create an item
   * @param {Object} args - Item creation arguments
   * @param {string} [args.pack_id] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async createItem(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can create items" };
    }

    try {
      if (args.pack_id) {
        const validation = this._validatePackForWrite(args.pack_id);
        if (validation.error) return validation;
      }

      const itemData = {
        name: args.name,
        type: args.item_type,
      };

      if (args.img) itemData.img = args.img;
      if (args.data) itemData.system = args.data;

      if (!args.pack_id) {
        const folderId = this._resolveFolderId(args.folder, "Item");
        if (folderId) itemData.folder = folderId;
      }

      const context = args.pack_id ? { pack: args.pack_id } : {};
      const item = await Item.create(itemData, context);
      return { success: true, id: item.id, name: item.name };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Update an item
   * @param {Object} args - Update arguments
   * @param {string} [args.pack_id] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async updateItem(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can update items" };
    }

    try {
      let item;
      if (args.pack_id) {
        const validation = this._validatePackForWrite(args.pack_id);
        if (validation.error) return validation;
        item = await validation.pack.getDocument(args.item_id);
      } else {
        item = game.items.get(args.item_id);
      }
      if (!item) return { error: "Item not found" };

      const updateData = {};
      if (args.name !== undefined) updateData.name = args.name;
      if (args.img !== undefined) updateData.img = args.img;
      if (args.data) updateData.system = args.data;
      if (!args.pack_id) {
        this._applyFolderUpdate(updateData, args.folder, "Item");
      }

      await item.update(updateData);
      return { success: true };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Create a journal entry
   * @param {Object} args - Journal creation arguments
   * @param {string} [args.pack_id] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async createJournalEntry(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can create journals" };
    }

    try {
      if (args.pack_id) {
        const validation = this._validatePackForWrite(args.pack_id);
        if (validation.error) return validation;
      }

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

      if (!args.pack_id) {
        const folderId = this._resolveFolderId(args.folder, "JournalEntry");
        if (folderId) journalData.folder = folderId;
      }

      const context = args.pack_id ? { pack: args.pack_id } : {};
      const journal = await JournalEntry.create(journalData, context);
      return { success: true, id: journal.id, name: journal.name };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Update a journal entry
   * @param {Object} args - Update arguments
   * @param {string} [args.pack_id] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async updateJournalEntry(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can update journals" };
    }

    try {
      let journal;
      if (args.pack_id) {
        const validation = this._validatePackForWrite(args.pack_id);
        if (validation.error) return validation;
        journal = await validation.pack.getDocument(args.journal_id);
      } else {
        journal = game.journal.get(args.journal_id);
      }
      if (!journal) return { error: "Journal not found" };

      const updateData = {};
      if (args.name !== undefined) updateData.name = args.name;
      if (!args.pack_id) {
        this._applyFolderUpdate(updateData, args.folder, "JournalEntry");
      }

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
   * @param {string} [args.pack_id] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async addJournalPage(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can add journal pages" };
    }

    try {
      let journal;
      if (args.pack_id) {
        const validation = this._validatePackForWrite(args.pack_id);
        if (validation.error) return validation;
        journal = await validation.pack.getDocument(args.journal_id);
      } else {
        journal = game.journal.get(args.journal_id);
      }
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
   * @param {string} [args.pack_id] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async updateJournalPage(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can update journal pages" };
    }

    try {
      let journal;
      if (args.pack_id) {
        const validation = this._validatePackForWrite(args.pack_id);
        if (validation.error) return validation;
        journal = await validation.pack.getDocument(args.journal_id);
      } else {
        journal = game.journal.get(args.journal_id);
      }
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
   * @param {string} [args.pack_id] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async deleteJournalPage(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can delete journal pages" };
    }

    try {
      let journal;
      if (args.pack_id) {
        const validation = this._validatePackForWrite(args.pack_id);
        if (validation.error) return validation;
        journal = await validation.pack.getDocument(args.journal_id);
      } else {
        journal = game.journal.get(args.journal_id);
      }
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
   * @param {string} [args.pack_id] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async listJournalPages(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can list journal pages" };
    }

    try {
      let journal;
      if (args.pack_id) {
        const pack = this._getCompendiumPack(args.pack_id);
        if (!pack) return { error: `Pack not found: ${args.pack_id}` };
        journal = await pack.getDocument(args.journal_id);
      } else {
        journal = game.journal.get(args.journal_id);
      }
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
   * Bulk reorder pages in a journal
   * @param {Object} args - Reorder arguments
   * @param {string} args.journal_id - Journal ID
   * @param {Array<string>} args.page_order - Array of page IDs in desired order
   * @param {string} [args.pack_id] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async reorderJournalPages(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can reorder journal pages" };
    }

    try {
      let journal;
      if (args.pack_id) {
        const validation = this._validatePackForWrite(args.pack_id);
        if (validation.error) return validation;
        journal = await validation.pack.getDocument(args.journal_id);
      } else {
        journal = game.journal.get(args.journal_id);
      }
      if (!journal) return { error: "Journal not found" };

      const pageOrder = args.page_order;
      if (!Array.isArray(pageOrder) || pageOrder.length === 0) {
        return { error: "page_order must be a non-empty array of page IDs" };
      }

      // Validate all page IDs exist
      const missingPages = pageOrder.filter((id) => !journal.pages.get(id));
      if (missingPages.length > 0) {
        return { error: `Page IDs not found: ${missingPages.join(", ")}` };
      }

      // Build updates array with sort values spaced by 100
      const updates = pageOrder.map((pageId, index) => ({
        _id: pageId,
        sort: (index + 1) * 100,
      }));

      // Perform bulk update
      await journal.updateEmbeddedDocuments("JournalEntryPage", updates);

      return {
        success: true,
        message: `Reordered ${pageOrder.length} pages`,
        order: pageOrder.map((id, idx) => ({
          id,
          name: journal.pages.get(id).name,
          sort: (idx + 1) * 100,
        })),
      };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Create a rollable table
   * @param {Object} args - Table creation arguments
   * @param {string} [args.pack_id] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async createRollableTable(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can create rollable tables" };
    }

    try {
      if (args.pack_id) {
        const validation = this._validatePackForWrite(args.pack_id);
        if (validation.error) return validation;
      }

      const tableData = {
        name: args.name,
        formula: args.formula,
      };

      if (args.img) tableData.img = args.img;
      if (args.description) tableData.description = args.description;

      // Convert results format to FVTT format
      if (args.results) {
        tableData.results = args.results.map((r, idx) => {
          const result = {
            range: r.range || [idx + 1, idx + 1],
            weight: r.weight || 1,
          };

          // Handle different result types
          if (r.type === "document" || r.type === 1) {
            // Document-type result - links to a world document
            result.type = CONST.TABLE_RESULT_TYPES.DOCUMENT;
            result.documentCollection = r.document_collection;
            result.documentId = r.document_id;
            result.text = r.text || ""; // Optional display text
          } else if (r.type === "compendium" || r.type === 2) {
            // Compendium-type result - links to a compendium document
            result.type = CONST.TABLE_RESULT_TYPES.COMPENDIUM;
            result.documentCollection = r.document_collection;
            result.documentId = r.document_id;
            result.text = r.text || "";
          } else {
            // Default: Text-type result
            result.type = CONST.TABLE_RESULT_TYPES.TEXT;
            result.text = r.text || "";
          }

          if (r.img) result.img = r.img;

          return result;
        });
      }

      if (!args.pack_id) {
        const folderId = this._resolveFolderId(args.folder, "RollTable");
        if (folderId) tableData.folder = folderId;
      }

      const context = args.pack_id ? { pack: args.pack_id } : {};
      const table = await RollTable.create(tableData, context);
      return { success: true, id: table.id, name: table.name };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Update a rollable table
   * @param {Object} args - Update arguments
   * @param {string} [args.pack_id] - Compendium pack ID (optional)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async updateRollableTable(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can update rollable tables" };
    }

    try {
      let table;
      if (args.pack_id) {
        const validation = this._validatePackForWrite(args.pack_id);
        if (validation.error) return validation;
        table = await validation.pack.getDocument(args.table_id);
      } else {
        table = game.tables.get(args.table_id);
      }
      if (!table) return { error: "Rollable table not found" };

      const updateData = {};
      if (args.name !== undefined) updateData.name = args.name;
      if (args.formula !== undefined) updateData.formula = args.formula;
      if (!args.pack_id) {
        this._applyFolderUpdate(updateData, args.folder, "RollTable");
      }

      // For results replacement
      if (args.results !== undefined) {
        // Delete existing results and create new ones
        await table.deleteEmbeddedDocuments(
          "TableResult",
          table.results.map((r) => r.id)
        );
        const newResults = args.results.map((r, idx) => {
          const result = {
            range: r.range || [idx + 1, idx + 1],
            weight: r.weight || 1,
          };

          // Handle different result types
          if (r.type === "document" || r.type === 1) {
            // Document-type result - links to a world document
            result.type = CONST.TABLE_RESULT_TYPES.DOCUMENT;
            result.documentCollection = r.document_collection;
            result.documentId = r.document_id;
            result.text = r.text || "";
          } else if (r.type === "compendium" || r.type === 2) {
            // Compendium-type result - links to a compendium document
            result.type = CONST.TABLE_RESULT_TYPES.COMPENDIUM;
            result.documentCollection = r.document_collection;
            result.documentId = r.document_id;
            result.text = r.text || "";
          } else {
            // Default: Text-type result
            result.type = CONST.TABLE_RESULT_TYPES.TEXT;
            result.text = r.text || "";
          }

          if (r.img) result.img = r.img;

          return result;
        });
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

  /**
   * List all users in the world
   * @param {Object} args - List arguments
   * @param {boolean} [args.include_inactive=true] - Include offline users
   * @param {Object} userContext
   * @returns {Object}
   */
  static listUsers(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can list users" };
    }

    const includeInactive = args.include_inactive !== false;

    let users = game.users.map((u) => ({
      id: u.id,
      name: u.name,
      role: u.role,
      role_name: this._getRoleName(u.role),
      active: u.active,
      color: u.color,
      is_gm: u.isGM,
    }));

    if (!includeInactive) {
      users = users.filter((u) => u.active);
    }

    return { users };
  }

  /**
   * Get human-readable role name
   * @param {number} role - FVTT role number
   * @returns {string}
   * @private
   */
  static _getRoleName(role) {
    const roleNames = {
      0: "NONE",
      1: "PLAYER",
      2: "TRUSTED",
      3: "ASSISTANT",
      4: "GAMEMASTER",
    };
    return roleNames[role] || "UNKNOWN";
  }

  /**
   * Update ownership permissions for a document
   * @param {Object} args - Update arguments
   * @param {string} args.document_type - Type of document
   * @param {string} args.document_id - Document ID
   * @param {Object} args.ownership - Ownership mapping (user ID or "default" -> permission level)
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async updateOwnership(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can update ownership" };
    }

    try {
      const collection = this._getCollection(args.document_type);
      if (!collection) {
        return { error: `Unknown document type: ${args.document_type}` };
      }

      const document = collection.get(args.document_id);
      if (!document) {
        return { error: "Document not found" };
      }

      // Validate ownership object
      const ownership = args.ownership;
      if (!ownership || typeof ownership !== "object") {
        return { error: "ownership must be an object" };
      }

      // Validate permission levels (0-3)
      for (const [key, value] of Object.entries(ownership)) {
        if (typeof value !== "number" || value < 0 || value > 3) {
          return { error: `Invalid permission level for ${key}: ${value}. Must be 0-3.` };
        }
        // Validate user IDs exist (except for "default")
        if (key !== "default" && !game.users.get(key)) {
          return { error: `User not found: ${key}` };
        }
      }

      // Update the document's ownership
      await document.update({ ownership });

      return {
        success: true,
        ownership: document.ownership,
      };
    } catch (error) {
      return { error: error.message };
    }
  }

  // ==========================================
  // Compendium Pack Methods
  // ==========================================

  /**
   * List available compendium packs
   * @param {Object} args - List arguments
   * @param {string} [args.document_type] - Filter by document type
   * @param {number} [args.limit=50] - Maximum results
   * @param {Object} userContext
   * @returns {Object}
   */
  static listCompendiumPacks(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can list compendium packs" };
    }

    const limit = args.limit || 50;
    let packs = Array.from(game.packs);

    // Filter by document type if specified
    if (args.document_type) {
      packs = packs.filter((p) => p.documentName === args.document_type);
    }

    const result = packs.slice(0, limit).map((pack) => ({
      collection: pack.collection,
      name: pack.metadata.name,
      label: pack.metadata.label,
      documentName: pack.documentName,
      system: pack.metadata.system || null,
      locked: pack.locked,
      size: pack.index.size,
    }));

    return { packs: result };
  }

  /**
   * Browse documents in a compendium pack using the lightweight index
   * @param {Object} args - Browse arguments
   * @param {string} args.pack_id - Compendium pack ID (e.g., 'dnd5e.monsters')
   * @param {string} [args.name] - Filter by document name (partial match)
   * @param {string} [args.folder] - Filter by folder name
   * @param {number} [args.offset=0] - Skip first N results
   * @param {number} [args.limit=50] - Maximum results
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async browseCompendiumPack(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can browse compendium packs" };
    }

    try {
      const pack = game.packs.get(args.pack_id);
      if (!pack) return { error: `Pack not found: ${args.pack_id}` };

      // Ensure the index is loaded
      await pack.getIndex();

      const offset = args.offset || 0;
      const limit = args.limit || 50;
      let documents = pack.index.contents;

      // Filter by name (partial match, case-insensitive)
      if (args.name) {
        const nameLower = args.name.toLowerCase();
        documents = documents.filter((d) => d.name?.toLowerCase().includes(nameLower));
      }

      // Filter by folder
      if (args.folder) {
        const folder = pack.folders.find((f) => f.name === args.folder);
        if (folder) {
          documents = documents.filter((d) => d.folder === folder.id);
        } else {
          documents = [];
        }
      }

      const total = documents.length;
      documents = documents.slice(offset, offset + limit);

      return {
        documents: documents.map((d) => ({
          _id: d._id,
          name: d.name,
          img: d.img || null,
          type: d.type || null,
          folder: d.folder || null,
        })),
        total_count: total,
        offset,
        limit,
      };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Search for documents across all compendium packs by name
   * @param {Object} args - Search arguments
   * @param {string} args.query - Search term
   * @param {string} [args.document_type] - Filter to packs of this document type
   * @param {number} [args.limit=50] - Maximum results
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async searchCompendiumPacks(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can search compendium packs" };
    }

    try {
      const limit = args.limit || 50;
      const queryLower = args.query.toLowerCase();
      const results = [];

      let packs = Array.from(game.packs);

      // Filter by document type if specified
      if (args.document_type) {
        packs = packs.filter((p) => p.documentName === args.document_type);
      }

      for (const pack of packs) {
        // Ensure index is loaded
        await pack.getIndex();

        for (const doc of pack.index.contents) {
          if (doc.name?.toLowerCase().includes(queryLower)) {
            results.push({
              pack_id: pack.collection,
              document_id: doc._id,
              name: doc.name,
              type: doc.type || null,
              img: doc.img || null,
            });

            if (results.length >= limit) break;
          }
        }

        if (results.length >= limit) break;
      }

      return {
        results,
        total_count: results.length,
      };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Import documents from a compendium pack into the world
   * @param {Object} args - Import arguments
   * @param {string} args.pack_id - Compendium pack ID
   * @param {string[]} [args.document_ids] - Document IDs to import (omit for all)
   * @param {string} [args.folder] - World folder name to place imported documents
   * @param {boolean} [args.keep_id=false] - Preserve original document IDs
   * @param {boolean} [args.keep_folders=false] - Recreate folder structure
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async importFromCompendium(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can import from compendiums" };
    }

    try {
      const pack = game.packs.get(args.pack_id);
      if (!pack) return { error: `Pack not found: ${args.pack_id}` };

      await pack.getIndex();

      // Determine which documents to import
      let documentIds = args.document_ids;
      if (!documentIds || documentIds.length === 0) {
        documentIds = pack.index.contents.map((d) => d._id);
      }

      const imported = [];
      const failed = [];

      // Resolve target folder
      let targetFolderId = null;
      if (args.folder) {
        const folderType = pack.documentName;
        const folder = game.folders.find((f) => f.name === args.folder && f.type === folderType);
        if (folder) targetFolderId = folder.id;
      }

      for (const docId of documentIds) {
        try {
          const doc = await pack.getDocument(docId);
          if (!doc) {
            failed.push({ id: docId, error: "Document not found in pack" });
            continue;
          }

          const importOptions = {
            folder: targetFolderId,
            keepId: args.keep_id || false,
          };

          const importedDoc = await pack.importDocument(doc, importOptions);
          imported.push({ id: importedDoc.id, name: importedDoc.name });
        } catch (err) {
          failed.push({ id: docId, error: err.message });
        }
      }

      return {
        success: true,
        imported,
        failed,
      };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Export documents from the world to a compendium pack
   * @param {Object} args - Export arguments
   * @param {string} args.pack_id - Compendium pack ID (must be unlocked)
   * @param {string} args.document_type - Type of documents to export
   * @param {string[]} args.document_ids - World document IDs to export
   * @param {boolean} [args.keep_id=false] - Preserve document IDs in compendium
   * @param {boolean} [args.keep_folders=false] - Recreate folder structure
   * @param {Object} userContext
   * @returns {Promise<Object>}
   */
  static async exportToCompendium(args, userContext) {
    if (userContext.role < CONST.USER_ROLES.GAMEMASTER) {
      return { error: "Only GMs can export to compendiums" };
    }

    try {
      const pack = game.packs.get(args.pack_id);
      if (!pack) return { error: `Pack not found: ${args.pack_id}` };

      if (pack.locked) {
        return { error: `Pack is locked: ${args.pack_id}. Unlock it first in FVTT.` };
      }

      const collection = this._getCollection(args.document_type);
      if (!collection) {
        return { error: `Unknown document type: ${args.document_type}` };
      }

      const exported = [];
      const failed = [];

      for (const docId of args.document_ids) {
        try {
          const doc = collection.get(docId);
          if (!doc) {
            failed.push({ id: docId, error: "Document not found in world" });
            continue;
          }

          const exportOptions = {
            keepId: args.keep_id || false,
          };

          // Export using toCompendium
          const compendiumData = doc.toCompendium(pack, exportOptions);
          const cls = pack.documentClass;
          const created = await cls.create(compendiumData, { pack: pack.collection });

          exported.push({ id: created.id, name: created.name });
        } catch (err) {
          failed.push({ id: docId, error: err.message });
        }
      }

      return {
        success: true,
        exported,
        failed,
      };
    } catch (error) {
      return { error: error.message };
    }
  }

  /**
   * Get a compendium pack by ID
   * @param {string} packId - Pack collection ID
   * @returns {CompendiumCollection|null}
   * @private
   */
  static _getCompendiumPack(packId) {
    return packId ? game.packs.get(packId) : null;
  }

  /**
   * Check if a pack is editable (exists and not locked)
   * @param {string} packId - Pack collection ID
   * @returns {Object} - { pack, error } - pack if valid, error if not
   * @private
   */
  static _validatePackForWrite(packId) {
    const pack = this._getCompendiumPack(packId);
    if (!pack) {
      return { error: `Pack not found: ${packId}` };
    }
    if (pack.locked) {
      return { error: `Pack is locked: ${packId}. Unlock it first in FVTT.` };
    }
    return { pack };
  }
}
