// Stylesheet first, so its rules load before any component module.
import "./index.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "@/app/App";

async function boot() {
  // Browser preview (npm run dev outside Tauri): answer every IPC call with synthetic data.
  // import.meta.env.DEV is false in production builds, so this branch and the whole src/dev
  // folder are removed from dist (scripts/check-dist.mjs verifies it).
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
    const { installMockIpc } = await import("./dev/mock-ipc");
    installMockIpc();
  }
  createRoot(document.getElementById("root") as HTMLElement).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

void boot();
