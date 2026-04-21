# Competitive Analysis: DB-Hive vs DB Pro (dbpro.app)

**Source:** https://www.dbpro.app/ — captured 2026-04-21.

This document enumerates features DB Pro ships that DB-Hive does not, to inform roadmap prioritization. Each gap is tagged with effort estimate and strategic value.

---

## 1. AI Features

DB Pro has a substantially broader AI footprint than our current AI Assistant (Milestone 3.14).

| Feature | DB-Hive status | Gap |
|---|---|---|
| Natural language → SQL | ✅ ship | Parity |
| Query explanation | ✅ ship | Parity |
| Query optimization | ✅ ship | Parity |
| Error fixing | ✅ ship | Parity |
| **AI dashboard generation** (prompt → chart + data) | ❌ missing | Requires dashboards first |
| **AI workflow generation** (prompt → automation) | ❌ missing | Requires workflows first |
| **AI insight suggestions** (proactive data summaries) | ❌ missing | Medium effort — wire into ResultsViewer |
| **AI chat history / favorite prompts** | ❌ missing | Small — localStorage or tauri-store |
| **Write-query guards** ("block destructive queries" LLM filter) | ❌ missing | Small — regex + confirm before DELETE/DROP/TRUNCATE without WHERE |
| AI provider — OpenRouter | ❌ missing | Small — add to existing provider trait |

---

## 2. Dashboards & Visualization

We have single-query charts (Milestone 3.7 data-viz) but no persistent dashboards.

| Feature | DB-Hive status | Gap |
|---|---|---|
| Ad-hoc chart on query result (bar / line / pie / scatter / area) | ✅ ship | Parity |
| **Saved dashboards** (multi-widget layout) | ❌ missing | Large — new storage model + layout editor |
| **Bar / pie / table widgets placed on a canvas** | ❌ missing | Medium once dashboard container exists |
| **Dashboard folders** (hierarchical organization) | ❌ missing | Small once dashboards exist |
| **Auto-refresh intervals per widget** | ❌ missing | Small |
| **Public dashboard links / share URL** | ❌ missing | Cloud-only — requires hosted backend; out of scope for desktop |
| **AI dashboard generation** (see §1) | ❌ missing | Requires dashboards first |

---

## 3. Query Management

| Feature | DB-Hive status | Gap |
|---|---|---|
| Query history auto-save | ✅ ship | Parity |
| Snippets | ✅ ship | Parity |
| Saved-query templates with parameters | ✅ ship (Query Templates) | Parity |
| **Saved queries with labels + descriptions** (richer than snippets) | Partial | Enhance Snippets metadata model |
| **Query folders** (nested organization) | ❌ missing | Small — SnippetSidebar already has list view; add folder tree |
| **Write-query guards** (pre-execute warnings for destructive SQL without WHERE) | ❌ missing | Small — static analysis in query submit path |
| **Global action bar** (Command Palette with DB-aware actions) | Partial (we have CommandPalette) | Extend to include table/query navigation verbs |

---

## 4. Collaboration & Cloud

This is the largest gap-by-category. DB-Hive is desktop-only; DB Pro's paid tiers target teams.

| Feature | DB-Hive status | Gap |
|---|---|---|
| Team creation | ❌ missing | Cloud service required |
| Shared connections / queries / dashboards | ❌ missing | Cloud service required |
| Real-time collaboration | ❌ missing | Cloud service + CRDT/OT layer |
| Role assignment (admin/member) | ❌ missing | Cloud service required |
| Upload-to-workspace flows | ❌ missing | Cloud service required |

**Strategic note:** Roadmap §"Workspace Sync" already flags this. Unblocking collaboration requires committing to a hosted backend (Postgres + Auth + E2E-encrypted sync). Decide cloud yes/no before scheduling.

---

## 5. Enterprise / Security

