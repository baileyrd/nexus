import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { registerBuiltins, registerPluginContributions } from "./contributions";
import { hydrateOverrides } from "./keybindings/overrides";
import { startPluginEventLogger } from "./plugins/events";
import "./styles.css";

registerBuiltins();
void registerPluginContributions();
void hydrateOverrides();
void startPluginEventLogger();

const root = document.getElementById("root");
if (!root) throw new Error("#root element missing from index.html");

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
