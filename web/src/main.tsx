import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App.tsx";
import { applyTheme, savedTheme } from "./api.ts";
import "./tokens.css";
import "./app.css";

applyTheme(savedTheme()); // before the first paint, so a light choice never flashes dark

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
