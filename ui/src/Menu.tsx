/** Shared menu / nested-submenu primitive with a safe-triangle hover corridor.
 *
 * Flat menus use `Menu` + `MenuItem`. Nested hover submenus use `SubMenu`; while
 * a submenu is open, diagonal pointer motion inside the corridor toward its
 * nearest edge keeps it open, motion outside switches immediately, and a short
 * idle pause on a sibling switches cleanly. */
import {
  createContext,
  forwardRef,
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type FocusEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent,
  type ReactNode,
  type RefObject,
} from "react";
import {
  isInsideSafeTriangle,
  type Point,
  type SubmenuSide,
} from "./safeTriangle";

/** Idle pause on a sibling inside the corridor before switching (ms). */
export const SAFE_TRIANGLE_IDLE_MS = 200;

/** Default edge padding so grazing the submenu border never flickers. */
export const SAFE_TRIANGLE_PADDING = 4;

type MenuCtx = {
  openId: string | null;
  requestOpen: (id: string, anchor: Point) => void;
  requestClose: (id?: string) => void;
  onTriggerPointerMove: (id: string, point: Point) => void;
  onTriggerPointerLeave: (id: string) => void;
  registerPanel: (id: string, el: HTMLElement | null) => void;
  registerTrigger: (id: string, el: HTMLElement | null) => void;
  registerSide: (id: string, side: SubmenuSide) => void;
  sideFor: (id: string) => SubmenuSide;
  idleMs: number;
  padding: number;
};

const MenuContext = createContext<MenuCtx | null>(null);

function useMenuCtx(): MenuCtx {
  const ctx = useContext(MenuContext);
  if (!ctx) throw new Error("MenuItem/SubMenu must be used inside Menu");
  return ctx;
}

export type MenuProps = {
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
  "aria-label"?: string;
  /** Close the whole menu (Escape from the root). Optional. */
  onClose?: () => void;
  idleMs?: number;
  padding?: number;
  onKeyDown?: (event: ReactKeyboardEvent<HTMLDivElement>) => void;
  onClick?: (event: ReactMouseEvent<HTMLDivElement>) => void;
};

/** Root `role="menu"` surface. Owns open-submenu state and the safe triangle. */
export const Menu = forwardRef<HTMLDivElement, MenuProps>(function Menu(
  {
    children,
    className,
    style,
    "aria-label": ariaLabel,
    onClose,
    idleMs = SAFE_TRIANGLE_IDLE_MS,
    padding = SAFE_TRIANGLE_PADDING,
    onKeyDown,
    onClick,
  },
  ref,
) {
  const [openId, setOpenId] = useState<string | null>(null);
  const anchorRef = useRef<Point | null>(null);
  const panels = useRef(new Map<string, HTMLElement>());
  const triggers = useRef(new Map<string, HTMLElement>());
  const sides = useRef(new Map<string, SubmenuSide>());
  const idleTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingId = useRef<string | null>(null);

  const clearIdle = useCallback(() => {
    if (idleTimer.current != null) {
      clearTimeout(idleTimer.current);
      idleTimer.current = null;
    }
    pendingId.current = null;
  }, []);

  useEffect(() => () => clearIdle(), [clearIdle]);

  const openNow = useCallback(
    (id: string, anchor: Point) => {
      clearIdle();
      anchorRef.current = anchor;
      setOpenId(id);
    },
    [clearIdle],
  );

  const requestOpen = useCallback(
    (id: string, anchor: Point) => {
      // Keyboard / explicit open bypasses the corridor and switches immediately.
      openNow(id, anchor);
    },
    [openNow],
  );

  const requestClose = useCallback(
    (id?: string) => {
      if (id != null && openId !== id) return;
      clearIdle();
      anchorRef.current = null;
      setOpenId(null);
    },
    [clearIdle, openId],
  );

  const onTriggerPointerMove = useCallback(
    (id: string, point: Point) => {
      if (openId == null || openId === id) {
        openNow(id, point);
        return;
      }
      const panel = panels.current.get(openId);
      const side = sides.current.get(openId) ?? "right";
      const anchor = anchorRef.current;
      if (!panel || !anchor) {
        openNow(id, point);
        return;
      }
      const rect = panel.getBoundingClientRect();
      if (isInsideSafeTriangle(point, anchor, rect, side, padding)) {
        // Inside corridor: keep current submenu; schedule idle switch to sibling.
        if (pendingId.current !== id) {
          clearIdle();
          pendingId.current = id;
          idleTimer.current = setTimeout(() => {
            idleTimer.current = null;
            pendingId.current = null;
            openNow(id, point);
          }, idleMs);
        }
        return;
      }
      // Outside corridor: switch immediately.
      openNow(id, point);
    },
    [clearIdle, idleMs, openId, openNow, padding],
  );

  const onTriggerPointerLeave = useCallback(
    (id: string) => {
      if (pendingId.current === id) clearIdle();
    },
    [clearIdle],
  );

  const registerPanel = useCallback((id: string, el: HTMLElement | null) => {
    if (el) panels.current.set(id, el);
    else panels.current.delete(id);
  }, []);

  const registerTrigger = useCallback((id: string, el: HTMLElement | null) => {
    if (el) triggers.current.set(id, el);
    else triggers.current.delete(id);
  }, []);

  const sideFor = useCallback((id: string): SubmenuSide => sides.current.get(id) ?? "right", []);

  const registerSide = useCallback((id: string, side: SubmenuSide) => {
    sides.current.set(id, side);
  }, []);

  const ctx = useMemo<MenuCtx>(
    () => ({
      openId,
      requestOpen,
      requestClose,
      onTriggerPointerMove,
      onTriggerPointerLeave,
      registerPanel,
      registerTrigger,
      registerSide,
      sideFor,
      idleMs,
      padding,
    }),
    [
      openId,
      requestOpen,
      requestClose,
      onTriggerPointerMove,
      onTriggerPointerLeave,
      registerPanel,
      registerTrigger,
      registerSide,
      sideFor,
      idleMs,
      padding,
    ],
  );

  function handleKey(event: ReactKeyboardEvent<HTMLDivElement>) {
    onKeyDown?.(event);
    if (event.defaultPrevented) return;
    const root = event.currentTarget;
    const items = menuItems(root);
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (event.key === "Escape") {
      event.preventDefault();
      if (openId != null) {
        requestClose(openId);
        triggers.current.get(openId)?.focus();
      } else {
        onClose?.();
      }
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (items.length === 0) return;
      const step = event.key === "ArrowDown" ? 1 : -1;
      const next = at < 0 ? 0 : (at + step + items.length) % items.length;
      items[next]?.focus();
    }
  }

  return (
    <MenuContext.Provider value={ctx}>
      <div
        ref={ref}
        className={className}
        style={style}
        role="menu"
        aria-label={ariaLabel}
        onKeyDown={handleKey}
        onClick={onClick}
      >
        {children}
      </div>
    </MenuContext.Provider>
  );
});

