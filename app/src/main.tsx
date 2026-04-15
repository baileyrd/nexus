import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { registerBuiltins } from "./contributions";
import "./styles.css";

registerBuiltins();

const root = document.getElementById("root");
if (!root) throw new Error("#root element missing from index.html");

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
