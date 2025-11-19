# DB-Hive

> A modern, cross-platform database client built with Tauri 2.0 and React 19

![Version](https://img.shields.io/badge/version-0.1.0--mvp-blue)
![License](https://img.shields.io/badge/license-MIT-green)

## Overview

DB-Hive is a professional database client application designed to compete with industry tools like Beekeeper Studio and DbGate. Built with security, performance, and developer experience in mind, it provides a fast, native experience for database management.

## Features

### ✅ Implemented (MVP)

- **Multi-Database Support**

  - ✅ PostgreSQL with full metadata support
  - ✅ SQLite with local database management
  - ✅ MySQL/MariaDB with connection handling

- **Connection Management**

  - Save and manage multiple connection profiles
  - Test connections before saving
  - Password persistence using Tauri plugin-store
  - Quick connect from saved profiles

- **SQL Editor**

  - Monaco Editor with SQL syntax highlighting
  - Execute single or multiple SQL statements
  - Query history with automatic saving
  - Snippet management for reusable queries
  - Keyboard shortcuts (Ctrl+Enter to execute)

- **Schema Browser**

  - Browse databases, schemas, and tables
  - View table structure (columns, indexes, data types)
  - Table data preview with pagination
  - Sample data viewer with 20 rows per page
  - Row detail viewer (JSON format)

- **Results Viewer**

  - Multiple view modes (Grid, JSON, Raw)
  - Sortable columns with visual indicators
  - Click cells to copy values
  - Copy entire rows or columns
  - NULL value indicators
  - Export results to CSV or JSON

- **Query Management**

  - Auto-save query history
  - Create and manage snippets
  - Search through history
  - Load queries from history or snippets

- **UI/UX**
  - Dark/Light/System theme support
  - Responsive layout with resizable panels
  - Loading states and skeletons
  - Error boundary for graceful error handling
  - Toast notifications for user feedback

### 🚧 Planned Features

- MongoDB support
- SQL Server support
- SSH tunneling
- Advanced SQL autocomplete
- Table data editing
- Query plan visualizer
- ER diagram generator
- Plugin system

## Technology Stack

### Frontend

- React 19 with TypeScript
- Tauri 2.0 for native functionality
- Monaco Editor for SQL editing
- TanStack Table v8 for virtualized data grids
- shadcn/ui + TailwindCSS for styling
- Sonner for toast notifications

### Backend

- Rust with Tokio async runtime
- tokio-postgres for PostgreSQL
- mysql_async for MySQL/MariaDB
- rusqlite for SQLite
- tauri-plugin-store for persistence
- serde for serialization

## Getting Started

### Prerequisites

- **Node.js** 18+ (Bun recommended for faster installs)
- **Rust** 1.70+
- **Git**

### Installation

```bash
# Clone the repository
git clone https://github.com/KwaminaWhyte/db-hive.git
cd db-hive

# Install dependencies (using Bun, or use npm/yarn/pnpm)
bun install

# Run in development mode
npm run tauri dev
```

### Building for Production

```bash
# Build frontend
bun run build

# Build Tauri app
npm run tauri build

# Output will be in src-tauri/target/release/bundle/
# - Linux: .deb, .rpm, .AppImage
# - macOS: .dmg, .app
# - Windows: .msi, .exe installer
```

See [Code Signing Guide](docs/CODE_SIGNING.md) for signing binaries on all platforms.

## Usage

### Connecting to a Database

1. Click the **"New Connection"** button or navigate to the Connections tab
2. Fill in the connection details:
   - **Name**: A friendly name for this connection
   - **Driver**: Select PostgreSQL, MySQL, or SQLite
   - **Host**: Database server hostname (e.g., localhost)
   - **Port**: Database port (default: 5432 for PostgreSQL, 3306 for MySQL)
   - **Database**: Database name to connect to
   - **Username**: Your database username
   - **Password**: Your database password (saved securely)
3. Click **"Test Connection"** to verify
4. Click **"Save"** to store the connection profile

For SQLite:

- Click **"Browse"** to select your .db file
- No username/password required

### Writing and Executing Queries

1. Connect to a database from the Connections list
2. The Query Editor will open automatically
3. Write your SQL query in the Monaco editor
4. Press **Ctrl+Enter** or click **"Execute"** to run
5. View results in the Results panel (Grid/JSON/Raw tabs)

### Managing Query History and Snippets

**History**:

- All executed queries are automatically saved
- Click the **History** tab in the query panel
- Click any history item to load it into the editor

**Snippets**:

- Save frequently used queries as snippets
- Click **"Save as Snippet"** button
- Provide a name and optional description
- Access snippets from the Snippets tab

### Browsing Schema

1. After connecting, the Schema Explorer appears on the left
2. Use the database dropdown to switch databases
3. Click on a table to view:
   - **Data tab**: Sample data with pagination
   - **Columns tab**: Column definitions and data types
   - **Indexes tab**: Index information
4. Double-click a row to view JSON details

### Copying Data

- **Single Cell**: Click any cell to copy its value
- **Entire Row**: Hover over row number, click copy button
- **Entire Column**: Hover over column header, click copy button
- **Export**: Use CSV or JSON export buttons in results header

## Project Structure

```
db-hive/
├── .claude/              # Claude Code configuration
│   ├── agents/          # Specialized sub-agents
│   └── skills/          # Reusable code patterns
├── docs/                # Documentation
│   ├── implementation-roadmap.md
│   ├── base-plan.md
│   └── difficulty.md
├── src/                 # React frontend
│   ├── components/     # React components
│   ├── types/          # TypeScript type definitions
│   └── App.tsx         # Main application
├── src-tauri/          # Rust backend
│   ├── src/
│   │   ├── commands/   # Tauri command handlers
│   │   ├── drivers/    # Database drivers
│   │   ├── models/     # Data models
│   │   └── state/      # Application state
│   └── tauri.conf.json
└── README.md
```

## Development

### Running Tests

```bash
# Frontend tests (when implemented)
npm test

# Rust tests
cd src-tauri
cargo test
```

### Development Commands

```bash
# Run development server
npm run tauri dev

# Build frontend only
bun run build

# Build Rust backend only
cd src-tauri
cargo build

# Run Rust checks
cargo check

# Format code
cargo fmt
```

## Architecture

### Multi-Process Design

- **Core Process (Rust)**: Manages database connections, state, and credentials
- **WebView Process (React)**: Handles UI rendering and user interactions
- **IPC Communication**: Tauri Commands for frontend → backend, Events for backend → frontend

### Security Model

- Credentials stored using tauri-plugin-store (encrypted local storage)
- No sensitive data in frontend state
- SQL injection prevention through parameterized queries
- Connection validation before saving

### Performance

- Virtualized tables with TanStack Table for smooth scrolling
- Lazy loading of schema metadata
- Pagination for large datasets
- Async operations throughout (Tokio runtime)

## Configuration

### Database Connection Examples

**PostgreSQL**:

```
Host: localhost
Port: 5432
Database: mydb
Username: postgres
Password: ••••••••
```

**MySQL**:

```
Host: localhost
Port: 3306
Database: mydb
Username: root
Password: ••••••••
```

**SQLite**:

```
Database File: /path/to/database.db
```

## Troubleshooting

### Connection Issues

**Problem**: "Connection refused" or "Could not connect"

- Check that the database server is running
- Verify host and port are correct
- Ensure firewall allows the connection
- Test with `psql`, `mysql`, or `sqlite3` CLI first

**Problem**: "Authentication failed"

- Double-check username and password
- Verify user has access to the specified database
- Check database user permissions

### Query Execution Issues

**Problem**: Query syntax error

- Verify SQL syntax for your database type
- MySQL uses backticks (\`) for identifiers
- PostgreSQL uses double quotes (") for identifiers

**Problem**: Query takes too long

- Add LIMIT clause to limit result size
- Create indexes on frequently queried columns
- Use WHERE clauses to filter data

### Application Issues

**Problem**: App won't start

- Ensure all dependencies are installed: `bun install`
- Check Rust toolchain: `rustc --version`
- Clear build cache: `rm -rf src-tauri/target`

**Problem**: Theme not working

- Check system theme settings
- Try switching theme manually in app
- Reload application

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

## Roadmap

See [Implementation Roadmap](docs/implementation-roadmap.md) for detailed development plan.

**Current Status**: MVP Complete (v0.1.0)

**Next Priorities**:

- User documentation and guides
- End-to-end testing
- Bug fixes and polish
- First stable release

## License

MIT License - See [LICENSE](LICENSE) file for details

## Acknowledgments

Inspired by excellent tools:

- [Beekeeper Studio](https://www.beekeeperstudio.io/)
- [DbGate](https://dbgate.org/)
- [DBeaver](https://dbeaver.io/)

Built with amazing technologies:

- [Tauri](https://tauri.app/)
- [React](https://react.dev/)
- [Monaco Editor](https://microsoft.github.io/monaco-editor/)
- [TanStack Table](https://tanstack.com/table/)
- [shadcn/ui](https://ui.shadcn.com/)

## Support

For issues and feature requests, please use the [GitHub Issues](https://github.com/KwaminaWhyte/db-hive/issues) page.

---

**Built with ❤️ using Claude Code**
