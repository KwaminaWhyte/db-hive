# Empty State Visual Design Guide

## Visual Layout

```
┌─────────────────────────────────────────────────┐
│                                                 │
│                                                 │
│                   ╭─────────╮                   │
│                   │         │                   │
│                   │  ICON   │  ← Rounded, colored bg
│                   │         │     with hover effect
│                   ╰─────────╯                   │
│                                                 │
│               Empty State Title                 │  ← Bold, large
│                                                 │
│    A helpful message explaining why this is     │  ← Muted color
│    empty and what the user can do about it.     │     Multiple lines
│                                                 │
│          ┌──────────────┐  ┌──────────┐        │
│          │ Primary CTA  │  │ Optional │        │  ← Action buttons
│          └──────────────┘  └──────────┘        │
│                                                 │
│                                                 │
└─────────────────────────────────────────────────┘
     max-width: 448px (md breakpoint)
```

## Component Anatomy

### Icon Container
```
┌──────────────────────┐
│   ╭──────────────╮   │
│   │              │   │
│   │   [ICON]     │   │ ← 64x64px (md size)
│   │              │   │   Stroke width: 1.5
│   ╰──────────────╯   │
└──────────────────────┘
     Rounded-full
     Padding: 16px
     Background: muted/50
     Color: muted-foreground/70
     Hover: scale(1.05) + bg opacity
```

### Typography Hierarchy
```
┌─────────────────────────────────┐
│  Empty State Title               │ ← h3, font-semibold
│  text-lg (md size)               │   text-foreground
│  tracking-tight                  │
├─────────────────────────────────┤
│  This is the descriptive message │ ← p, text-base (md size)
│  that helps users understand     │   text-muted-foreground
│  what happened.                  │   leading-relaxed
└─────────────────────────────────┘
```

### Action Buttons
```
┌─────────────────────┐  ┌──────────────┐
│ [Icon] Action Label │  │ Second Action│
└─────────────────────┘  └──────────────┘
       Primary               Secondary
   variant: default      variant: outline
   shadow-sm             shadow-sm
   hover:shadow          hover:shadow
```

## Size Variants Comparison

### Small (sm)
```
┌────────────────────────┐
│      ╭────────╮        │
│      │ 48x48  │        │  Icon: 48px
│      ╰────────╯        │  Title: text-base
│   Title (base)         │  Message: text-sm
│  Message text (sm)     │  Padding: py-8
│   [Action Button]      │  Gap: gap-3
└────────────────────────┘
```

### Medium (md) - Default
```
┌──────────────────────────┐
│       ╭────────╮         │
│       │ 64x64  │         │  Icon: 64px
│       ╰────────╯         │  Title: text-lg
│    Title (lg)            │  Message: text-base
│  Message text (base)     │  Padding: py-12
│   [Action Button]        │  Gap: gap-4
└──────────────────────────┘
```

### Large (lg)
```
┌────────────────────────────┐
│        ╭────────╮          │
│        │ 80x80  │          │  Icon: 80px
│        ╰────────╯          │  Title: text-xl
│     Title (xl)             │  Message: text-lg
│  Message text (lg)         │  Padding: py-16
│   [Action Button]          │  Gap: gap-6
└────────────────────────────┘
```

## Color Themes

### NoConnectionsEmpty (Blue)
```
Light Mode:
╭─────────────────────╮
│  ╭───────────────╮  │
│  │   [Database]  │  │ ← bg-blue-50
│  │               │  │   text-blue-600
│  ╰───────────────╯  │
│  No Connections Yet │
╰─────────────────────╯

Dark Mode:
╭─────────────────────╮
│  ╭───────────────╮  │
│  │   [Database]  │  │ ← bg-blue-950/30
│  │               │  │   text-blue-400
│  ╰───────────────╯  │
│  No Connections Yet │
╰─────────────────────╯
```

### NoHistoryEmpty (Violet)
```
╭─────────────────────╮
│  ╭───────────────╮  │
│  │   [History]   │  │ ← bg-violet-50 / bg-violet-950/30
│  │               │  │   text-violet-600 / text-violet-400
│  ╰───────────────╯  │
│  No Query History   │
╰─────────────────────╯
```

### NoTablesEmpty (Emerald)
```
╭─────────────────────╮
│  ╭───────────────╮  │
│  │    [Table]    │  │ ← bg-emerald-50 / bg-emerald-950/30
│  │               │  │   text-emerald-600 / text-emerald-400
│  ╰───────────────╯  │
│  No Tables Found    │
╰─────────────────────╯
```

