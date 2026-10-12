import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import { setTransport } from "@/lib/transport";
import { createMockTransport } from "@/lib/transport/mockTransport";
import { tauriTransport } from "@/lib/transport/tauriTransport";

import { SharingSettings, shortAgent } from "./SharingSettings";

afterEach(() => setTransport(tauriTransport));

const card = (id: string, name: string) => ({ id, name, icon: "hexagon", color: "violet", category: "General", tagline: "", description: "", version: null, source: "builtin", enabled: true, available: true });

const settings = (allowed: string[]) => ({
  "share.enabled": true,
  "share.autostart": false,
  "share.port": 9889,
  "share.bind": "0.0.0.0",
  "share.mdns": true,
  "share.https": false,
  "share.default_policy": "chat_read",
  "share.allowed_assistants": allowed,
  "share.session_days": 30,
  "share.max_devices": 5,
  "share.notify_pairing": true,
});

function mount(allowed: string[]) {
  const t = createMockTransport({
    share_status: () => ({ running: true, port: 9889, urls: ["http://192.168.1.20:9889"], devices: 1, https_fingerprint: null, error: null, suggested_port: null }),
    settings_get: () => settings(allowed),
    assistants_list: () => [card("nexus", "Nexus"), card("duo", "Duo")],
    share_devices_list: () => [{ id: "d1", name: "Kim's iPhone", policy: "chat_read", user_agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit Safari/604.1", last_ip: "192.168.1.5", created_at: 1, last_seen_at: null, expires_at: 2 }],
    share_settings_set: () => ({ running: true, port: 9889, urls: [], devices: 1, https_fingerprint: null, error: null, suggested_port: null }),
  });
  setTransport(t);
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <SharingSettings />
    </QueryClientProvider>,
  );
  return t;
}

test("the device list shows a short browser/device description", async () => {
  mount(["*"]);
  expect(await screen.findByText(/192\.168\.1\.5/)).toBeTruthy();
  expect(screen.getByText(/iPhone · Safari/)).toBeTruthy();
  expect(shortAgent("Mozilla/5.0 (Windows NT 10.0) Chrome/120 Edg/120")).toBe("Windows · Chrome");
  expect(shortAgent("weird")).toBe("weird");
});

test("turning off 'all assistants' lists them and unticking one saves the list", async () => {
  const t = mount(["*"]);
  fireEvent.click(await screen.findByLabelText("All assistants are available remotely"));
  await waitFor(() => expect(t.calls.some((c) => c.command === "share_settings_set")).toBe(true));
  expect(t.calls.find((c) => c.command === "share_settings_set")?.args).toMatchObject({ key: "share.allowed_assistants", value: ["nexus", "duo"] });
});

test("with a chosen list, ticking an assistant adds it", async () => {
  const t = mount(["nexus"]);
  const box = await screen.findByLabelText("Duo");
  fireEvent.click(box);
  await waitFor(() => expect(t.calls.filter((c) => c.command === "share_settings_set").length).toBeGreaterThan(0));
  expect(t.calls.filter((c) => c.command === "share_settings_set").at(-1)?.args).toMatchObject({ value: ["nexus", "duo"] });
});
