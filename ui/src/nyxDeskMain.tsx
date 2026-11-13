import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { NyxDesk } from "./NyxDesk";
import "./styles.css";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <NyxDesk />
    </StrictMode>,
  );
}
