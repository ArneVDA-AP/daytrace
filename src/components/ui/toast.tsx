import * as React from "react"
import { Toast as ToastPrimitive } from "radix-ui"
import { CircleCheck, Info, OctagonAlert, X } from "lucide-react"
import { cn } from "@/lib/utils"

/*
 * Toasts on the Radix Toast primitive instead of shadcn's default (sonner).
 * Reason: sonner inserts a <style> element at runtime. Tauri adds a nonce to style-src as soon as
 * the built index.html contains an inline <style> (tauri manager/mod.rs, replace_csp_nonce); with
 * a nonce present, browsers ignore 'unsafe-inline' and runtime-injected styles stop applying.
 * Radix Toast ships no CSS, so it cannot break that way, and it announces politely by default.
 */

type Tone = "default" | "success" | "error"
interface ToastItem {
  id: number
  title: string
  description?: string
  tone: Tone
  action?: { label: string; onClick: () => void }
}

const listeners = new Set<(t: ToastItem[]) => void>()
let items: ToastItem[] = []
let seq = 0

export function toast(title: string, opts: { description?: string; tone?: Tone; action?: ToastItem["action"] } = {}) {
  items = [...items, { id: ++seq, title, description: opts.description, tone: opts.tone ?? "default", action: opts.action }]
  listeners.forEach((l) => l(items))
}

function dismiss(id: number) {
  items = items.filter((t) => t.id !== id)
  listeners.forEach((l) => l(items))
}

const ICON: Record<Tone, React.ReactNode> = {
  default: <Info className="size-4 text-muted-foreground" aria-hidden />,
  success: <CircleCheck className="size-4 text-live-foreground" aria-hidden />,
  error: <OctagonAlert className="size-4 text-destructive" aria-hidden />,
}

export function Toaster() {
  const [list, setList] = React.useState<ToastItem[]>(items)
  React.useEffect(() => {
    listeners.add(setList)
    return () => {
      listeners.delete(setList)
    }
  }, [])
  return (
    <ToastPrimitive.Provider swipeDirection="right" duration={5000}>
      {list.map((t) => (
        <ToastPrimitive.Root
          key={t.id}
          type={t.tone === "error" ? "foreground" : "background"}
          onOpenChange={(open) => !open && dismiss(t.id)}
          className={cn(
            "grid w-[22rem] grid-cols-[auto_1fr_auto] items-start gap-x-3 gap-y-1 rounded-lg border bg-popover p-3 text-popover-foreground shadow-lg",
            "data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:slide-in-from-bottom-2",
            "data-[state=closed]:animate-out data-[state=closed]:fade-out-0",
          )}
        >
          <span className="mt-0.5">{ICON[t.tone]}</span>
          <div className="min-w-0">
            <ToastPrimitive.Title className="text-body font-medium">{t.title}</ToastPrimitive.Title>
            {t.description && (
              <ToastPrimitive.Description className="mt-0.5 text-meta break-words text-muted-foreground">
                {t.description}
              </ToastPrimitive.Description>
            )}
            {t.action && (
              <ToastPrimitive.Action altText={t.action.label} asChild>
                <button
                  className="mt-2 rounded-sm text-meta font-medium underline underline-offset-2 hover:text-foreground"
                  onClick={t.action.onClick}
                >
                  {t.action.label}
                </button>
              </ToastPrimitive.Action>
            )}
          </div>
          <ToastPrimitive.Close
            aria-label="Dismiss"
            className="-m-1 rounded-sm p-1 text-muted-foreground hover:bg-surface-2 hover:text-foreground"
          >
            <X className="size-4" aria-hidden />
          </ToastPrimitive.Close>
        </ToastPrimitive.Root>
      ))}
      <ToastPrimitive.Viewport className="fixed right-4 bottom-4 z-[100] flex max-h-screen flex-col gap-2 outline-none" />
    </ToastPrimitive.Provider>
  )
}
