//! Query history and snippet management commands
//!
//! This module provides Tauri commands for managing query execution history
//! and saved query snippets. History is automatically saved when queries are
//! executed, and snippets can be manually created and managed by users.

use crate::models::{DbError, QueryHistory, QuerySnippet};
use crate::state::AppState;
use std::sync::Mutex;
use tauri::{AppHandle, State};

// ============================================================================
// Query History Commands
// ============================================================================

/// Save a query to history
///
/// Adds a new query execution record to the history. This is typically called
/// automatically after a query is executed, capturing metadata like execution
/// time and row count.
///
/// # Arguments
///
/// * `history` - Query history record to save
/// * `state` - Application state
/// * `app` - Tauri application handle
///
/// # Returns
///
/// Result with the history entry ID if successful
///
/// # Frontend Usage
///
/// ```typescript
/// await invoke<string>('save_to_history', {
///   history: {
///     connectionId: 'conn-123',
///     connectionName: 'Production DB',
///     database: 'mydb',
///     query: 'SELECT * FROM users',
///     executedAt: new Date().toISOString(),
///     executionTimeMs: 150,
///     rowCount: 42,
///     success: true,
///   }
/// });
/// ```
#[tauri::command]
pub fn save_to_history(
    history: QueryHistory,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<String, DbError> {
    let history_id = history.id.clone();

    {
        let mut state = state.lock().unwrap();
        state.add_history(history);
    }

    // Save to persistent storage (drop lock before async call)
    let state = state.lock().unwrap();
    state.save_history_to_store(&app)?;

    Ok(history_id)
}

/// Get query history with optional filtering and pagination
///
/// Retrieves query history records, optionally filtered by connection ID
/// and/or limited to a certain number of recent entries.
///
/// # Arguments
///
/// * `connection_id` - Optional filter by connection ID
/// * `limit` - Optional limit on number of records (most recent first)
/// * `state` - Application state
///
/// # Returns
///
/// Vector of query history records, sorted by most recent first
///
/// # Frontend Usage
///
/// ```typescript
/// // Get all history
/// const allHistory = await invoke<QueryHistory[]>('get_query_history', {});
///
/// // Get last 50 entries
/// const recentHistory = await invoke<QueryHistory[]>('get_query_history', {
///   limit: 50
/// });
///
/// // Get history for specific connection
/// const connHistory = await invoke<QueryHistory[]>('get_query_history', {
///   connectionId: 'conn-123',
///   limit: 100
/// });
/// ```
#[tauri::command]
pub fn get_query_history(
    connection_id: Option<String>,
    limit: Option<usize>,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<QueryHistory>, DbError> {
    let state = state.lock().unwrap();

    let mut history = if let Some(conn_id) = connection_id {
        state.get_history_by_connection(&conn_id)
    } else {
        state.get_all_history()
    };

    // Sort by most recent first (newest executedAt first)
    history.sort_by(|a, b| b.executed_at.cmp(&a.executed_at));

    // Apply limit if specified
    if let Some(lim) = limit {
        history.truncate(lim);
    }

    Ok(history)
}

/// Clear query history
///
/// Removes query history entries. Can clear all history or just history
/// for a specific connection.
///
/// # Arguments
///
/// * `connection_id` - Optional filter to only clear history for specific connection
/// * `state` - Application state
/// * `app` - Tauri application handle
///
/// # Returns
///
/// Number of history entries removed
///
/// # Frontend Usage
///
/// ```typescript
/// // Clear all history
/// const removed = await invoke<number>('clear_history', {});
///
/// // Clear history for specific connection
/// const removed = await invoke<number>('clear_history', {
///   connectionId: 'conn-123'
/// });
/// ```
#[tauri::command]
pub fn clear_history(
    connection_id: Option<String>,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<usize, DbError> {
    let count;

    {
        let mut state = state.lock().unwrap();
        count = if let Some(conn_id) = connection_id {
            state.clear_history_by_connection(&conn_id)
        } else {
            state.clear_all_history()
        };
    }

    // Save to persistent storage
    let state = state.lock().unwrap();
    state.save_history_to_store(&app)?;

    Ok(count)
}

// ============================================================================
// Query Snippet Commands
// ============================================================================

/// Save a query snippet
///
/// Creates or updates a saved query snippet. If a snippet with the same ID
/// already exists, it will be updated; otherwise a new snippet is created.
///
/// # Arguments
///
/// * `snippet` - Query snippet to save
/// * `state` - Application state
/// * `app` - Tauri application handle
///
/// # Returns
///
/// The snippet ID (UUID)
///
/// # Frontend Usage
///
/// ```typescript
/// // Create new snippet
/// const snippetId = await invoke<string>('save_snippet', {
///   snippet: {
///     id: '', // Empty for new snippet
///     name: 'User Backup',
///     query: 'SELECT * FROM users',
///     description: 'Backup all users',
///     tags: ['backup', 'users'],
///   }
/// });
///
/// // Update existing snippet
/// await invoke<string>('save_snippet', {
///   snippet: {
///     id: 'existing-uuid',
///     name: 'Updated Name',
///     query: 'SELECT * FROM users WHERE active = true',
///     // ... other fields
///   }
/// });
/// ```
#[tauri::command]
pub fn save_snippet(
    mut snippet: QuerySnippet,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<String, DbError> {
    // Generate ID if not provided (new snippet)
    if snippet.id.is_empty() {
        snippet.id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        snippet.created_at = now.clone();
        snippet.updated_at = now;
    } else {
        // Update timestamp for existing snippet
        snippet.updated_at = chrono::Utc::now().to_rfc3339();
    }

    let snippet_id = snippet.id.clone();

    {
        let mut state = state.lock().unwrap();
        state.add_snippet(snippet);
    }

    // Save to persistent storage
    let state = state.lock().unwrap();
    state.save_snippets_to_store(&app)?;

    Ok(snippet_id)
}

/// List all saved query snippets
///
/// Retrieves all saved query snippets, optionally filtered by tags.
///
/// # Arguments
///
/// * `tag` - Optional filter by tag
/// * `state` - Application state
///
/// # Returns
///
/// Vector of query snippets, sorted alphabetically by name
///
/// # Frontend Usage
///
/// ```typescript
/// // Get all snippets
/// const snippets = await invoke<QuerySnippet[]>('list_snippets', {});
///
/// // Get snippets with specific tag
/// const backupSnippets = await invoke<QuerySnippet[]>('list_snippets', {
///   tag: 'backup'
/// });
/// ```
#[tauri::command]
pub fn list_snippets(
    tag: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<QuerySnippet>, DbError> {
    let state = state.lock().unwrap();

    let mut snippets = if let Some(t) = tag {
        state.get_snippets_by_tag(&t)
    } else {
        state.get_all_snippets()
    };

    // Sort alphabetically by name
    snippets.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(snippets)
}

/// Delete a query snippet
///
/// Removes a saved query snippet by ID.
///
/// # Arguments
///
/// * `snippet_id` - ID of snippet to delete
/// * `state` - Application state
/// * `app` - Tauri application handle
///
/// # Returns
///
/// Ok if successful, error if snippet not found
///
/// # Frontend Usage
///
/// ```typescript
/// await invoke('delete_snippet', {
///   snippetId: 'snippet-uuid'
/// });
/// ```
#[tauri::command]
pub fn delete_snippet(
    snippet_id: String,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<(), DbError> {
    {
        let mut state = state.lock().unwrap();
        state
            .remove_snippet(&snippet_id)
            .ok_or_else(|| DbError::NotFound(format!("Snippet not found: {}", snippet_id)))?;
    }

    // Save to persistent storage
    let state = state.lock().unwrap();
    state.save_snippets_to_store(&app)?;

    Ok(())
}

/// Get a specific snippet by ID
///
/// # Arguments
///
/// * `snippet_id` - ID of snippet to retrieve
/// * `state` - Application state
///
/// # Returns
///
/// The snippet if found
///
/// # Frontend Usage
///
/// ```typescript
/// const snippet = await invoke<QuerySnippet>('get_snippet', {
///   snippetId: 'snippet-uuid'
/// });
/// ```
#[tauri::command]
pub fn get_snippet(
    snippet_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<QuerySnippet, DbError> {
    let state = state.lock().unwrap();
    state
        .get_snippet(&snippet_id)
        .cloned()
        .ok_or_else(|| DbError::NotFound(format!("Snippet not found: {}", snippet_id)))
}
