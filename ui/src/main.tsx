import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Root } from "./Pair";
import "./styles.css";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <Root />
    </StrictMode>,
  );
}

// Phone smoke builds drive their own UI; other builds leave this out.
if (import.meta.env.VITE_PHONE_SMOKE === "1") {
  void import("./phoneSmoke").then((smoke) => smoke.runPhoneSmoke());
}
