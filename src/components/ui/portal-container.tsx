import * as React from "react"

/*
 * Where menus, popovers and tooltips put their content. Radix portals them to <body> by default,
 * which is outside every landmark (axe "region"). The shell provides its landmarks instead (the
 * rail's <aside> and the section's <main>), so a menu opened from the rail lands in the rail and
 * one opened from a page lands in <main>. The content is positioned `fixed`, so the landmark's
 * overflow does not clip it.
 *
 * Inside a modal dialog the value is reset to <body>: the dialog hides everything else from
 * assistive tech while it is open, so a menu inside <main> would be hidden with it.
 */
const PortalContainerContext = React.createContext<HTMLElement | null>(null)

function PortalContainer({ container, children }: { container: HTMLElement | null; children: React.ReactNode }) {
  return <PortalContainerContext.Provider value={container}>{children}</PortalContainerContext.Provider>
}

/** The container for a Radix Portal (`undefined`: <body>). */
function usePortalContainer(): HTMLElement | undefined {
  return React.useContext(PortalContainerContext) ?? undefined
}

export { PortalContainer, usePortalContainer }
