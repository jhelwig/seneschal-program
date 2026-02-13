/**
 * Backend API client for communicating with the Seneschal backend service
 */

import { SETTINGS } from "../constants.mjs";
import { getSetting } from "../utils.mjs";

/**
 * Client for communicating with the Seneschal backend service
 */
export class BackendClient {
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

    return new Promise((resolve, reject) => {
      const xhr = new XMLHttpRequest();
      xhr.open("POST", `${this.baseUrl}/api/documents`);

      // Set a long timeout for PDF processing (5 minutes)
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
   * Update a document's details
   * @param {string} documentId
   * @param {Object} updates - Updated fields
   * @param {string} updates.title - Document title
   * @param {string} updates.access_level - Access level (player, trusted, assistant, gm_only)
   * @param {string} [updates.tags] - Comma-separated tags
   * @returns {Promise<Object>} Updated document
   */
  async updateDocument(documentId, updates) {
    const response = await fetch(`${this.baseUrl}/api/documents/${documentId}`, {
      method: "PUT",
      headers: {
        ...this.headers,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(updates),
    });
    if (!response.ok) {
      const errorBody = await response.json().catch(() => ({}));
      throw new Error(errorBody.message || `Failed to update document: ${response.statusText}`);
    }
    return response.json();
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
   * @returns {Promise<Object>} Extract result with count
   */
  async reextractDocumentImages(documentId) {
    const response = await fetch(`${this.baseUrl}/api/documents/${documentId}/images/extract`, {
      method: "POST",
      headers: this.headers,
      body: JSON.stringify({}),
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

  /**
   * Delete a single image
   * @param {string} imageId - Image ID
   * @returns {Promise<Object>} Delete result
   */
  async deleteImage(imageId) {
    const response = await fetch(`${this.baseUrl}/api/images/${imageId}`, {
      method: "DELETE",
      headers: this.headers,
    });
    if (!response.ok) {
      throw new Error(`Failed to delete image: ${response.statusText}`);
    }
    return response.json();
  }

  // ==================== Settings API ====================

  /**
   * Get all backend settings
   * @returns {Promise<Object>} Settings response with settings map and overridden keys
   */
  async getSettings() {
    const response = await fetch(`${this.baseUrl}/api/settings`, {
      method: "GET",
      headers: this.headers,
    });
    if (!response.ok) {
      const errorBody = await response.json().catch(() => ({}));
      throw new Error(errorBody.message || `Failed to get settings: ${response.statusText}`);
    }
    return response.json();
  }

  /**
   * Update backend settings
   * @param {Object} settings - Key-value pairs to update. Use null to delete/revert to default.
   * @returns {Promise<Object>} Updated settings response
   */
  async updateSettings(settings) {
    const response = await fetch(`${this.baseUrl}/api/settings`, {
      method: "PUT",
      headers: this.headers,
      body: JSON.stringify({ settings }),
    });
    if (!response.ok) {
      const errorBody = await response.json().catch(() => ({}));
      throw new Error(errorBody.message || `Failed to update settings: ${response.statusText}`);
    }
    return response.json();
  }
}