function menuItems(root: HTMLElement): HTMLElement[] {
  const active = document.activeElement;
  const nested =
    active instanceof HTMLElement ? active.closest<HTMLElement>('[role="menu"]') : null;
  const scope = nested && nested !== root && root.contains(nested) ? nested : root;
  return [...scope.querySelectorAll<HTMLElement>('[role="menuitem"]')].filter(
    (el) => el.closest('[role="menu"]') === scope,
  );
}

export type MenuItemProps = {
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
  disabled?: boolean;
  onClick?: () => void;
  onKeyDown?: (event: ReactKeyboardEvent<HTMLButtonElement>) => void;
  "aria-checked"?: boolean | "true" | "false";
};

/** A single `role="menuitem"` action. */
export const MenuItem = forwardRef<HTMLButtonElement, MenuItemProps>(function MenuItem(
  { children, className, style, disabled, onClick, onKeyDown, "aria-checked": ariaChecked },
  ref,
) {
  return (
    <button
      ref={ref}
      type="button"
      role="menuitem"
      className={className}
      style={style}
      disabled={disabled}
      aria-checked={ariaChecked}
      onClick={onClick}
      onKeyDown={onKeyDown}
    >
      {children}
    </button>
  );
});

export type SubMenuProps = {
  /** Stable id for this submenu within its parent Menu. */
  id?: string;
  label: ReactNode;
  children: ReactNode;
  /** Preferred open side; defaults to `"right"`. Flips to `"left"` when needed by the caller. */
  side?: SubmenuSide;
  className?: string;
  triggerClassName?: string;
  panelClassName?: string;
};

