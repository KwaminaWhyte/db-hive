---
name: tauri-command
description: Generate Tauri command boilerplate with proper error handling, async support, and state management. Use when creating new Rust commands for frontend-backend communication.
allowed-tools: Read, Write, Edit, Grep
---

# Tauri Command Generator

This skill helps you create properly structured Tauri commands with error handling, state management, and async support.

## When to Use

Use this skill when:
- Creating a new Tauri command
- Adding a command to communicate with the frontend
- Need boilerplate for async commands with state
- Want to ensure proper error handling patterns

## Command Template Structure

### Basic Command
```rust
#[tauri::command]
fn command_name() -> Result<ReturnType, DbError> {
    // Implementation
    Ok(result)
}
```

### Async Command
```rust
#[tauri::command]
async fn command_name(param: String) -> Result<ReturnType, DbError> {
    // Async implementation
    Ok(result)
}
```

### Command with State
```rust
#[tauri::command]
async fn command_name(
    state: State<'_, Mutex<AppState>>,
    param: String,
) -> Result<ReturnType, DbError> {
    let state = state.lock().unwrap();
    // Access state
    Ok(result)
}
```

### Command with Streaming (Channel)
```rust
use tauri::ipc::Channel;

#[tauri::command]
async fn command_name(
    connection_id: String,
    on_event: Channel<EventType>,
) -> Result<(), DbError> {
    // Send events through channel
    on_event.send(event_data)?;
    Ok(())
}
```

## Full Example: Create Connection Command

```rust
use tauri::{State, Manager};
use std::sync::Mutex;
use serde::{Serialize, Deserialize};

/// Creates a new database connection and saves it to the profile store.
///
/// # Arguments
///
/// * `state` - Application state containing connection manager
/// * `profile` - Connection profile with credentials
///
/// # Returns
///
/// Returns the connection ID if successful
///
/// # Errors
///
/// Returns `DbError` if:
/// - Connection fails
/// - Credentials are invalid
/// - Profile validation fails
#[tauri::command]
async fn create_connection(
    state: State<'_, Mutex<AppState>>,
    profile: ConnectionProfile,
) -> Result<String, DbError> {
    // Validate profile
    profile.validate()?;

    // Create connection
    let connection = DatabaseConnection::connect(&profile).await?;

    // Generate ID
    let connection_id = uuid::Uuid::new_v4().to_string();

    // Store in state
    {
        let mut state = state.lock().unwrap();
        state.connections.insert(connection_id.clone(), connection);
    }

    // Save profile to storage
    save_connection_profile(&profile).await?;

    Ok(connection_id)
}

// Register in lib.rs
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![create_connection])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

## Error Handling Pattern

```rust
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl serde::Serialize for DbError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct ErrorPayload {
            kind: &'static str,
            message: String,
        }

        let payload = ErrorPayload {
            kind: match self {
                DbError::ConnectionFailed(_) => "connection",
                DbError::InvalidConfig(_) => "config",
                DbError::Io(_) => "io",
            },
            message: self.to_string(),
        };

        payload.serialize(serializer)
    }
}
```

## TypeScript Types

Generate corresponding TypeScript types:

```typescript
interface ConnectionProfile {
    id: string;
    name: string;
    driver: 'postgres' | 'mysql' | 'sqlite' | 'mongodb';
    host: string;
    port: number;
    username: string;
    database?: string;
}

interface DbError {
    kind: 'connection' | 'config' | 'io';
    message: string;
}

// Call command
import { invoke } from '@tauri-apps/api/core';

try {
    const connectionId = await invoke<string>('create_connection', {
        profile: {
            name: 'My DB',
            driver: 'postgres',
            host: 'localhost',
            port: 5432,
            username: 'user',
        },
    });
    console.log('Connected:', connectionId);
} catch (err) {
    const error = err as DbError;
    console.error(`${error.kind}: ${error.message}`);
}
```

## Checklist

When creating a new command:

1. [ ] Define in appropriate module (`src-tauri/src/commands/`)
2. [ ] Mark as `pub` if in module (not in `lib.rs`)
3. [ ] Use `#[tauri::command]` macro
4. [ ] Return `Result<T, E>` where `E` implements `Serialize`
5. [ ] Use `async` for I/O operations
6. [ ] Document with doc comments
7. [ ] Register in `lib.rs` with `generate_handler!`
8. [ ] Create corresponding TypeScript types
9. [ ] Write tests
10. [ ] Update API documentation

## Common Patterns

### Read from File System
```rust
#[tauri::command]
async fn read_file(path: String) -> Result<String, DbError> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| DbError::Io(e))?;
    Ok(content)
}
```

### Write to File System
```rust
#[tauri::command]
async fn write_file(path: String, content: String) -> Result<(), DbError> {
    tokio::fs::write(path, content)
        .await
        .map_err(|e| DbError::Io(e))?;
    Ok(())
}
```

### Access AppHandle
```rust
#[tauri::command]
async fn open_dialog(app: tauri::AppHandle) -> Result<Option<String>, DbError> {
    use tauri::api::dialog::blocking::FileDialogBuilder;

    let path = FileDialogBuilder::new()
        .pick_file();

    Ok(path.map(|p| p.to_string_lossy().to_string()))
}
```

## Testing Commands

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tauri::State;

    #[tokio::test]
    async fn test_create_connection() {
        let state = Mutex::new(AppState::default());
        let state_handle = State::from(&state);

        let profile = ConnectionProfile {
            name: "Test".to_string(),
            driver: DbDriver::Postgres,
            host: "localhost".to_string(),
            port: 5432,
            username: "test".to_string(),
            database: Some("test".to_string()),
        };

        let result = create_connection(state_handle, profile).await;
        assert!(result.is_ok());
    }
}
```

## Remember

- Always use `Result<T, E>` for error handling
- Use `async` for I/O operations
- Lock mutexes for shortest time possible
- Document all public commands
- Create corresponding TypeScript types
- Write tests for commands
