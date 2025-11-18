# DB-Hive

> A modern, cross-platform database client built with Tauri and React

## Project Status

**Planning Phase** - Documentation and architecture complete. Ready for development!

## Overview

DB-Hive is a professional database client application designed to compete with industry tools like Beekeeper Studio and DbGate. Built with security, performance, and developer experience in mind.

### Key Features (Planned)

- **Multi-Database Support**: PostgreSQL, MySQL/MariaDB, SQLite, MongoDB, SQL Server
- **Modern SQL Editor**: Monaco editor with syntax highlighting and autocomplete
- **Secure Credentials**: OS keyring integration with optional master passphrase
- **SSH Tunneling**: Connect to databases through secure SSH tunnels
- **Performance**: Virtualized tables, streaming results, efficient memory usage
- **Cross-Platform**: Windows, macOS, and Linux support
- **Lightweight**: Small binary size (~10MB) using system webviews

## Technology Stack

### Frontend
- **React 19** with TypeScript
- **Tauri 2.0** for native functionality
- **Zustand** for state management
- **Monaco Editor** for SQL editing
- **TanStack Table v8** for virtualized data grids
- **TailwindCSS** + shadcn/ui for styling

### Backend
- **Rust** with Tokio async runtime
- **Database Drivers**: tokio-postgres, mysql_async, rusqlite, mongodb
- **Secure Storage**: OS keyring via `keyring` crate
- **Error Handling**: thiserror for typed errors

## Project Structure

```
db-hive/
├── .claude/
│   ├── agents/          # Specialized sub-agents
│   │   ├── rust-backend-dev.md
│   │   ├── react-ui-dev.md
│   │   ├── db-driver-specialist.md
│   │   ├── test-engineer.md
│   │   └── tech-writer.md
│   └── skills/          # Reusable skills
│       ├── tauri-command/
│       ├── database-driver/
│       └── react-component/
├── docs/
│   ├── base-plan.md              # Original concept
│   ├── difficulty.md             # Complexity analysis
│   ├── implementation-roadmap.md # Detailed action plan
│   └── tauri/                    # Tauri documentation
├── src/                 # React frontend
├── src-tauri/          # Rust backend
└── README.md
```

## Documentation

### Core Documents

- **[Implementation Roadmap](docs/implementation-roadmap.md)** - Complete development plan with actionable steps
- **[Base Plan](docs/base-plan.md)** - Original architecture and design
- **[Difficulty Assessment](docs/difficulty.md)** - Complexity analysis and time estimates

### Sub-Agents

The project uses 5 specialized Claude Code sub-agents for different development tasks. See [CLAUDE.md](CLAUDE.md#sub-agents-system) for detailed descriptions, responsibilities, and usage examples:

- **rust-backend-dev** - Tauri commands, database drivers, async operations
- **react-ui-dev** - React components, state management, UI integration
- **db-driver-specialist** - Database-specific implementations and metadata
- **test-engineer** - Testing strategies and quality assurance
- **tech-writer** - Documentation and user guides

### Skills

Reusable code generation patterns. See [CLAUDE.md](CLAUDE.md#skills-system) for detailed usage:

- **tauri-command** - Generate Tauri command boilerplate
- **database-driver** - Implement database driver interfaces
- **react-component** - Create React components with best practices

## Development Roadmap

### Phase 0: Setup & Architecture (Weeks 1-2)
- [x] Project structure and documentation
- [x] Sub-agents and skills setup
- [ ] Development environment configuration
- [ ] Core infrastructure design

### Phase 1: MVP (Weeks 3-14)
- [ ] Connection management
- [ ] PostgreSQL and SQLite drivers
- [ ] SQL editor with Monaco
- [ ] Query execution and results
- [ ] Schema browser
- [ ] Query history and snippets

### Phase 2: Advanced Features (Weeks 15-28)
- [ ] MySQL/MariaDB and MongoDB support
- [ ] SSH tunneling
- [ ] Advanced autocomplete
- [ ] Table editor
- [ ] Query plan visualizer
- [ ] ER diagram generator

### Phase 3: Enterprise (Ongoing)
- [ ] Plugin system
- [ ] Workspace sync
- [ ] Visual query builder
- [ ] Schema migration tools
- [ ] AI assistant integration

## Getting Started

### Prerequisites

- Node.js 18+ and npm/yarn/pnpm
- Rust 1.70+
- Git

### Development

```bash
# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build

# Run tests
npm test && cd src-tauri && cargo test
```

## Using Sub-Agents with Claude Code

When developing with Claude Code, you can leverage specialized sub-agents. See [CLAUDE.md](CLAUDE.md#sub-agents-system) for complete details on each agent's responsibilities and when to use them.

**Example invocations**:
```
"Use rust-backend-dev to implement a Tauri command for creating database connections"
"Use react-ui-dev to create a connection form component with validation"
"Use db-driver-specialist to implement the PostgreSQL driver with metadata queries"
"Use test-engineer to write tests for the connection manager commands"
"Use tech-writer to document the database driver API with examples"
```

**Skills for code generation**:
```
"Use tauri-command skill to generate a command for executing queries"
"Use database-driver skill to implement the MySQL driver interface"
"Use react-component skill to create a connection list component"
```

For detailed information on all sub-agents and skills, refer to [CLAUDE.md](CLAUDE.md).

## Architecture Highlights

### Multi-Process Design
- **Core Process (Rust)**: Database connections, state management, credentials
- **WebView Process (React)**: UI rendering and user interactions
- **IPC Communication**: Tauri Commands and Events for data flow

### Security Model
- Credentials stored in OS keyring
- Optional master passphrase encryption
- TLS/SSL support for database connections
- SSH tunneling for remote databases

### Performance Optimization
- Result streaming to handle large datasets
- Virtualized tables for smooth scrolling
- Connection pooling for efficiency
- Async operations throughout

## Contributing

**Note**: Project is in planning/early development phase. Contribution guidelines will be finalized as development progresses.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/)
- [Tauri Extension](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode)
- [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
- [ESLint](https://marketplace.visualstudio.com/items?itemName=dbaeumer.vscode-eslint)
- [Prettier](https://marketplace.visualstudio.com/items?itemName=esbenp.prettier-vscode)

## License

MIT License - See LICENSE file for details

## Acknowledgments

Inspired by excellent tools like:
- [Beekeeper Studio](https://www.beekeeperstudio.io/)
- [DbGate](https://dbgate.org/)
- [DBeaver](https://dbeaver.io/)

Built with amazing technologies:
- [Tauri](https://tauri.app/)
- [React](https://react.dev/)
- [Monaco Editor](https://microsoft.github.io/monaco-editor/)
- [TanStack Table](https://tanstack.com/table/)

---

**Status**: 📋 Planning Complete → Next: 🚀 Begin Development

For detailed implementation steps, see [Implementation Roadmap](docs/implementation-roadmap.md).
