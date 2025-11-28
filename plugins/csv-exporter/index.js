/**
 * Advanced CSV Exporter Plugin for DB-Hive
 *
 * This plugin adds advanced CSV export functionality with:
 * - Custom delimiters
 * - Header options
 * - String quoting
 * - Multiple encodings
 * - NULL value handling
 *
 * Note: This plugin uses synchronous APIs compatible with boa_engine runtime.
 */

// Plugin metadata
var PLUGIN_ID = 'com.dbhive.csv-exporter';
var PLUGIN_NAME = 'Advanced CSV Exporter';
var PLUGIN_VERSION = '1.0.0';

// Get default configuration
function getDefaultConfig() {
  return {
    delimiter: ',',
    includeHeaders: true,
    quoteStrings: true,
    encoding: 'utf-8',
    nullValue: ''
  };
}

// Plugin lifecycle - called when plugin is loaded
function onLoad() {
  console.log('[' + PLUGIN_NAME + '] Plugin loaded v' + PLUGIN_VERSION);

  // Get plugin configuration
  var config = DBHive.getConfig() || getDefaultConfig();
  console.log('[' + PLUGIN_NAME + '] Config:', JSON.stringify(config));

  // Register UI components (synchronous in boa runtime)
  registerExportButton();
  registerContextMenu();

  return {
    success: true,
    message: 'CSV Exporter plugin loaded successfully'
  };
}

// Plugin lifecycle - called when plugin is unloaded
function onUnload() {
  console.log('[' + PLUGIN_NAME + '] Plugin unloaded');
  return {
    success: true,
    message: 'CSV Exporter plugin unloaded'
  };
}

// Register export button in the UI
function registerExportButton() {
  DBHive.registerUiComponent({
    id: 'csv-export-button',
    location: 'toolbar',
    componentType: {
      type: 'button',
      data: {
        label: 'Export CSV',
        icon: 'file-export',
        tooltip: 'Export query results to CSV file',
        action: 'exportToCsv'
      }
    }
  });
  console.log('[' + PLUGIN_NAME + '] Export button registered');
}

// Register context menu item
function registerContextMenu() {
  DBHive.registerUiComponent({
    id: 'csv-export-context',
    location: 'contextMenu',
    componentType: {
      type: 'menuItem',
      data: {
        label: 'Export to CSV',
        icon: 'file-export',
        shortcut: 'Ctrl+Shift+E',
        action: 'exportToCsv'
      }
    }
  });
  console.log('[' + PLUGIN_NAME + '] Context menu registered');
}

// Main export function
function exportToCsv() {
  try {
    // Show notification that export is starting
    DBHive.showNotification({
      title: 'CSV Export',
      message: 'Preparing to export data...',
      notificationType: 'info',
      duration: 3000
    });

    // Get the current query results
    var queryResults = getCurrentQueryResults();

    if (!queryResults || queryResults.rows.length === 0) {
      DBHive.showNotification({
        title: 'CSV Export',
        message: 'No data to export',
        notificationType: 'warning',
        duration: 3000
      });
      return { success: false, message: 'No data to export' };
    }

    // Convert to CSV
    var config = DBHive.getConfig() || getDefaultConfig();
    var csvContent = convertToCsv(queryResults, config);

    // Generate filename with timestamp
    var now = new Date();
    var timestamp = now.toISOString().replace(/[:.]/g, '-');
    var filename = 'export_' + timestamp + '.csv';

    // Save the file
    DBHive.writeFile(filename, csvContent);

    // Show success notification
    DBHive.showNotification({
      title: 'CSV Export',
      message: 'Data exported successfully to ' + filename,
      notificationType: 'success',
      duration: 5000
    });

    // Log to storage for history
    DBHive.storage.set('lastExport', {
      filename: filename,
      timestamp: now.toISOString(),
      rowCount: queryResults.rows.length,
      config: config
    });

    return { success: true, filename: filename, rowCount: queryResults.rows.length };

  } catch (error) {
    console.error('[' + PLUGIN_NAME + '] Export failed:', error);
    DBHive.showNotification({
      title: 'CSV Export Failed',
      message: error.message || 'An error occurred during export',
      notificationType: 'error',
      duration: 5000
    });
    return { success: false, error: error.message };
  }
}

