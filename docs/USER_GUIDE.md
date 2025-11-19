# DB-Hive User Guide

Complete guide to using DB-Hive database client.

## Table of Contents

- [Getting Started](#getting-started)
- [Managing Connections](#managing-connections)
- [Using the SQL Editor](#using-the-sql-editor)
- [Browsing Your Database](#browsing-your-database)
- [Working with Results](#working-with-results)
- [Query History and Snippets](#query-history-and-snippets)
- [Tips and Tricks](#tips-and-tricks)
- [Keyboard Shortcuts](#keyboard-shortcuts)

## Getting Started

### First Launch

When you first launch DB-Hive, you'll see the Connections screen. This is where you manage all your database connections.

### Creating Your First Connection

1. Click the **"New Connection"** button in the top-right
2. Fill in the connection form (see [Connection Setup](#connection-setup) below)
3. Click **"Test Connection"** to verify it works
4. Click **"Save"** to store the connection

### Connection Setup

#### PostgreSQL

```
Connection Name: My PostgreSQL DB
Driver: PostgreSQL
Host: localhost
Port: 5432
Database: myapp
Username: postgres
Password: your_password
```

**Common PostgreSQL ports:**
- Default: 5432
- Custom installs may use 5433, 5434, etc.

#### MySQL/MariaDB

```
Connection Name: My MySQL DB
Driver: MySQL
Host: localhost
Port: 3306
Database: myapp
Username: root
Password: your_password
```

**Common MySQL ports:**
- MySQL: 3306
- MariaDB: 3306
- Custom: 3307, 3308, etc.

#### SQLite

```
Connection Name: My Local Database
Driver: SQLite
Database File: /path/to/database.db
```

Click **"Browse"** to select your SQLite file. No host, port, username, or password needed.

**Note**: SQLite files typically end in `.db`, `.sqlite`, or `.sqlite3`

## Managing Connections

### Connecting to a Database

1. Find your connection in the list
2. Click the **"Connect"** button (plug icon)
3. The Schema Explorer will appear on the left
4. The Query Editor will open on the right

### Editing a Connection

1. Click the **"Edit"** button (pencil icon) next to a connection
2. Update the connection details
3. Test and save

### Deleting a Connection

1. Click the **"Delete"** button (trash icon) next to a connection
2. Confirm the deletion
3. **Note**: This only deletes the connection profile, not your actual database!

### Disconnecting

Click the **"Disconnect"** button at the top of the Schema Explorer to close the connection and return to the Connections screen.

## Using the SQL Editor

### Writing Queries

The SQL Editor uses Monaco Editor (the same editor as VS Code) with full SQL syntax highlighting.

**Features:**
- Syntax highlighting for SQL
- Auto-indentation
- Bracket matching
- Multi-cursor editing (Alt+Click)
- Find and replace (Ctrl+F)

### Executing Queries

**Single Statement:**
```sql
SELECT * FROM users WHERE active = true;
```

Press **Ctrl+Enter** or click **"Execute"**

**Multiple Statements:**
```sql
SELECT * FROM users;
SELECT * FROM orders;
SELECT * FROM products;
```

All statements will be executed in sequence. Results from the last SELECT will be shown.

**Note**: Use semicolons to separate statements

### Statement Types

**SELECT Queries:**
- Results appear in the Results Viewer
- Can sort, filter, and export data

**DML Statements** (INSERT, UPDATE, DELETE):
- Shows "X rows affected" message
- No result grid displayed

**DDL Statements** (CREATE, ALTER, DROP):
- Shows success/error message
- Use carefully! These modify your database structure

**Example DML:**
```sql
UPDATE users SET last_login = NOW() WHERE id = 123;
-- Shows: "1 row affected"

DELETE FROM temp_data WHERE created_at < '2024-01-01';
-- Shows: "452 rows affected"
```

## Browsing Your Database

### Schema Explorer

After connecting, the left panel shows:

**Top Section:**
- Database dropdown (switch between databases)
- Disconnect button

**Main Section:**
- List of all tables in the selected database
- Table icons (table or view)
- Row counts (when available)

### Viewing Table Data

Click on any table name to open the Table Inspector:

#### Data Tab
- Shows first 20 rows of data
- Pagination controls at bottom
- Double-click a row to see JSON details
- Click cells to copy values

#### Columns Tab
- All column definitions
- Data types (VARCHAR, INTEGER, etc.)
- Nullable indicators
- Primary key markers
- Default values

#### Indexes Tab
- All indexes on the table
- Index columns
- Unique constraints
- Primary key indexes

### Switching Databases

Use the database dropdown at the top of the Schema Explorer to switch between databases on the same server.

**Note**: This only works for PostgreSQL and MySQL. SQLite databases must be opened as separate connections.

## Working with Results

### View Modes

Results can be viewed in three modes:

**Grid View** (default):
- Sortable table
- Click headers to sort
- Click cells to copy
- Hover for copy buttons

**JSON View**:
- Pretty-printed JSON array
- Good for API responses
- Easy to copy entire structure

**Raw View**:
- Tab-delimited text
- Column headers on first line
- Good for spreadsheets
- Ready for pasting into Excel

### Copying Data

**Single Cell:**
1. Click any cell
2. Value is copied to clipboard
3. Toast notification confirms

**Entire Row:**
1. Hover over the row number
2. Click the copy button that appears
3. Row copied as tab-separated values

**Entire Column:**
1. Hover over column header
2. Click the copy button
3. Column copied as newline-separated values

**NULL Values:**
- NULL values are copied as the text "NULL"
- Empty strings are copied as empty

### Exporting Data

**CSV Export:**
1. Click the **CSV** button in results header
2. Choose save location
3. Properly escaped for Excel/Numbers
4. NULL values become empty cells

**JSON Export:**
1. Click the **JSON** button
2. Choose save location
3. Pretty-printed JSON array
4. NULL values become `null`

### Sorting Results

Click any column header to sort:
- First click: Ascending (↑)
- Second click: Descending (↓)
- Third click: Remove sorting

Multiple column sorting not currently supported.

## Query History and Snippets

### Query History

All executed queries are automatically saved to history.

**Viewing History:**
1. Open the History tab (bottom-right panel)
2. See recent queries with:
   - Timestamp
   - Row count (for SELECT)
   - Success/failure status
   - Execution time

**Loading from History:**
1. Click any history item
2. Query loads into editor
3. Modify and re-execute

**Searching History:**
- Type in the search box
- Filters by query text
- Case-insensitive

### Query Snippets

Save frequently used queries as snippets.

**Creating a Snippet:**
1. Write your query in the editor
2. Click **"Save as Snippet"**
3. Enter a name and description
4. Click **Save**

**Using Snippets:**
1. Open the Snippets tab
2. Click a snippet name
3. Query loads into editor

**Deleting Snippets:**
1. Click the delete button (×) next to a snippet
2. Confirm deletion

**Example Snippets:**
```sql
-- User Count by Status
SELECT status, COUNT(*) as count
FROM users
GROUP BY status;

-- Recent Orders
SELECT * FROM orders
WHERE created_at > NOW() - INTERVAL '7 days'
ORDER BY created_at DESC;

-- Table Size
SELECT
    schemaname,
    tablename,
    pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) AS size
FROM pg_tables
ORDER BY pg_total_relation_size(schemaname||'.'||tablename) DESC;
```

## Tips and Tricks

### Performance Tips

**Large Result Sets:**
- Always use LIMIT for exploratory queries
- Use WHERE clauses to filter data
- Consider pagination for millions of rows

**Example:**
```sql
-- Instead of this (slow):
SELECT * FROM large_table;

-- Do this (fast):
SELECT * FROM large_table LIMIT 100;

-- Or filter:
SELECT * FROM large_table
WHERE created_at > '2024-01-01'
LIMIT 100;
```

**Slow Queries:**
- Add indexes on frequently queried columns
- Use EXPLAIN to see query plan
- Avoid SELECT * in production queries

### Data Exploration

**Quick Stats:**
```sql
SELECT
    COUNT(*) as total_rows,
    COUNT(DISTINCT user_id) as unique_users,
    MIN(created_at) as oldest,
    MAX(created_at) as newest
FROM orders;
```

**Data Distribution:**
```sql
SELECT
    status,
    COUNT(*) as count,
    ROUND(100.0 * COUNT(*) / SUM(COUNT(*)) OVER (), 2) as percentage
FROM users
GROUP BY status
ORDER BY count DESC;
```

### Working with Multiple Databases

**Cross-Database Queries:**
- Not supported in a single query
- Execute separate queries per database
- Export and merge results externally

**Switching Context:**
1. Use database dropdown to switch
2. Or disconnect and connect to different database

### Keyboard Shortcuts

| Action | Shortcut |
|--------|----------|
| Execute Query | Ctrl+Enter |
| Find | Ctrl+F |
| Replace | Ctrl+H |
| Comment Line | Ctrl+/ |
| Indent | Tab |
| Outdent | Shift+Tab |
| Select All | Ctrl+A |
| Undo | Ctrl+Z |
| Redo | Ctrl+Y or Ctrl+Shift+Z |
| Multi-cursor | Alt+Click |

**Monaco Editor Tips:**
- Select text and press Ctrl+D to select next occurrence
- Alt+↑/↓ to move line up/down
- Ctrl+Shift+K to delete line
- Ctrl+L to select current line

### Theme Switching

Click the theme toggle button (sun/moon icon) in the top-right to switch between:
- Light mode
- Dark mode
- System (follows OS setting)

## Common Workflows

### Workflow 1: Exploring a New Database

1. Connect to the database
2. Browse tables in Schema Explorer
3. Click a table to see sample data
4. Check columns to understand structure
5. Write exploratory SELECT query
6. Save useful queries as snippets

### Workflow 2: Data Analysis

1. Write query in editor
2. Execute and view results
3. Sort by different columns
4. Copy interesting data
5. Export to CSV for further analysis
6. Save query to history

### Workflow 3: Database Maintenance

1. Write UPDATE/DELETE query
2. Add WHERE clause with LIMIT for safety
3. Execute to verify
4. Remove LIMIT and execute final query
5. Check "rows affected" message
6. Verify with SELECT query

**Example:**
```sql
-- Step 1: Preview what will be updated
SELECT * FROM users
WHERE last_login < '2023-01-01'
LIMIT 10;

-- Step 2: Update with limit
UPDATE users
SET status = 'inactive'
WHERE last_login < '2023-01-01'
LIMIT 10;

-- Step 3: Remove limit and run full update
UPDATE users
SET status = 'inactive'
WHERE last_login < '2023-01-01';

-- Step 4: Verify
SELECT status, COUNT(*) FROM users GROUP BY status;
```

## Troubleshooting

### Can't Connect

1. Verify database is running
2. Check firewall settings
3. Test with CLI tool (psql, mysql, sqlite3)
4. Verify credentials
5. Check network connectivity

### Query Errors

**Syntax Error:**
- Check SQL syntax for your database type
- Verify table and column names
- Check for missing commas or quotes

**Permission Denied:**
- User lacks required permissions
- Ask database admin for access
- Verify user has SELECT, INSERT, etc. rights

**Table Not Found:**
- Check if you're in the right database
- Verify table name spelling
- Check schema name (PostgreSQL)

### Performance Issues

**Slow Queries:**
- Add LIMIT clause
- Use WHERE to filter
- Create indexes
- Avoid complex JOINs without indexes

**App Freezing:**
- Limit result set size
- Close unused table tabs
- Restart application

## Best Practices

1. **Always use LIMIT** when exploring unfamiliar tables
2. **Test with WHERE** before running UPDATE/DELETE
3. **Save useful queries** as snippets for reuse
4. **Use transactions** for multi-step operations (BEGIN/COMMIT)
5. **Regular backups** before major changes
6. **Verify permissions** before sharing connection profiles
7. **Close connections** when done to free resources

## Getting Help

- Check this user guide
- Review [README.md](../README.md) for features
- See [Troubleshooting](../README.md#troubleshooting) section
- Report issues on GitHub

---

**Happy querying! 🚀**