| Feature | DB-Hive status | Gap |
|---|---|---|
| SSH tunnel | ✅ ship | Parity |
| Local credential storage (OS keyring) | ✅ ship | Parity (better than DB Pro's file-based "local storage") |
| **Self-hosted / air-gapped deployment** | N/A (we're desktop-native already) | Parity through a different model |
| **SSO / SAML** | ❌ missing | Cloud-only |
| **Audit logging** (user actions, not just query log) | ❌ missing | Medium — extend ActivityLogger with user + event types |
| **Role-based access control (RBAC)** | ❌ missing | Cloud-only |
| **Query approval workflows** | ❌ missing | Cloud-only |
| **IP allowlisting** | ❌ missing | Cloud-only |
| **Custom data retention policies** | Partial (ActivityLogger has 7-day default) | Expose in Settings |

---

## 6. Productivity / UI

| Feature | DB-Hive status | Gap |
|---|---|---|
| Multi-tab workflow | ✅ ship | Parity |
| Unlimited tabs | ✅ ship | Parity |
| **Table tags** (user-defined labels on tables) | ❌ missing | Medium — schema-explorer UI + tauri-store metadata |
| **Inspector** (record detail panel with relationships + metadata) | Partial (RowJsonViewer) | Enhance to show related rows via FKs |
| Form view for row editing | Partial (EditableCell inline) | Could add a proper form-modal for new rows |

---

## 7. Supported Databases

| DB | DB Pro | DB-Hive |
|---|---|---|
| PostgreSQL | ✅ | ✅ |
| MySQL / MariaDB | ✅ | ✅ |
| SQLite | ✅ | ✅ |
| MongoDB | ✅ | ✅ |
| SQL Server | ✅ | ✅ |
| Supabase | ✅ | ✅ |
| Neon | ✅ | ✅ |
| Turso | ✅ | ✅ (as of 2026-04-21) |
| **Redis** | ✅ | ❌ (Milestone 3.13 pending) |
| **ClickHouse** | ✅ | ❌ (not on roadmap) |
| **PlanetScale Postgres** | ✅ | ❌ (likely works via Postgres driver; untested/unlabeled) |

---

## 8. Platforms

| Platform | DB Pro | DB-Hive |
|---|---|---|
| macOS | ✅ | ✅ |
| Windows | ✅ | ✅ |
| Linux | ✅ | ✅ |
| **Web browser** | ✅ (Cloud tier) | ❌ (Tauri is desktop-only; WASM port is a major undertaking) |

---

## 9. Import / Export

| Feature | DB-Hive status | Gap |
|---|---|---|
| CSV / JSON import | ✅ ship | Parity |
| CSV / JSON export | ✅ ship | Parity |
| SQL dump/restore | ✅ ship | Parity (DB Pro doesn't list SQL dump; we're ahead) |
| Excel import (xlsx) | ✅ ship | Parity via calamine |

---

## Prioritized Gap List (quick-wins first)

Ordered by effort × strategic value. Pick from the top.

1. **Write-query guards** — small, high value, visible safety net. Flag destructive statements without WHERE before sending.
2. **Query folders** — small, organizational polish users ask for.
3. **Table tags** — medium, user-visible organization feature that differentiates well.
4. **OpenRouter AI provider** — small, broadens the AI offering.
5. **AI chat history + favorite prompts** — small, user-visible polish in the AI Assistant.
6. **Inspector with FK-related rows** — medium, high value for exploratory workflows.
7. **Saved dashboards** — large, but fundamental parity gap vs DB Pro. Build on existing ResultsChart.
8. **AI insight suggestions** — medium once dashboards exist.
9. **Audit-grade activity log** — medium, enterprise optionality; extend ActivityLogger.
10. **Redis / ClickHouse drivers** — medium per driver; checkboxes for the comparison table.

**Deliberately deferred** (require a cloud backend; strategic decision needed before scheduling): team collaboration, shared resources, SSO/SAML, RBAC, query approvals, IP allowlisting, public dashboard links, web browser platform.

---

## Notes on DB Pro's Pricing Model

DB Pro lists a one-time $49 license, $9.99/month subscription, $14/editor/month cloud tier, and custom enterprise. Strategically, DB-Hive positioning has to decide between: (a) free/open-source forever, (b) paid desktop + free tier, (c) paid cloud + free desktop. Out of scope for this document — flagged for product-level decision.
