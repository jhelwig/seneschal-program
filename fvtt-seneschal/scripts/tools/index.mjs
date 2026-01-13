/**
 * Tool executor for FVTT tools requested by the backend
 */

import { FvttApiWrapper } from "../api/index.mjs";

/**
 * Executes FVTT tools requested by the backend
 */
export class ToolExecutor {
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

      case "create_scene":
        return FvttApiWrapper.createScene(
          args.name,
          args.image_path,
          args.width,
          args.height,
          args.grid_size,
          args.folder,
          userContext
        );

      case "fvtt_assets_browse":
        return FvttApiWrapper.browseAssets(
          args.path,
          args.source,
          args.extensions,
          args.recursive,
          userContext
        );

      case "image_describe":
        return FvttApiWrapper.fetchImageForDescription(args.image_path, userContext);

      case "list_folders":
        return FvttApiWrapper.listFolders(args.document_type, args.parent_folder, userContext);

      case "create_folder":
        return FvttApiWrapper.createFolder(
          args.name,
          args.document_type,
          args.parent_folder,
          args.color,
          userContext
        );

      case "update_folder":
        return FvttApiWrapper.updateFolder(
          args.folder_id,
          args.name,
          args.parent_folder,
          args.color,
          userContext
        );

      case "delete_folder":
        return FvttApiWrapper.deleteFolder(args.folder_id, args.delete_contents, userContext);

      // Scene CRUD
      case "get_scene":
        return FvttApiWrapper.read("scene", args.scene_id, userContext);

      case "update_scene":
        return FvttApiWrapper.updateScene(args, userContext);

      case "delete_scene":
        return FvttApiWrapper.write("scene", "delete", { id: args.scene_id }, userContext);

      case "list_scenes":
        return FvttApiWrapper.listDocuments("scene", args, userContext);

      // Actor CRUD
      case "create_actor":
        return FvttApiWrapper.createActor(args, userContext);

      case "get_actor":
        return FvttApiWrapper.read("actor", args.actor_id, userContext);

      case "update_actor":
        return FvttApiWrapper.updateActor(args, userContext);

      case "delete_actor":
        return FvttApiWrapper.write("actor", "delete", { id: args.actor_id }, userContext);

      case "list_actors":
        return FvttApiWrapper.listDocuments("actor", args, userContext);

      // Item CRUD
      case "create_item":
        return FvttApiWrapper.createItem(args, userContext);

      case "get_item":
        return FvttApiWrapper.read("item", args.item_id, userContext);

      case "update_item":
        return FvttApiWrapper.updateItem(args, userContext);

      case "delete_item":
        return FvttApiWrapper.write("item", "delete", { id: args.item_id }, userContext);

      case "list_items":
        return FvttApiWrapper.listDocuments("item", args, userContext);

      // Journal CRUD
      case "create_journal":
        return FvttApiWrapper.createJournalEntry(args);

      case "get_journal":
        return FvttApiWrapper.read("journal_entry", args.journal_id, userContext);

      case "update_journal":
        return FvttApiWrapper.updateJournalEntry(args);

      case "delete_journal":
        return FvttApiWrapper.write(
          "journal_entry",
          "delete",
          { id: args.journal_id },
          userContext
        );

      case "list_journals":
        return FvttApiWrapper.listDocuments("journal_entry", args, userContext);

      // Journal Page CRUD
      case "add_journal_page":
        return FvttApiWrapper.addJournalPage(args);

      case "update_journal_page":
        return FvttApiWrapper.updateJournalPage(args);

      case "delete_journal_page":
        return FvttApiWrapper.deleteJournalPage(args);

      case "list_journal_pages":
        return FvttApiWrapper.listJournalPages(args);

      // Rollable Table CRUD
      case "create_rollable_table":
        return FvttApiWrapper.createRollableTable(args, userContext);

      case "get_rollable_table":
        return FvttApiWrapper.read("rollable_table", args.table_id, userContext);

      case "update_rollable_table":
        return FvttApiWrapper.updateRollableTable(args, userContext);

      case "delete_rollable_table":
        return FvttApiWrapper.write("rollable_table", "delete", { id: args.table_id }, userContext);

      case "list_rollable_tables":
        return FvttApiWrapper.listDocuments("rollable_table", args, userContext);

      default:
        return { error: `Unknown tool: ${tool}` };
    }
  }
}