### NoSearchResultsEmpty (Amber)
```
╭─────────────────────╮
│  ╭───────────────╮  │
│  │   [SearchX]   │  │ ← bg-amber-50 / bg-amber-950/30
│  │               │  │   text-amber-600 / text-amber-400
│  ╰───────────────╯  │
│  No Results Found   │
╰─────────────────────╯
```

### NoResultsEmpty (Slate)
```
╭─────────────────────╮
│  ╭───────────────╮  │
│  │ [FileQuestion]│  │ ← bg-slate-50 / bg-slate-950/30
│  │               │  │   text-slate-600 / text-slate-400
│  ╰───────────────╯  │
│    No Results       │
╰─────────────────────╯
```

### NoDataEmpty (Cyan)
```
╭─────────────────────╮
│  ╭───────────────╮  │
│  │    [Inbox]    │  │ ← bg-cyan-50 / bg-cyan-950/30
│  │               │  │   text-cyan-600 / text-cyan-400
│  ╰───────────────╯  │
│     No Data         │
╰─────────────────────╯
```

## Animation Sequence

### Frame by Frame

```
Frame 1 (0ms):
┌─────────────────┐
│                 │  ← Container starts fading in
│                 │     and sliding up
│                 │
└─────────────────┘

Frame 2 (100ms):
┌─────────────────┐
│   ╭────────╮    │
│   │ [Icon] │    │  ← Icon zooms in and fades in
│   ╰────────╯    │
│                 │
└─────────────────┘

Frame 3 (200ms):
┌─────────────────┐
│   ╭────────╮    │
│   │ [Icon] │    │
│   ╰────────╯    │
│   Title Text    │  ← Content fades in and slides up
│   Message text  │
└─────────────────┘

Frame 4 (300ms):
┌─────────────────┐
│   ╭────────╮    │
│   │ [Icon] │    │
│   ╰────────╯    │
│   Title Text    │
│   Message text  │
│  [Action Button]│  ← Actions fade in and slide up
└─────────────────┘

Frame 5 (500-800ms):
┌─────────────────┐
│   ╭────────╮    │
│   │ [Icon] │    │  ← All animations complete
│   ╰────────╯    │     Component fully visible
│   Title Text    │
│   Message text  │
│  [Action Button]│
└─────────────────┘
```

## Hover States

### Icon Hover
```
Before:                    After:
╭──────────╮              ╭──────────╮
│          │              │          │
│  [Icon]  │  ──hover──>  │  [Icon]  │
│          │              │          │
╰──────────╯              ╰──────────╯
scale: 1                  scale: 1.05
bg: muted/50             bg: muted/70
transition: 300ms
```

### Button Hover
```
Before:                    After:
┌─────────────┐           ┌─────────────┐
│   Action    │ ─hover─>  │   Action    │
└─────────────┘           └─────────────┘
shadow: shadow-sm         shadow: shadow
transition: shadow
```

## Responsive Behavior

### Desktop (≥768px)
```
┌──────────────────────────────────────┐
│                                      │
│     ┌────────────────────────┐      │
│     │                        │      │
│     │   Empty State Content  │      │
│     │   (max-width: 448px)   │      │
│     │                        │      │
│     └────────────────────────┘      │
│                                      │
└──────────────────────────────────────┘
        Centered with margins
```

### Mobile (<768px)
```
┌────────────────────┐
│                    │
│  ╭────────╮        │
│  │ [Icon] │        │
│  ╰────────╯        │
│   Title Text       │
│  Message wraps     │
│  naturally on      │
│  smaller screens   │
│ [Action Button]    │
│ [Second Action]    │  ← Buttons may wrap
│                    │
└────────────────────┘
    Fluid width with
    horizontal padding
```

## Spacing System

```
Component Spacing (Medium size example):

py-12 ──→ ┌─────────────────────┐ ←── 48px top padding
          │                     │
gap-4 ──→ │   ╭────────╮        │
          │   │ [Icon] │        │
          │   ╰────────╯        │
          │        ↕ 16px       │ ←── gap-4 between sections
          │   Title Text        │
          │   Message text      │
          │        ↕ 16px       │
          │  [Action Button]    │
          │                     │
py-12 ──→ └─────────────────────┘ ←── 48px bottom padding

px-6 ←──→ │                     │ ←──→ px-6
     24px │   Content Area      │ 24px
          │   (max-w-md)        │
```

