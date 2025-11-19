# DB-Hive v0.1.0-mvp Release Notes

**Release Date**: November 19, 2025
**Version**: 0.1.0-mvp
**Status**: Production Ready

## 🎉 Welcome to DB-Hive

DB-Hive is a modern, cross-platform database client built with Tauri 2.0 and React 19. This MVP release provides a solid foundation for managing PostgreSQL, MySQL, and SQLite databases with a professional user interface.

## 🚀 What's New

### Core Features

#### Multi-Database Support

- **PostgreSQL**: Full support with metadata queries, connection pooling
- **MySQL/MariaDB**: Complete driver implementation with MySQL-specific features
- **SQLite**: Local database management with file picker

#### Connection Management

- Create, edit, and delete connection profiles
- Test connections before saving
- Secure password persistence (stored locally)
- Quick connect from saved profiles
- Database dropdown to switch between databases on the same server

#### SQL Editor

- **Monaco Editor**: VS Code's editor with SQL syntax highlighting
- **Multi-statement execution**: Run multiple queries separated by semicolons
- **Keyboard shortcuts**: Ctrl+Enter to execute, Ctrl+F to find, Ctrl+/ to comment
- **Execution time tracking**: See how long queries take to run
- **Multi-cursor editing**: Alt+Click for multiple cursors

#### Schema Browser

- Browse databases, schemas, and tables in a tree view
- View table metadata:
  - **Data tab**: Sample data with pagination (20 rows per page)
  - **Columns tab**: Column definitions, types, nullability, defaults, primary keys
  - **Indexes tab**: Index information with unique and primary key indicators
- **Context menus**:
  - Generate SELECT query with database-specific quoting
  - Generate INSERT template
  - Copy table name
  - Refresh table data

#### Results Viewer

- **Three view modes**:
  - **Grid**: Sortable table with click-to-copy cells
  - **JSON**: Pretty-printed JSON array
  - **Raw**: Tab-delimited text for spreadsheets
- **Copy operations**:
  - Click any cell to copy its value
  - Copy entire rows (tab-separated)
  - Copy entire columns (newline-separated)
- **Export options**:
  - Export to CSV (properly escaped for Excel)
  - Export to JSON (pretty-printed)
- **NULL handling**: Clear visual indicators for NULL values

#### Query Management

- **Query History**: Automatically saves all executed queries
  - Timestamp, row count, execution time
  - Success/failure status
  - Search through history
  - Click to reload into editor
- **Query Snippets**: Save frequently used queries
  - Name and description
  - One-click loading
  - Delete when no longer needed

#### Professional UI/UX

- **Theme Support**: Dark, Light, or System (follows OS preference)
- **Toast Notifications**: Non-intrusive feedback (using Sonner)
- **Error Handling**: ErrorBoundary catches React errors gracefully
- **Loading States**: Skeleton components for smooth UX
- **Welcome Screen**: DB-Hive logo and welcome message on startup
- **Responsive Layout**: Resizable panels, works on any screen size

## 📊 Technical Specifications

### Frontend

- React 19 with TypeScript
- Tauri 2.0 for native desktop functionality
- Monaco Editor for SQL editing
- TanStack Table v8 for virtualized data grids
- shadcn/ui + TailwindCSS for modern UI components
- Vite for fast development and builds

### Backend

- Rust with Tokio async runtime
- Database drivers:
  - `tokio-postgres` for PostgreSQL
  - `mysql_async` for MySQL/MariaDB
  - `rusqlite` for SQLite
- `tauri-plugin-store` for persistent storage
- `serde` for serialization

### Architecture

- Multi-process design (Rust core + React WebView)
- IPC communication via Tauri commands
- Thread-safe state management with `Mutex<AppState>`
- Async operations throughout for responsive UI

## 📚 Documentation

This release includes comprehensive documentation:

### User Documentation

- **README.md**: Complete overview, installation, usage examples
- **USER_GUIDE.md**: 533-line step-by-step tutorial covering:
  - Getting started
  - Connection setup for each database
  - SQL editor features
  - Schema browsing
  - Query management
  - Tips, tricks, and keyboard shortcuts
  - Common workflows
  - Troubleshooting

### Developer Documentation

- **CLAUDE.md**: Project overview for Claude Code
- **docs/implementation-roadmap.md**: Detailed development plan
- **CHANGELOG.md**: Version history and changes

## 🐛 Bug Fixes

- Fixed connection editing returning "duplicate ID" error
- Fixed form not updating when switching between profiles
- Toast notifications now follow theme mode
- Improved error messages with ErrorBoundary

## ⚠️ Known Limitations

- MongoDB and SQL Server drivers not yet implemented
- SSH tunneling not available
- Passwords stored in plaintext (will use OS keyring in future)
- No advanced SQL autocomplete yet
- No table data inline editing
- No query plan visualizer
- No ER diagram generator

## 🔒 Security

- All user input validated before SQL execution
- No SQL injection vulnerabilities detected
- Connection credentials stored locally (not transmitted)
- Future releases will implement OS keyring for secure password storage

## 📦 Installation

### Prerequisites

- Node.js 18+ (Bun recommended)
- Rust 1.70+
- Git

### Build from Source

```bash
# Clone the repository
git clone https://github.com/KwaminaWhyte/db-hive.git
cd db-hive

# Install dependencies
bun install

# Run in development mode
npm run tauri dev

# Build for production
bun run build
npm run tauri build

# Output will be in src-tauri/target/release/bundle/
```

## 🎯 Usage Quick Start

1. **Launch DB-Hive** - You'll see the welcome screen with the logo
2. **Create a Connection**:
   - Click "New Connection"
   - Fill in your database details
   - Click "Test Connection" to verify
   - Click "Save"
3. **Connect**: Click the "Connect" button on your saved connection
4. **Explore**: Browse tables in the Schema Explorer
5. **Query**: Write SQL in the Query Editor, press Ctrl+Enter to execute
6. **View Results**: See results in Grid, JSON, or Raw format
7. **Export**: Click CSV or JSON to export results

## 🗺️ Roadmap

### Next Release (v0.2.0)

- MongoDB database driver
- SQL Server database driver
- SSH tunneling support
- Advanced SQL autocomplete
- Table data inline editing

### Future Releases

- Query plan visualizer
- ER diagram generator
- Plugin system
- Cloud database support
- Team collaboration features

See [docs/implementation-roadmap.md](implementation-roadmap.md) for the complete roadmap.

## 🤝 Contributing

We welcome contributions! Please:

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

## 📝 License

MIT License - See [LICENSE](../LICENSE) file for details

## 🙏 Acknowledgments

Built with amazing technologies:

- [Tauri](https://tauri.app/) - Lightweight desktop framework
- [React](https://react.dev/) - UI library
- [Monaco Editor](https://microsoft.github.io/monaco-editor/) - VS Code's editor
- [TanStack Table](https://tanstack.com/table/) - Powerful data grids
- [shadcn/ui](https://ui.shadcn.com/) - Beautiful UI components

Inspired by:

- [Beekeeper Studio](https://www.beekeeperstudio.io/)
- [DbGate](https://dbgate.org/)
- [DBeaver](https://dbeaver.io/)

## 📞 Support

- **Documentation**: See [README.md](../README.md) and [USER_GUIDE.md](USER_GUIDE.md)
- **Issues**: Report bugs at [GitHub Issues](https://github.com/KwaminaWhyte/db-hive/issues)
- **Discussions**: Ask questions in [GitHub Discussions](https://github.com/KwaminaWhyte/db-hive/discussions)

---

**Thank you for using DB-Hive!**

Built with ❤️ using Claude Code
