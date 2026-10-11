/** `true` in the LAN Share web build (`pnpm build:web`), `false` in the desktop app. */
export const IS_WEB = import.meta.env.VITE_TARGET === "web";
