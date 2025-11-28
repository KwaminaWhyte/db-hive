/**
 * DBHive API - JavaScript wrapper for plugin interactions
 *
 * This file is injected into the plugin runtime to provide
 * the global DBHive object that plugins use.
 */

const DBHive = {
    // ========== Database API ==========

    /**
     * Execute a SQL query on a database connection
     * @param {string} query - The SQL query to execute
     * @param {string} connectionId - The connection ID
     * @returns {Promise<{columns: string[], rows: any[][], executionTime: number, rowCount: number}>}
     */
    executeQuery: async (query, connectionId) => {
        // TODO: Implement when database integration is added
        console.log(`[DBHive] executeQuery called: ${query.substring(0, 50)}...`);
        return {
            columns: [],
            rows: [],
            executionTime: 0,
            rowCount: 0
        };
    },

    /**
     * Get database metadata (schemas, tables, columns)
     * @param {string} connectionId - The connection ID
     * @returns {Promise<{schemas: string[], tables: Object}>}
     */
    getMetadata: async (connectionId) => {
        // TODO: Implement when database integration is added
        console.log(`[DBHive] getMetadata called for connection: ${connectionId}`);
        return {
            schemas: [],
            tables: {}
        };
    },

    // ========== UI API ==========

    /**
     * Create a new tab in the UI
     * @param {Object} config - Tab configuration
     * @param {string} config.title - Tab title
     * @param {string} [config.icon] - Icon name
     * @param {Object} config.contentType - Content type configuration
     * @param {boolean} [config.closable=true] - Whether the tab can be closed
     * @returns {Promise<string>} Tab ID
     */
    createTab: async (config) => {
        return await __dbhive_internal__.createTab(JSON.stringify(config));
    },

    /**
     * Show a notification to the user
     * @param {Object} notification - Notification configuration
     * @param {string} notification.title - Notification title
     * @param {string} notification.message - Notification message
     * @param {string} [notification.notificationType='info'] - Type: info, success, warning, error
     * @param {number} [notification.duration=5000] - Duration in milliseconds
     * @returns {Promise<void>}
     */
    showNotification: async (notification) => {
        return await __dbhive_internal__.showNotification(
            notification.title || 'Notification',
            notification.message || '',
            notification.notificationType || 'info'
        );
    },

    /**
     * Register a UI component (button, menu item, panel)
     * @param {Object} component - Component configuration
     * @param {string} component.id - Unique component ID
     * @param {string} component.location - Location: toolbar, sidebar, statusBar, contextMenu, panel
     * @param {Object} component.componentType - Component type and data
     * @returns {Promise<void>}
     */
    registerUiComponent: async (component) => {
        return await __dbhive_internal__.registerUiComponent(JSON.stringify(component));
    },

    // ========== File System API ==========

    /**
     * Read a file from the plugin's data directory
     * @param {string} path - Relative path to the file
     * @returns {Promise<string>} File contents
     */
    readFile: async (path) => {
        return await __dbhive_internal__.readFile(path);
    },

    /**
     * Write a file to the plugin's data directory
     * @param {string} path - Relative path to the file
     * @param {string} content - Content to write
     * @returns {Promise<void>}
     */
    writeFile: async (path, content) => {
        return await __dbhive_internal__.writeFile(path, content);
    },

    // ========== Network API ==========

    /**
     * Make an HTTP request
     * @param {Object} request - Request configuration
     * @param {string} request.url - URL to request
     * @param {string} [request.method='GET'] - HTTP method
     * @param {Object} [request.headers] - Request headers
     * @param {string} [request.body] - Request body
     * @returns {Promise<{status: number, headers: Object, body: string}>}
     */
    httpRequest: async (request) => {
        const result = await __dbhive_internal__.httpRequest(
            request.url,
            request.method || 'GET',
            request.body
        );
        return JSON.parse(result);
    },

    // ========== System API ==========

    /**
     * Clipboard access object
     */
    clipboard: {
        /**
         * Read text from clipboard
         * @returns {Promise<string>}
         */
        read: async () => {
            return await __dbhive_internal__.clipboardRead();
        },

        /**
         * Write text to clipboard
         * @param {string} content - Content to write
         * @returns {Promise<void>}
         */
        write: async (content) => {
            return await __dbhive_internal__.clipboardWrite(content);
        }
    },

    // ========== Storage API ==========

    /**
     * Persistent key-value storage for the plugin
     */
    storage: {
        /**
         * Get a value from storage
         * @param {string} key - Storage key
         * @returns {Promise<any>} Stored value or null
         */
        get: async (key) => {
            const result = await __dbhive_internal__.getData(key);
            if (result) {
                try {
                    return JSON.parse(result);
                } catch {
                    return result;
                }
            }
            return null;
        },

        /**
         * Store a value
         * @param {string} key - Storage key
         * @param {any} value - Value to store
         * @returns {Promise<void>}
         */
        set: async (key, value) => {
            const stringValue = typeof value === 'string' ? value : JSON.stringify(value);
            return await __dbhive_internal__.storeData(key, stringValue);
        }
    },

    // ========== Plugin Communication ==========

    /**
     * Send a message to another plugin
     * @param {string} targetId - Target plugin ID
     * @param {Object} message - Message to send
     * @returns {Promise<void>}
     */
    sendToPlugin: async (targetId, message) => {
        return await __dbhive_internal__.sendToPlugin(targetId, JSON.stringify(message));
    },

    /**
     * Register a message handler for inter-plugin communication
     * Note: This is currently a stub - full implementation requires event system
     * @param {Function} handler - Message handler function
     */
    onMessage: (handler) => {
        // Store the handler - will be called when messages arrive
        globalThis.__pluginMessageHandler = handler;
        console.log('[DBHive] Message handler registered');
    },

    // ========== Configuration ==========

    /**
     * Get the plugin's configuration
     * @returns {Object|null} Plugin configuration
     */
    getConfig: () => {
        const configStr = __dbhive_internal__.getConfig();
        if (configStr) {
            try {
                return JSON.parse(configStr);
            } catch {
                return null;
            }
        }
        return null;
    }
};

// Make DBHive available globally
globalThis.DBHive = DBHive;
