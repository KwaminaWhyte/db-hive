# TanStack Router Testing Checklist

**Date:** 2025-11-21
**Branch:** feature/tanstack-router
**Status:** Ready for Manual Testing

---

## ✅ Phase 1-3 Navigation Testing

### Test 1: Welcome Screen Navigation
**Route:** `/`

- [ ] Page loads without errors
- [ ] Theme toggle visible in top-right
- [ ] Settings button visible in top-right
- [ ] Click "New Connection" → navigates to `/connections/new`
- [ ] Click "Recent Connections" → navigates to `/connections`
- [ ] Click Settings button → navigates to `/settings`
- [ ] DB-Hive logo and welcome text visible

---

### Test 2: Settings Page
**Route:** `/settings`

- [ ] Settings page loads
- [ ] Back button visible (top-left)
- [ ] Theme toggle visible (top-right)
- [ ] Click back button → navigates to `/`
- [ ] All settings sections render correctly
- [ ] Settings can be changed and saved

---

### Test 3: Connections List
**Route:** `/connections`

- [ ] Connections page loads
- [ ] Settings button visible (top-right)
- [ ] Theme toggle visible
- [ ] Connection list sidebar visible (left side)
- [ ] Placeholder message visible (right side): "Select a connection to get started"
- [ ] "New Connection" button in list
- [ ] Click "New Connection" → navigates to `/connections/new`
- [ ] Click Settings → navigates to `/settings`

---

### Test 4: New Connection Form
**Route:** `/connections/new`

- [ ] Form loads without errors
- [ ] Back button visible (top-left) - "Back"
- [ ] Settings button visible (top-right)
- [ ] Theme toggle visible
- [ ] ConnectionForm component renders
- [ ] All form fields visible (driver, host, port, etc.)
- [ ] Click back button → navigates to `/connections`
- [ ] Fill form and save → navigates back to `/connections`

---

### Test 5: Edit Connection Form
**Route:** `/connections/$profileId/edit`

**Prerequisites:** Must have at least one saved connection

- [ ] From `/connections`, click edit button (✏️) on a profile
- [ ] Route changes to `/connections/{id}/edit`
- [ ] Back button visible (top-left)
- [ ] Settings button visible (top-right)
- [ ] Form loads with existing connection data pre-filled
- [ ] Driver dropdown shows correct value
- [ ] Host, port, database fields populated
- [ ] Username field populated (if saved)
- [ ] Click back button → navigates to `/connections`
- [ ] Update values and save → navigates back to `/connections`

---

### Test 6: Browser Navigation
**Test browser back/forward buttons**

Sequence:
1. Start at `/` (Welcome)
2. Click "Recent Connections" → `/connections`
3. Click "New Connection" → `/connections/new`
4. Click back button → `/connections`
5. Press browser back button → should return to `/`
6. Press browser forward button → should go to `/connections`
7. Press browser forward button again → should go to `/connections/new`

- [ ] Browser back button works correctly
- [ ] Browser forward button works correctly
- [ ] Each navigation updates URL in address bar
- [ ] No console errors during navigation

---

### Test 7: Direct URL Access (Deep Linking)
**Test typing URLs directly in browser**

- [ ] Open dev server: `http://localhost:1420/`
- [ ] Type `/settings` in address bar → loads settings page
- [ ] Type `/connections` in address bar → loads connections list
- [ ] Type `/connections/new` in address bar → loads new connection form
- [ ] Type `/connections/invalid-id/edit` in address bar → shows error or redirects

---

### Test 8: Page Refresh
**Test that routes persist after refresh**

1. Navigate to `/connections`
2. Press F5 or Ctrl+R to refresh
   - [ ] Page reloads at `/connections`
   - [ ] Connection list visible

3. Navigate to `/connections/new`
4. Press F5 to refresh
   - [ ] Page reloads at `/connections/new`
   - [ ] Form still visible

5. Navigate to `/settings`
6. Press F5 to refresh
   - [ ] Page reloads at `/settings`
   - [ ] Settings page visible

---

### Test 9: Theme Persistence
**Test theme toggle across routes**

- [ ] Set theme to dark on `/` → switch to `/connections` → theme still dark
- [ ] Set theme to light on `/settings` → switch to `/` → theme still light
- [ ] Refresh page → theme persists
- [ ] Theme toggle works on all routes

---

### Test 10: Console Errors
**Check browser console throughout testing**

- [ ] No React errors in console
- [ ] No TanStack Router warnings
- [ ] No 404 errors
- [ ] No "useConnectionContext" errors
- [ ] DevTools panel visible in bottom-right (dev mode only)

---

## 🐛 Known Limitations (Expected Behavior)

1. **Clicking on a connection does nothing**
   - ✅ Expected - `/query` route not implemented yet (Phase 4)
   - Logs "Connected: {id}" to console

2. **No connected routes available**
   - ✅ Expected - Will be implemented in Phase 4:
     - `/query` - Query panel
     - `/table/{schema}/{tableName}` - Table inspector
     - `/er-diagram/{schema}` - ER diagram

3. **Bundle size warning**
   - ✅ Expected - Will optimize with code splitting in Phase 6

---

## 📝 Issues Found

| Issue # | Route | Description | Severity | Status |
|---------|-------|-------------|----------|--------|
| | | | | |

**Instructions for logging issues:**
1. Add row with issue number
2. Note which route/page
3. Describe what went wrong
4. Mark severity (Low/Medium/High/Critical)
5. Leave status empty for now

---

## ✅ Sign-Off

**Tester:** _______________
**Date:** _______________
**Build:** feature/tanstack-router @ commit `3b8023e`

**Overall Status:**
- [ ] All tests passing
- [ ] Minor issues found (document above)
- [ ] Major issues found (stop and debug)

**Ready to proceed to Phase 4:** Yes / No

---

## 🚀 How to Test

### Start Dev Server
```bash
cd /home/kwamina/Desktop/others/db-hive
bun run dev
```

### Open in Browser
```
http://localhost:1420
```

### Open Browser DevTools
Press F12 to see:
- Console (check for errors)
- Network tab (check for failed requests)
- TanStack Router DevTools (bottom-right panel)

### Test Navigation
Click through all routes and verify they work as expected per checklist above.

---

## 📊 Phase Progress

**Completed:**
- ✅ Phase 1: Setup & Configuration
- ✅ Phase 2: Core Infrastructure
- ✅ Phase 3: Settings & Connections Routes

**Next:**
- ⏳ Phase 4: Connected Routes (after testing passes)
- ⏳ Phase 5: Component Refactoring
- ⏳ Phase 6: Testing & Cleanup
