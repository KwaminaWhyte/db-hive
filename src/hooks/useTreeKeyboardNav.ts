import { useCallback, useMemo, useRef, useState } from "react";
import type { KeyboardEvent } from "react";

/**
 * A flat descriptor for one VISIBLE node in a tree, listed in document order.
 * Built from the same data that drives the render so keyboard navigation and
 * the visual tree can never disagree.
 */
export interface TreeNavNode {
  /** Stable, unique id for the node (e.g. `schema:public`, `table:public.users`) */
  id: string;
  /** 1-based depth, used for `aria-level` */
  level: number;
  /** `undefined` for leaf nodes; `true`/`false` for expandable nodes */
  expanded?: boolean;
  /** Id of the parent node, or `null` for root-level nodes */
  parentId: string | null;
  /** Expand/collapse the node (expandable nodes only) */
  toggle?: () => void;
  /** Primary action — same as clicking the node (open table, toggle schema, …) */
  activate: () => void;
}

/**
 * Roving-tabindex keyboard navigation for a `role="tree"` widget.
 *
 * Keyboard model (WAI-ARIA tree view pattern):
 * - Down/Up: move focus through visible nodes in document order
 * - Right: expand a collapsed node, or move to first child if already expanded
 * - Left: collapse an expanded node, or move to parent if collapsed/leaf
 * - Enter/Space: activate the node's primary action (same as click)
 * - Home/End: jump to first/last visible node
 *
 * Usage: attach `handleKeyDown` to the `role="tree"` container and spread
 * `getTreeItemProps(id)` onto each `role="treeitem"` element.
 */
export function useTreeKeyboardNav(nodes: TreeNavNode[]) {
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const itemRefs = useRef(new Map<string, HTMLElement>());

  // Exactly one node carries tabIndex={0}; fall back to the first visible
  // node when the previously focused node is no longer visible.
  const effectiveFocusedId = useMemo(() => {
    if (focusedId && nodes.some((n) => n.id === focusedId)) return focusedId;
    return nodes.length > 0 ? nodes[0].id : null;
  }, [focusedId, nodes]);

  const focusNode = useCallback((id: string) => {
    setFocusedId(id);
    itemRefs.current.get(id)?.focus();
  }, []);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent<HTMLElement>) => {
      if (nodes.length === 0 || !effectiveFocusedId) return;
      const index = nodes.findIndex((n) => n.id === effectiveFocusedId);
      if (index < 0) return;
      const node = nodes[index];

      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          if (index < nodes.length - 1) focusNode(nodes[index + 1].id);
          break;
        case "ArrowUp":
          e.preventDefault();
          if (index > 0) focusNode(nodes[index - 1].id);
          break;
        case "ArrowRight": {
          e.preventDefault();
          if (node.expanded === false) {
            node.toggle?.();
          } else if (node.expanded === true) {
            const firstChild = nodes.find((n) => n.parentId === node.id);
            if (firstChild) focusNode(firstChild.id);
          }
          break;
        }
        case "ArrowLeft":
          e.preventDefault();
          if (node.expanded === true) {
            node.toggle?.();
          } else if (node.parentId) {
            focusNode(node.parentId);
          }
          break;
        case "Home":
          e.preventDefault();
          focusNode(nodes[0].id);
          break;
        case "End":
          e.preventDefault();
          focusNode(nodes[nodes.length - 1].id);
          break;
        case "Enter":
        case " ":
          // preventDefault stops the browser's native button activation so
          // the action fires exactly once (via activate, same path as click).
          e.preventDefault();
          node.activate();
          break;
      }
    },
    [nodes, effectiveFocusedId, focusNode],
  );

  /** Props to spread onto each treeitem element (ref, roving tabIndex, focus sync) */
  const getTreeItemProps = useCallback(
    (id: string) => ({
      ref: (el: HTMLElement | null) => {
        if (el) itemRefs.current.set(id, el);
        else itemRefs.current.delete(id);
      },
      tabIndex: id === effectiveFocusedId ? 0 : -1,
      onFocus: () => setFocusedId(id),
    }),
    [effectiveFocusedId],
  );

  return { handleKeyDown, getTreeItemProps, focusedId: effectiveFocusedId };
}
