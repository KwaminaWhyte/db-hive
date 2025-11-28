# Advanced CSV Exporter Plugin

Export your database query results to CSV format with advanced formatting options.

## Features

- **Custom Delimiters**: Choose between comma, semicolon, tab, or pipe delimiters
- **Header Options**: Include or exclude column headers
- **String Quoting**: Automatically quote string values
- **Multiple Encodings**: Support for UTF-8, UTF-16, and ASCII
- **NULL Value Handling**: Customize how NULL values are represented
- **Batch Export**: Export multiple queries at once
- **Export History**: Track all your exports

## Installation

1. Open DB-Hive
2. Navigate to View → Plugin Manager
3. Search for "CSV Exporter"
4. Click Install

## Usage

### Via Toolbar Button
After installation, you'll see an "Export CSV" button in the toolbar. Click it to export the current query results.

### Via Context Menu
Right-click on query results and select "Export to CSV" from the context menu.

### Via Keyboard Shortcut
Press `Ctrl+Shift+E` (Windows/Linux) or `Cmd+Shift+E` (Mac) to quickly export.

## Configuration

You can customize the export behavior in the plugin settings:

| Setting | Description | Default |
|---------|-------------|---------|
| Delimiter | Character to separate fields | `,` |
| Include Headers | Include column names as first row | `true` |
| Quote Strings | Wrap string values in quotes | `true` |
| Encoding | File character encoding | `utf-8` |
| NULL Value | How to represent NULL values | `""` (empty) |

## API for Other Plugins

This plugin exposes an API that other plugins can use:

```javascript
// Send a message to export data
await DBHive.sendToPlugin('com.dbhive.csv-exporter', {
  action: 'export',
  data: {
    columns: ['col1', 'col2'],
    rows: [[1, 'value'], [2, 'value2']]
  }
});
```

## Permissions

This plugin requires the following permissions:
- **Execute Query**: To access query results
- **Write Files**: To save CSV files
- **Show Notifications**: To display export status
- **Create Tab**: To create export preview tabs

## Support

For issues or feature requests, please visit:
https://github.com/dbhive/csv-exporter/issues

## License

MIT License - See LICENSE file for details