// Get current query results (mock implementation)
function getCurrentQueryResults() {
  // In a real implementation, this would interact with the actual query results
  // For now, return mock data to demonstrate the plugin works
  return {
    columns: ['id', 'name', 'email', 'created_at'],
    rows: [
      [1, 'John Doe', 'john@example.com', '2024-01-15'],
      [2, 'Jane Smith', 'jane@example.com', '2024-01-16'],
      [3, 'Bob Johnson', null, '2024-01-17']
    ]
  };
}

// Convert query results to CSV format
function convertToCsv(queryResults, config) {
  var csv = '';

  // Add headers if configured
  if (config.includeHeaders) {
    var headerRow = [];
    for (var i = 0; i < queryResults.columns.length; i++) {
      headerRow.push(quoteValue(queryResults.columns[i], config));
    }
    csv = csv + headerRow.join(config.delimiter) + '\n';
  }

  // Add data rows
  var dataRows = [];
  for (var r = 0; r < queryResults.rows.length; r++) {
    var row = queryResults.rows[r];
    var formattedRow = [];
    for (var c = 0; c < row.length; c++) {
      formattedRow.push(formatValue(row[c], config));
    }
    dataRows.push(formattedRow.join(config.delimiter));
  }
  csv = csv + dataRows.join('\n');

  // Add BOM for UTF-16 encoding
  if (config.encoding === 'utf-16') {
    csv = '\uFEFF' + csv;
  }

  return csv;
}

// Format a value for CSV
function formatValue(value, config) {
  // Handle NULL values
  if (value === null || value === undefined) {
    return config.nullValue;
  }

  // Convert to string
  var stringValue = String(value);

  // Quote if needed
  if (config.quoteStrings && typeof value === 'string') {
    return quoteValue(stringValue, config);
  }

  return stringValue;
}

// Quote a value for CSV
function quoteValue(value, config) {
  // Check if value needs quoting
  var needsQuoting = value.indexOf(config.delimiter) >= 0 ||
                    value.indexOf('\n') >= 0 ||
                    value.indexOf('\r') >= 0 ||
                    value.indexOf('"') >= 0;

  if (needsQuoting || config.quoteStrings) {
    // Escape quotes by doubling them
    var escaped = value.replace(/"/g, '""');
    return '"' + escaped + '"';
  }

  return value;
}

// Export data programmatically (can be called by other plugins)
function exportData(data) {
  try {
    var config = DBHive.getConfig() || getDefaultConfig();
    var csvContent = convertToCsv(data, config);

    var now = new Date();
    var timestamp = now.toISOString().replace(/[:.]/g, '-');
    var filename = 'export_' + timestamp + '.csv';

    DBHive.writeFile(filename, csvContent);

    return {
      success: true,
      filename: filename,
      rowCount: data.rows.length
    };
  } catch (error) {
    return {
      success: false,
      error: error.message
    };
  }
}

// Advanced export with custom options
function exportWithOptions(queryResults, options) {
  var config = getDefaultConfig();
  // Merge options
  if (options) {
    if (options.delimiter !== undefined) config.delimiter = options.delimiter;
    if (options.includeHeaders !== undefined) config.includeHeaders = options.includeHeaders;
    if (options.quoteStrings !== undefined) config.quoteStrings = options.quoteStrings;
    if (options.encoding !== undefined) config.encoding = options.encoding;
    if (options.nullValue !== undefined) config.nullValue = options.nullValue;
  }

  var csvContent = convertToCsv(queryResults, config);
  var filename = (options && options.filename) ? options.filename : ('export_' + Date.now() + '.csv');

  DBHive.writeFile(filename, csvContent);

  return { success: true, filename: filename };
}

// Get export history
function getExportHistory() {
  var history = DBHive.storage.get('exportHistory');
  return history || [];
}

// Clear export history
function clearExportHistory() {
  DBHive.storage.set('exportHistory', []);
  return { success: true };
}

// Plugin exports - required for the runtime to find functions
// The runtime wraps plugin code in an IIFE and captures this assignment
__plugin_exports__ = {
  // Lifecycle hooks
  onLoad: onLoad,
  onUnload: onUnload,

  // Main functionality
  exportToCsv: exportToCsv,
  exportData: exportData,
  exportWithOptions: exportWithOptions,

  // History management
  getExportHistory: getExportHistory,
  clearExportHistory: clearExportHistory,

  // Configuration
  getDefaultConfig: getDefaultConfig,

  // Metadata
  id: PLUGIN_ID,
  name: PLUGIN_NAME,
  version: PLUGIN_VERSION
};
