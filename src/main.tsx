import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/globals.css";
import "./lib/i18n";

// 生产模式去掉 StrictMode（避免双重渲染拖慢首屏），开发模式保留
const root = document.getElementById("root")!;
ReactDOM.createRoot(root).render(
  import.meta.env.DEV ? (
    <React.StrictMode>
      <App />
    </React.StrictMode>
  ) : (
    <App />
  )
);
