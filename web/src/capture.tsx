// The quick-capture window's entry (capture.html): one box, nothing else.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { applyTheme, savedTheme } from "./api.ts";
import Capture from "./components/Capture.tsx";
import "./tokens.css";
import "./app.css";

applyTheme(savedTheme());

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Capture />
  </StrictMode>,
);
