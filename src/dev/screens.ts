/*
 * DEV ONLY. Drawn stand-ins for screenshots: an SVG "window" per app kind with the window title
 * and the capture time, returned as a data URL like get_screenshot does.
 */

type Kind = "code" | "browser" | "video" | "chat" | "terminal" | "docs" | "idle" | "mail";

function kindOf(app: string | null): Kind {
  const a = (app ?? "").toLowerCase();
  if (!a) return "idle";
  if (a.includes("code") || a.includes("analyzer")) return "code";
  if (a.includes("terminal")) return "terminal";
  if (a.includes("youtube")) return "video";
  if (a.includes("teams")) return "chat";
  if (a.includes("outlook")) return "mail";
  if (a.includes("notion") || a.includes("obsidian") || a.includes("linear") || a.includes("excel")) return "docs";
  return "browser";
}

const esc = (s: string) => s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

function lines(x: number, y: number, n: number, color: string, seed: number, maxW: number, gap = 16): string {
  let out = "";
  for (let i = 0; i < n; i++) {
    const w = 0.35 + (((seed * 9301 + i * 49297) % 233280) / 233280) * 0.6;
    out += `<rect x="${x + ((i * 7 + seed) % 4) * 14}" y="${y + i * gap}" width="${Math.round(maxW * w)}" height="7" rx="3" fill="${color}"/>`;
  }
  return out;
}

export function screenSvg(app: string | null, title: string, time: string, seed: number): string {
  const k = kindOf(app);
  const W = 1280;
  const H = 720;
  let body = "";
  switch (k) {
    case "code":
      body = `<rect y="40" width="${W}" height="${H - 40}" fill="#1f2430"/><rect y="40" width="220" height="${H - 40}" fill="#191d26"/>${lines(24, 72, 18, "#3b4254", seed, 150, 22)}${lines(260, 72, 26, "#6f86c9", seed + 3, 700, 22)}${lines(300, 84, 12, "#c3a46b", seed + 7, 420, 44)}`;
      break;
    case "terminal":
      body = `<rect y="40" width="${W}" height="${H - 40}" fill="#0c0e12"/>${lines(24, 70, 26, "#3fae6b", seed, 900, 22)}`;
      break;
    case "video":
      body = `<rect y="40" width="${W}" height="${H - 40}" fill="#f3f3f3"/><rect x="40" y="80" width="820" height="460" rx="10" fill="#202124"/><polygon points="430,270 430,350 500,310" fill="#ffffff" opacity="0.85"/>${lines(40, 570, 3, "#c9c9c9", seed, 700, 22)}${lines(900, 80, 14, "#d6d6d6", seed + 5, 320, 34)}`;
      break;
    case "chat":
      body = `<rect y="40" width="${W}" height="${H - 40}" fill="#f5f5fb"/><rect y="40" width="260" height="${H - 40}" fill="#e9e9f4"/>${lines(24, 80, 10, "#c5c5dc", seed, 200, 40)}<rect x="300" y="80" width="940" height="520" rx="10" fill="#2b2d3a"/>${lines(330, 110, 10, "#5a5d75", seed + 2, 600, 44)}`;
      break;
    case "mail":
      body = `<rect y="40" width="${W}" height="${H - 40}" fill="#ffffff"/><rect y="40" width="380" height="${H - 40}" fill="#f2f6fb"/>${lines(24, 80, 12, "#c6d4e6", seed, 320, 46)}${lines(420, 90, 16, "#cfd6de", seed + 4, 760, 26)}`;
      break;
    case "docs":
      body = `<rect y="40" width="${W}" height="${H - 40}" fill="#fbfaf8"/><rect x="300" y="70" width="680" height="${H - 100}" fill="#ffffff" stroke="#e6e3de"/>${lines(340, 110, 3, "#3d3d3d", seed, 420, 30)}${lines(340, 220, 14, "#cfcac2", seed + 1, 580, 26)}`;
      break;
    case "browser":
      body = `<rect y="40" width="${W}" height="${H - 40}" fill="#ffffff"/><rect y="40" width="${W}" height="44" fill="#eceff3"/><rect x="160" y="50" width="620" height="24" rx="12" fill="#ffffff"/>${lines(80, 120, 2, "#2f3640", seed, 520, 34)}${lines(80, 200, 14, "#d5dae1", seed + 6, 760, 28)}<rect x="900" y="120" width="300" height="220" rx="8" fill="#e8edf3"/>`;
      break;
    case "idle":
      body = `<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#28324a"/><stop offset="1" stop-color="#0f1320"/></linearGradient></defs><rect y="0" width="${W}" height="${H}" fill="url(#g)"/><text x="640" y="330" font-family="Segoe UI, sans-serif" font-size="88" fill="#ffffff" text-anchor="middle" font-weight="300">${esc(time)}</text>`;
      break;
  }
  const bar =
    k === "idle"
      ? ""
      : `<rect width="${W}" height="40" fill="#e3e6ea"/><text x="16" y="26" font-family="Segoe UI, sans-serif" font-size="15" fill="#333">${esc(title)}</text><g fill="#666"><rect x="${W - 120}" y="19" width="12" height="2"/><rect x="${W - 76}" y="13" width="12" height="12" fill="none" stroke="#666" stroke-width="1.5"/></g>`;
  const stamp = k === "idle" ? "" : `<rect x="${W - 150}" y="${H - 44}" width="134" height="30" rx="6" fill="#000" opacity="0.55"/><text x="${W - 83}" y="${H - 23}" font-family="Segoe UI, sans-serif" font-size="15" fill="#fff" text-anchor="middle">${esc(time)}</text>`;
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}">${body}${bar}${stamp}</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}
