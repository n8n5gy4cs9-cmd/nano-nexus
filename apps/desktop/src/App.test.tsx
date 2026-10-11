import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { RouterProvider, createMemoryRouter } from "react-router-dom";
import { beforeEach, expect, test } from "vitest";

import { setTransport } from "@/lib/transport";
import { createMockTransport } from "@/lib/transport/mockTransport";
import { routes } from "@/routes";
import { useUi } from "@/stores/ui";

function mount(path = "/") {
  setTransport(createMockTransport({ app_info: () => ({ os: "linux", version: "0.1.0", arch: "x86_64", data_dir: "", log_dir: "" }) }));
  const router = createMemoryRouter(routes, { initialEntries: [path] });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

beforeEach(() => useUi.setState({ sidebarOpen: true, inspectorOpen: false }));

test("sidebar lists all eight sections plus settings", () => {
  mount();
  for (const label of ["Chat", "Assistants", "Models", "Analyzer", "Rename Forge", "Editor", "Changes", "Permissions", "Settings"]) {
    expect(screen.getAllByText(label).length).toBeGreaterThan(0);
  }
});

test("navigating shows the placeholder page", () => {
  mount("/models");
  expect(screen.getByRole("heading", { name: "Models" })).toBeTruthy();
});

test("Ctrl+digit navigates and Ctrl+I toggles the inspector (non-mac)", () => {
  const router = mount();
  fireEvent.keyDown(window, { key: "3", ctrlKey: true });
  expect(router.state.location.pathname).toBe("/models");
  expect(screen.queryByLabelText("Inspector")).toBeNull();
  fireEvent.keyDown(window, { key: "i", ctrlKey: true });
  expect(screen.getByLabelText("Inspector")).toBeTruthy();
});

test("sidebar can be collapsed", () => {
  mount();
  fireEvent.click(screen.getByLabelText("Toggle sidebar"));
  expect(screen.queryByLabelText("Sidebar")).toBeNull();
});
