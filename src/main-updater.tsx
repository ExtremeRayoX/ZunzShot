import React from "react";
import ReactDOM from "react-dom/client";
import { UpdaterWindowApp } from "./components/UpdaterWindowApp";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <UpdaterWindowApp />
  </React.StrictMode>
);
