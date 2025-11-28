# DB-Hive Plugin Development Guide

## Overview

DB-Hive supports extending its functionality through plugins. Plugins can add new features, integrate with external services, modify the UI, and much more.

## Table of Contents

1. [Getting Started](#getting-started)
2. [Plugin Architecture](#plugin-architecture)
3. [Creating Your First Plugin](#creating-your-first-plugin)
4. [Plugin API Reference](#plugin-api-reference)
5. [Best Practices](#best-practices)
6. [Testing Plugins](#testing-plugins)
7. [Publishing Plugins](#publishing-plugins)

## Getting Started

### Prerequisites

- Basic knowledge of JavaScript or WebAssembly
- Understanding of DB-Hive's architecture
- Node.js (for JavaScript plugins) or Rust (for WASM plugins)

### Plugin Types

DB-Hive supports two types of plugins:

1. **JavaScript Plugins**: Written in JavaScript/TypeScript, run in a sandboxed environment
2. **WebAssembly Plugins**: Compiled from languages like Rust, C++, or Go

## Plugin Architecture

### File Structure

```
my-plugin/
├── manifest.json       # Plugin metadata and configuration
├── index.js           # Main plugin file (for JS plugins)
├── icon.svg           # Plugin icon
├── README.md          # Plugin documentation
└── assets/            # Additional resources (optional)
```

### Manifest File

The `manifest.json` file defines your plugin's metadata, permissions, and configuration:

```json
{
  "id": "com.example.my-plugin",
  "name": "My Plugin",
  "version": "1.0.0",
  "description": "A brief description of what your plugin does",
  "author": {
    "name": "Your Name",
    "email": "you@example.com",
    "url": "https://yourwebsite.com"
  },
  "category": "tool",
  "main": "index.js",
  "pluginType": "javascript",
  "permissions": ["executeQuery", "writeFiles"],
  "minVersion": "0.13.0",
  "maxVersion": "1.0.0",
  "icon": "icon.svg",
  "homepage": "https://github.com/you/my-plugin",
  "repository": "https://github.com/you/my-plugin",
  "license": "MIT",
  "keywords": ["database", "tool"],
  "dependencies": {},
  "configSchema": {
    "type": "object",
    "properties": {
      "enabled": {
        "type": "boolean",
        "default": true,
        "title": "Enable Feature",
        "description": "Enable or disable the main feature"
      }
    }
  }
}
```

### Categories

Plugins must belong to one of these categories:

- `driver`: Database drivers
- `theme`: UI themes
- `tool`: Utility tools
- `export`: Export formats
- `import`: Import formats
- `formatter`: Code formatters
- `analyzer`: Query analyzers
- `visualizer`: Data visualizers
- `extension`: General extensions

### Permissions

Plugins must declare the permissions they need:

- `readFiles`: Read files from the plugin's data directory
- `writeFiles`: Write files to the plugin's data directory
- `executeQuery`: Execute database queries
- `modifySchema`: Modify database schemas
- `readMetadata`: Read database metadata
- `createTab`: Create new UI tabs
- `modifyUI`: Modify the user interface
- `showNotification`: Display system notifications
- `networkAccess`: Make network requests
- `runCommand`: Execute system commands
- `accessClipboard`: Read/write clipboard
- `accessOtherPlugins`: Communicate with other plugins

## Creating Your First Plugin

### Step 1: Create Plugin Directory

```bash
mkdir my-awesome-plugin
cd my-awesome-plugin
```

### Step 2: Create Manifest

Create `manifest.json`:

```json
{
  "id": "com.example.awesome",
  "name": "Awesome Plugin",
  "version": "1.0.0",
  "description": "My awesome DB-Hive plugin",
  "author": {
    "name": "Your Name"
  },
  "category": "tool",
  "main": "index.js",
  "pluginType": "javascript",
  "permissions": ["showNotification"],
  "minVersion": "0.13.0",
  "license": "MIT"
}
```

### Step 3: Create Main File

Create `index.js`:

```javascript
// Plugin lifecycle - called when plugin is loaded
async function onLoad() {
  console.log('Awesome Plugin loaded!');

  // Show a notification
  await DBHive.showNotification({
    title: 'Awesome Plugin',
    message: 'Plugin loaded successfully!',
    notificationType: 'success'
  });

  return {
    success: true,
    message: 'Plugin initialized'
  };
}

// Plugin lifecycle - called when plugin is unloaded
async function onUnload() {
  console.log('Awesome Plugin unloaded');
  return {
    success: true
  };
}

// Export plugin interface
// The runtime provides __plugin_exports__ - simply assign your exports to it
__plugin_exports__ = {
  onLoad,
  onUnload,
  name: 'Awesome Plugin',
  version: '1.0.0'
};
```

> **Important**: The plugin runtime automatically provides `__plugin_exports__`. Simply assign your exports object to it without using `var`, `let`, `const`, or `globalThis`. The runtime wraps your code in a scope that captures this assignment.

## Plugin API Reference

### Global Object: `DBHive`

The `DBHive` object is available globally in your plugin and provides access to DB-Hive's functionality.

### Database API

#### `DBHive.executeQuery(query, connectionId)`
Execute a SQL query on a database connection.

```javascript
const result = await DBHive.executeQuery(
  'SELECT * FROM users LIMIT 10',
  'connection-123'
);
console.log(result.rows);
```

#### `DBHive.getMetadata(connectionId)`
Get database metadata (schemas, tables, columns).

```javascript
const metadata = await DBHive.getMetadata('connection-123');
console.log(metadata.schemas);
```

### UI API

#### `DBHive.createTab(config)`
Create a new tab in the UI.

```javascript
const tabId = await DBHive.createTab({
  title: 'My Custom Tab',
  icon: 'star',
  contentType: { type: 'html', content: '<h1>Hello World</h1>' },
  closable: true
});
```

#### `DBHive.showNotification(notification)`
Display a system notification.

```javascript
await DBHive.showNotification({
  title: 'Success',
  message: 'Operation completed',
  notificationType: 'success',
  duration: 5000
});
```

#### `DBHive.registerUiComponent(component)`
Register a UI component (button, menu item, panel).

```javascript
await DBHive.registerUiComponent({
  id: 'my-button',
  location: 'toolbar',
  componentType: {
    type: 'button',
    data: {
      label: 'My Button',
      icon: 'star',
      tooltip: 'Click me!',
      action: 'myAction'
    }
  }
});
```

### File System API

#### `DBHive.readFile(path)`
Read a file from the plugin's data directory.

```javascript
const content = await DBHive.readFile('config.json');
const config = JSON.parse(content);
```

#### `DBHive.writeFile(path, content)`
Write a file to the plugin's data directory.

```javascript
await DBHive.writeFile('output.txt', 'Hello World');
```

### Network API

#### `DBHive.httpRequest(request)`
Make an HTTP request.

```javascript
const response = await DBHive.httpRequest({
  url: 'https://api.example.com/data',
  method: 'GET',
  headers: {
    'Authorization': 'Bearer token'
  }
});
console.log(response.body);
```

### System API

#### `DBHive.clipboard`
Access the system clipboard.

```javascript
// Read clipboard
const text = await DBHive.clipboard.read();

// Write to clipboard
await DBHive.clipboard.write('Hello World');
```

### Storage API

#### `DBHive.storage`
Persistent key-value storage for your plugin.

```javascript
// Store data
await DBHive.storage.set('user-preference', { theme: 'dark' });

// Retrieve data
const pref = await DBHive.storage.get('user-preference');
```

### Plugin Communication

#### `DBHive.sendToPlugin(targetId, message)`
Send a message to another plugin.

```javascript
await DBHive.sendToPlugin('com.example.other-plugin', {
  action: 'process',
  data: { foo: 'bar' }
});
```

#### `DBHive.onMessage(handler)`
Listen for messages from other plugins.

```javascript
DBHive.onMessage((message) => {
  console.log(`Received from ${message.from}:`, message.message);
});
```

### Configuration

#### `DBHive.getConfig()`
Get the plugin's configuration.

```javascript
const config = DBHive.getConfig();
console.log(config.enabled);
```

## Best Practices

### 1. Error Handling

Always wrap async operations in try-catch blocks:

```javascript
async function myFunction() {
  try {
    const result = await DBHive.executeQuery(query, connectionId);
    // Process result
  } catch (error) {
    console.error('Query failed:', error);
    await DBHive.showNotification({
      title: 'Error',
      message: error.message,
      notificationType: 'error'
    });
  }
}
```

### 2. Performance

- Avoid blocking operations
- Use async/await for all I/O operations
- Cache frequently accessed data
- Clean up resources in `onUnload`

### 3. Security

- Never store sensitive data in plain text
- Validate all user input
- Use the permission system properly
- Don't request permissions you don't need

### 4. User Experience

- Provide clear error messages
- Show progress for long operations
- Respect user preferences
- Provide documentation

### 5. Compatibility

- Test with different DB-Hive versions
- Handle missing features gracefully
- Specify accurate version requirements

## Testing Plugins

### Local Development

1. Create a `plugins` directory in your DB-Hive installation
2. Copy your plugin folder to the plugins directory
3. Restart DB-Hive
4. Your plugin should appear in the Plugin Manager

### Testing Checklist

- [ ] Plugin loads without errors
- [ ] All permissions work correctly
- [ ] UI components render properly
- [ ] Error handling works
- [ ] Memory leaks are avoided
- [ ] Plugin unloads cleanly

### Debugging

Use console.log for debugging:

```javascript
console.log('[MyPlugin] Debug info:', data);
```

Check the developer console for errors:
- Windows/Linux: `Ctrl+Shift+I`
- macOS: `Cmd+Option+I`

## Publishing Plugins

### 1. Prepare for Release

- Test thoroughly
- Update version number
- Write comprehensive README
- Create icon (SVG recommended)
- Add screenshots if applicable

### 2. Package Plugin

Create a ZIP file containing all plugin files:

```bash
zip -r my-plugin-1.0.0.zip my-plugin/
```

### 3. Submit to Marketplace

1. Fork the [DB-Hive Plugins Repository](https://github.com/dbhive/plugins)
2. Add your plugin to the `plugins` directory
3. Update the `marketplace.json` file
4. Create a pull request

### 4. Marketplace Requirements

- Unique plugin ID
- Clear description
- Proper categorization
- Valid manifest.json
- MIT or compatible license
- No malicious code
- Respect user privacy

## Examples

### Example 1: Query Formatter

```javascript
async function formatQuery() {
  const query = await DBHive.clipboard.read();
  const formatted = beautifySQL(query);
  await DBHive.clipboard.write(formatted);

  await DBHive.showNotification({
    title: 'SQL Formatted',
    message: 'Query has been formatted and copied to clipboard',
    notificationType: 'success'
  });
}
```

### Example 2: Data Exporter

```javascript
async function exportToJSON() {
  const result = await DBHive.executeQuery(
    'SELECT * FROM products',
    'current-connection'
  );

  const json = JSON.stringify(result.rows, null, 2);
  await DBHive.writeFile('export.json', json);

  await DBHive.showNotification({
    title: 'Export Complete',
    message: `Exported ${result.rows.length} rows`,
    notificationType: 'success'
  });
}
```

### Example 3: Custom Theme

```javascript
async function applyTheme() {
  await DBHive.registerUiComponent({
    id: 'custom-theme',
    location: 'head',
    componentType: {
      type: 'style',
      data: {
        css: `
          :root {
            --primary: #6366f1;
            --secondary: #8b5cf6;
            --accent: #ec4899;
          }
        `
      }
    }
  });
}
```

## Support

For help with plugin development:

- [Plugin Development Forum](https://github.com/dbhive/dbhive/discussions)
- [API Documentation](https://docs.dbhive.com/plugins)
- [Example Plugins](https://github.com/dbhive/example-plugins)
- [Discord Community](https://discord.gg/dbhive)

## License

Plugins should use MIT or a compatible open-source license.