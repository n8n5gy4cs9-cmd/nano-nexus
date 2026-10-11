import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import "./styles/globals.css";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { ThemeProvider } from "./components/ThemeProvider";

const root = document.getElementById("root");
if (!root) throw new Error("#root not found");
createRoot(root).render(
  <StrictMode>
    <ThemeProvider>
      <App />
    </ThemeProvider>
  </StrictMode>,
);
