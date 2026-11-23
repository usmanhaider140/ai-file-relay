import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ApprovalPopup } from "./Approval";
import "./styles.css";

// The same page serves both windows: "#approval" is the popup.
const isPopup = window.location.hash === "#approval";

createRoot(document.getElementById("root")!).render(
  <StrictMode>{isPopup ? <ApprovalPopup /> : <App />}</StrictMode>,
);
