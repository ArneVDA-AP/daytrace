import { useEffect, useRef, useState } from "react";

/**
 * Loading indicator timing: stays false for `delay` ms (fast loads never flash a skeleton), and
 * once shown stays true for at least `min` ms (no flicker). `const show = useDelayedLoading(!data)`.
 */
export function useDelayedLoading(loading: boolean, delay = 150, min = 300): boolean {
  const [show, setShow] = useState(false);
  const shownAt = useRef(0);
  useEffect(() => {
    if (loading) {
      const t = setTimeout(() => {
        shownAt.current = Date.now();
        setShow(true);
      }, delay);
      return () => clearTimeout(t);
    }
    if (!show) return;
    const left = Math.max(0, min - (Date.now() - shownAt.current));
    const t = setTimeout(() => setShow(false), left);
    return () => clearTimeout(t);
  }, [loading, delay, min, show]);
  return show;
}
