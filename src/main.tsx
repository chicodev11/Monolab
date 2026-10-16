import React from "react";
import ReactDOM from "react-dom/client";
import { ErrorBoundary } from "./components/ui/ErrorBoundary";
import { applyTheme, readTheme } from "./lib/theme";
import { loadLanguage } from "./lib/i18n";

applyTheme(readTheme());

const sink = import.meta.env.DEV && window.location.hash === "#/dev/kitchen-sink";

void loadLanguage().finally(async () => {
  const [{ default: App }, KitchenSink] = await Promise.all([
    import("./App"),
    sink ? import("./dev/KitchenSink").then((module) => module.KitchenSink) : Promise.resolve(null),
  ]);
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <ErrorBoundary>{sink && KitchenSink ? <KitchenSink /> : <App />}</ErrorBoundary>
    </React.StrictMode>,
  );
});