/** Nested hover submenu with safe-triangle corridor + keyboard open/close. */
export function SubMenu({
  id: idProp,
  label,
  children,
  side = "right",
  className,
  triggerClassName,
  panelClassName,
}: SubMenuProps) {
  const ctx = useMenuCtx();
  const autoId = useId();
  const id = idProp ?? autoId;
  const open = ctx.openId === id;
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    ctx.registerTrigger(id, triggerRef.current);
    return () => ctx.registerTrigger(id, null);
  }, [ctx, id]);

  useEffect(() => {
    ctx.registerPanel(id, open ? panelRef.current : null);
    return () => ctx.registerPanel(id, null);
  }, [ctx, id, open]);

  useEffect(() => {
    ctx.registerSide(id, side);
  }, [ctx, id, side]);

  function pointFrom(event: PointerEvent | FocusEvent): Point {
    if ("clientX" in event) return { x: event.clientX, y: event.clientY };
    const r = triggerRef.current?.getBoundingClientRect();
    return r ? { x: r.left + r.width / 2, y: r.top + r.height / 2 } : { x: 0, y: 0 };
  }

  function onPointerEnter(event: PointerEvent<HTMLButtonElement>) {
    ctx.onTriggerPointerMove(id, pointFrom(event));
  }

  function onPointerMove(event: PointerEvent<HTMLButtonElement>) {
    ctx.onTriggerPointerMove(id, pointFrom(event));
  }

  function onPointerLeave() {
    ctx.onTriggerPointerLeave(id);
  }

  function onKeyDown(event: ReactKeyboardEvent<HTMLButtonElement>) {
    const openKey = side === "right" ? "ArrowRight" : "ArrowLeft";
    const closeKey = side === "right" ? "ArrowLeft" : "ArrowRight";
    if (event.key === openKey || event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      event.stopPropagation();
      const r = triggerRef.current?.getBoundingClientRect();
      ctx.requestOpen(id, r ? { x: r.right, y: r.top + r.height / 2 } : { x: 0, y: 0 });
      // Focus first item in the panel after open.
      queueMicrotask(() => {
        panelRef.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
      });
    } else if (event.key === closeKey && open) {
      event.preventDefault();
      event.stopPropagation();
      ctx.requestClose(id);
    }
  }

  return (
    <div className={className ? `menu-submenu ${className}` : "menu-submenu"} data-submenu-id={id}>
      <button
        ref={triggerRef}
        type="button"
        role="menuitem"
        className={triggerClassName}
        aria-haspopup="menu"
        aria-expanded={open}
        onPointerEnter={onPointerEnter}
        onPointerMove={onPointerMove}
        onPointerLeave={onPointerLeave}
        onKeyDown={onKeyDown}
      >
        {label}
      </button>
      {open && (
        <div
          ref={panelRef}
          className={panelClassName ? `menu-submenu-panel feed-menu ${panelClassName}` : "menu-submenu-panel feed-menu"}
          role="menu"
          aria-label={typeof label === "string" ? label : undefined}
          data-side={side}
          onKeyDown={(event) => {
            const closeKey = side === "right" ? "ArrowLeft" : "ArrowRight";
            if (event.key === closeKey || event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              ctx.requestClose(id);
              triggerRef.current?.focus();
            }
          }}
        >
          {children}
        </div>
      )}
    </div>
  );
}

/** Hook for callers that need safe-triangle checks without the full Menu tree. */
export function useSafeTriangle(options?: {
  idleMs?: number;
  padding?: number;
  side?: SubmenuSide;
}): {
  openId: string | null;
  anchor: Point | null;
  panelRef: RefObject<HTMLElement | null>;
  onPointerMoveToward: (id: string, point: Point) => void;
  open: (id: string, anchor: Point) => void;
  close: () => void;
  isProtected: (point: Point) => boolean;
} {
  const idleMs = options?.idleMs ?? SAFE_TRIANGLE_IDLE_MS;
  const padding = options?.padding ?? SAFE_TRIANGLE_PADDING;
  const side = options?.side ?? "right";
  const [openId, setOpenId] = useState<string | null>(null);
  const [anchor, setAnchor] = useState<Point | null>(null);
  const panelRef = useRef<HTMLElement | null>(null);
  const idleTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingId = useRef<string | null>(null);

  const clearIdle = useCallback(() => {
    if (idleTimer.current != null) {
      clearTimeout(idleTimer.current);
      idleTimer.current = null;
    }
    pendingId.current = null;
  }, []);

  useEffect(() => () => clearIdle(), [clearIdle]);

  const open = useCallback(
    (id: string, next: Point) => {
      clearIdle();
      setOpenId(id);
      setAnchor(next);
    },
    [clearIdle],
  );

  const close = useCallback(() => {
    clearIdle();
    setOpenId(null);
    setAnchor(null);
  }, [clearIdle]);

  const isProtected = useCallback(
    (point: Point) => {
      if (openId == null || anchor == null || !panelRef.current) return false;
      const rect = panelRef.current.getBoundingClientRect();
      return isInsideSafeTriangle(point, anchor, rect, side, padding);
    },
    [anchor, openId, padding, side],
  );

  const onPointerMoveToward = useCallback(
    (id: string, point: Point) => {
      if (openId == null || openId === id) {
        open(id, point);
        return;
      }
      if (isProtected(point)) {
        if (pendingId.current !== id) {
          clearIdle();
          pendingId.current = id;
          idleTimer.current = setTimeout(() => {
            idleTimer.current = null;
            pendingId.current = null;
            open(id, point);
          }, idleMs);
        }
        return;
      }
      open(id, point);
    },
    [clearIdle, idleMs, isProtected, open, openId],
  );

  return { openId, anchor, panelRef, onPointerMoveToward, open, close, isProtected };
}
