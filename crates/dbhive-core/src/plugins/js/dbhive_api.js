/**
 * DBHive plugin API.
 *
 * Injected into every plugin runtime before the plugin's own code runs, and
 * the only way a plugin can reach anything outside its own interpreter.
 *
 * Two differences from the version this replaces:
 *
 *  1. Everything here is SYNCHRONOUS. The old wrapper declared `async`
 *     functions, which made every call return a Promise that nothing ever
 *     drove: `JSON.stringify` of the result gave "{}" and plugins worked
 *     around it by not using it. The host calls plugin functions and reads
 *     their return value directly, so plugins return values, not promises.
 *
 *  2. Interfaces are DATA. A plugin no longer returns something a web view
 *     will render. It describes panels, labels, fields and buttons, and the
 *     host draws them natively. `DBHive.ui` builds those descriptors.
 *
 * A plugin exports its entry points by assigning to __plugin_exports__:
 *
 *   __plugin_exports__ = {
 *     onLoad:  function () { return { success: true }; },
 *     renderUi: function () {
 *       return {
 *         title: 'My Plugin',
 *         panels: [ DBHive.ui.panel('Options', [
 *           DBHive.ui.input('name', 'File name', 'out.csv'),
 *           DBHive.ui.button('run', 'Export', { style: 'primary' })
 *         ]) ]
 *       };
 *     },
 *     onUiEvent: function (event) {
 *       // event.type is 'buttonPressed' | 'valueChanged' | 'menuSelected'
 *       // event.fields holds every field value on a button press.
 *       return null; // or a new descriptor to replace the interface
 *     }
 *   };
 */

var DBHive = {
    // ===== Database =====
    // Not available. Plugins are not given a database connection in this
    // build; these exist so a plugin fails with a clear message rather than
    // a TypeError.
    executeQuery: function () {
        throw new Error('DBHive.executeQuery: database access is not available to plugins');
    },
    getMetadata: function () {
        throw new Error('DBHive.getMetadata: database access is not available to plugins');
    },

    // ===== Interface =====

    /**
     * Replace this plugin's interface. Requires the modifyUI permission.
     * Rejected descriptors throw, with the reason.
     */
    setUi: function (descriptor) {
        return __dbhive_internal__.setUi(JSON.stringify(descriptor));
    },

    /** Ask the host to open a view for this plugin. Requires createTab. */
    createTab: function (config) {
        return __dbhive_internal__.createTab(JSON.stringify(config || {}));
    },

    /** Raise a toast. Requires showNotification. */
    showNotification: function (notification) {
        notification = notification || {};
        return __dbhive_internal__.showNotification(
            notification.title || 'Notification',
            notification.message || '',
            notification.notificationType || 'info'
        );
    },

    /** Builders for the declarative interface. All plain data. */
    ui: {
        panel: function (title, items, options) {
            options = options || {};
            return {
                id: options.id || '',
                title: title,
                collapsible: !!options.collapsible,
                open: options.open === undefined ? true : !!options.open,
                items: items || []
            };
        },
        text: function (text, style) {
            return { type: 'text', text: String(text), style: style || 'body' };
        },
        separator: function () {
            return { type: 'separator' };
        },
        button: function (id, label, options) {
            options = options || {};
            return {
                type: 'button',
                id: id,
                label: label,
                tooltip: options.tooltip || null,
                style: options.style || 'normal',
                action: options.action || null,
                enabled: options.enabled === undefined ? true : !!options.enabled
            };
        },
        input: function (id, label, value, options) {
            options = options || {};
            return {
                type: 'input',
                id: id,
                label: label,
                value: value === undefined || value === null ? '' : String(value),
                placeholder: options.placeholder || '',
                multiline: !!options.multiline,
                enabled: options.enabled === undefined ? true : !!options.enabled
            };
        },
        number: function (id, label, value, options) {
            options = options || {};
            return {
                type: 'number',
                id: id,
                label: label,
                value: value || 0,
                min: options.min === undefined ? null : options.min,
                max: options.max === undefined ? null : options.max,
                enabled: options.enabled === undefined ? true : !!options.enabled
            };
        },
        checkbox: function (id, label, value) {
            return { type: 'checkbox', id: id, label: label, value: !!value, enabled: true };
        },
        select: function (id, label, value, options) {
            var list = [];
            for (var i = 0; i < (options || []).length; i++) {
                var o = options[i];
                if (typeof o === 'string') {
                    list.push({ value: o, label: o });
                } else {
                    list.push({ value: o.value, label: o.label || o.value });
                }
            }
            return {
                type: 'select',
                id: id,
                label: label,
                value: value === undefined || value === null ? '' : String(value),
                options: list,
                enabled: true
            };
        },
        table: function (title, columns, rows) {
            var out = [];
            for (var r = 0; r < (rows || []).length; r++) {
                var row = [];
                for (var c = 0; c < rows[r].length; c++) {
                    var cell = rows[r][c];
                    row.push(cell === null || cell === undefined ? '' : String(cell));
                }
                out.push(row);
            }
            return { type: 'table', title: title || '', columns: columns || [], rows: out };
        },
        toolbarItem: function (id, label, options) {
            options = options || {};
            return {
                id: id,
                label: label,
                tooltip: options.tooltip || null,
                icon: options.icon || 'plug',
                action: options.action || null,
                enabled: options.enabled === undefined ? true : !!options.enabled
            };
        },
        menuItem: function (id, label, options) {
            options = options || {};
            return {
                id: id,
                label: label,
                shortcutHint: options.shortcutHint || null,
                action: options.action || null,
                enabled: options.enabled === undefined ? true : !!options.enabled
            };
        }
    },

    // ===== Files =====
    // Both are confined to this plugin's own data directory.

    readFile: function (path) {
        return __dbhive_internal__.readFile(path);
    },
    writeFile: function (path, content) {
        return __dbhive_internal__.writeFile(path, content);
    },

    // ===== Network =====

    /** http/https only, and never to a private or loopback address. */
    httpRequest: function (request) {
        request = request || {};
        var result = __dbhive_internal__.httpRequest(
            request.url,
            request.method || 'GET',
            request.body === undefined ? null : request.body
        );
        return JSON.parse(result);
    },

    // ===== System =====

    clipboard: {
        read: function () {
            return __dbhive_internal__.clipboardRead();
        },
        write: function (content) {
            return __dbhive_internal__.clipboardWrite(content);
        }
    },

    // ===== Storage =====

    storage: {
        get: function (key) {
            var result = __dbhive_internal__.getData(key);
            if (result) {
                try {
                    return JSON.parse(result);
                } catch (e) {
                    return result;
                }
            }
            return null;
        },
        set: function (key, value) {
            var stringValue = typeof value === 'string' ? value : JSON.stringify(value);
            return __dbhive_internal__.storeData(key, stringValue);
        }
    },

    // ===== Plugin to plugin =====

    sendToPlugin: function (targetId, message) {
        return __dbhive_internal__.sendToPlugin(targetId, JSON.stringify(message));
    },

    // ===== Configuration =====

    getConfig: function () {
        var configStr = __dbhive_internal__.getConfig();
        if (configStr) {
            try {
                return JSON.parse(configStr);
            } catch (e) {
                return null;
            }
        }
        return null;
    }
};

globalThis.DBHive = DBHive;
