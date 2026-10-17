import * as React from "react";

/*
 * Focus return for dialogs (WCAG 2.4.3). Radix gives focus back only to a dialog's own Trigger;
 * most Daytrace dialogs open from state (a menu item, a URL param, a shortcut), and closing them
 * left focus on <body>, so the next Tab started over at "Skip to content". Dialog and AlertDialog
 * remember what had focus when they opened and give it back when they close:
 *   - opened from a DropdownMenu item: the menu's trigger (the item is gone by then);
 *   - that element is gone, hidden or disabled: the dialog's `returnFocus` (for example the
 *     Export… button when the export opened from a link), else Radix's default.
 */

export type ReturnFocus = () => HTMLElement | null | undefined;

const Ctx = React.createContext<React.RefObject<HTMLElement | null> | null>(null);
export const FocusReturnProvider = Ctx.Provider;

/** What should get focus back: the focused element, or the trigger of the menu it sits in. */
export function captureOpener(): HTMLElement | null {
  const a = document.activeElement as HTMLElement | null;
  if (!a || a === document.body || a === document.documentElement) return null;
  const menu = a.closest<HTMLElement>('[role="menu"], [role="menubar"]');
  if (menu) {
    const id = menu.getAttribute("aria-labelledby");
    const trigger = id ? document.getElementById(id) : null;
    if (trigger) return trigger;
  }
  return a;
}

export function canFocus(el: HTMLElement | null | undefined): el is HTMLElement {
  if (!el || !el.isConnected) return false;
  if ((el as HTMLButtonElement).disabled) return false;
  if (el.closest("[inert]")) return false;
  return el.getClientRects().length > 0;
}

/**
 * Remembers the opener of a dialog. `open` is the controlled prop (undefined when uncontrolled);
 * the returned onOpenChange catches uncontrolled opens (through a Trigger).
 */
export function useOpener(open: boolean | undefined, onOpenChange?: (open: boolean) => void) {
  const ref = React.useRef<HTMLElement | null>(null);
  const was = React.useRef(false);
  // A layout effect runs before Radix's FocusScope moves focus into the new dialog.
  React.useLayoutEffect(() => {
    if (open === undefined) return;
    if (open && !was.current) ref.current = captureOpener();
    was.current = open;
  }, [open]);
  const change = React.useCallback(
    (o: boolean) => {
      if (o && open === undefined) ref.current = captureOpener();
      onOpenChange?.(o);
    },
    [open, onOpenChange],
  );
  return { ref, onOpenChange: change };
}

/** onCloseAutoFocus for the dialog content: focus the opener, else `returnFocus`. */
export function useCloseAutoFocus(returnFocus?: ReturnFocus, own?: (e: Event) => void) {
  const opener = React.useContext(Ctx);
  return React.useCallback(
    (e: Event) => {
      own?.(e);
      if (e.defaultPrevented) return;
      const first = opener?.current;
      const target = canFocus(first) ? first : returnFocus?.();
      if (canFocus(target)) {
        e.preventDefault();
        target.focus();
      }
    },
    [opener, returnFocus, own],
  );
}