## Text Alignment

```
All text is center-aligned:

        ╭────────╮
        │ [Icon] │
        ╰────────╯
    Centered Title
A longer message that
  wraps to multiple
 lines, all centered
   [Action Button]
```

## Accessibility Features

### Focus States
```
┌─────────────────────┐
│  [Action Button]    │ ← Normal state
└─────────────────────┘

┌─────────────────────┐
│  [Action Button]    │ ← Focus state (Tab key)
└─────────────────────┘
       ↑
  3px ring
  ring-ring/50
  border-ring
```

### Screen Reader Structure
```
<div role="status">              ← Implicit status role
  <div>                          ← Icon (decorative)
    <Icon aria-hidden="true" />
  </div>
  <h3>                           ← Semantic heading
    No Connections Yet
  </h3>
  <p>                            ← Descriptive text
    Get started by...
  </p>
  <button>                       ← Accessible button
    Add Connection
  </button>
</div>
```

## Context-Specific Examples

### In a Full-Page Layout
```
┌────────────────────────────────────────┐
│ Header / Navigation                    │
├────────────────────────────────────────┤
│                                        │
│                                        │
│          ╭────────╮                    │
│          │ [Icon] │                    │
│          ╰────────╯                    │
│       No Connections Yet               │
│   Get started by creating...           │
│      [Add Connection]                  │
│                                        │
│                                        │
├────────────────────────────────────────┤
│ Footer                                 │
└────────────────────────────────────────┘
```

### In a Card/Panel
```
┌────────────────────────────┐
│ Card Header                │
├────────────────────────────┤
│                            │
│    ╭────────╮              │
│    │ [Icon] │              │
│    ╰────────╯              │
│  No History                │
│ Your executed queries...   │
│                            │
└────────────────────────────┘
```

### In a Table View
```
┌────────────────────────────────────────┐
│ Table Header                           │
├────────────────────────────────────────┤
│                                        │
│     ╭────────╮                         │
│     │ [Icon] │                         │
│     ╰────────╯                         │
│    No Results                          │
│  The query returned no results.        │
│                                        │
└────────────────────────────────────────┘
```

### In a Search Context (Smaller)
```
┌──────────────────────┐
│ Search Input         │
├──────────────────────┤
│  ╭────╮              │  ← Smaller icon (48px)
│  │[X] │              │
│  ╰────╯              │
│ No Results Found     │  ← Smaller text
│  Try adjusting...    │
│  [Clear Search]      │
└──────────────────────┘
```

## Design Tokens

### Colors (CSS Variables)
```css
/* Icon backgrounds */
--blue: hsl(221 83% 53%);        /* NoConnectionsEmpty */
--violet: hsl(258 90% 66%);      /* NoHistoryEmpty */
--emerald: hsl(142 76% 36%);     /* NoTablesEmpty */
--amber: hsl(38 92% 50%);        /* NoSearchResultsEmpty */
--slate: hsl(215 16% 47%);       /* NoResultsEmpty */
--cyan: hsl(189 94% 43%);        /* NoDataEmpty */

/* Text colors */
--foreground: hsl(222 47% 11%);
--muted-foreground: hsl(215 16% 47%);
```

### Spacing Scale
```
gap-3:  0.75rem  (12px)   ← Small size
gap-4:  1rem     (16px)   ← Medium size (default)
gap-6:  1.5rem   (24px)   ← Large size

py-8:   2rem     (32px)   ← Small size
py-12:  3rem     (48px)   ← Medium size (default)
py-16:  4rem     (64px)   ← Large size

px-6:   1.5rem   (24px)   ← Horizontal padding
```

### Font Sizes
```
text-sm:   0.875rem  (14px)  ← Small messages
text-base: 1rem      (16px)  ← Default messages
text-lg:   1.125rem  (18px)  ← Medium titles
text-xl:   1.25rem   (20px)  ← Large titles
```

## Print Styles (Future)

```css
@media print {
  .empty-state {
    /* Simplify for print */
    animation: none;
    color-adjust: exact;
  }

  .empty-state button {
    /* Hide interactive elements */
    display: none;
  }
}
```

---

This visual guide should help designers and developers understand the exact layout, spacing, colors, and behavior of the empty state components.
