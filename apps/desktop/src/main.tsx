import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/layout.css";
import "./styles/components.css";
import "./styles/screens.css";
import "./styles/editor.css";
import "./styles/database.css";
import { render } from "solid-js/web";
import { Bootstrap } from "./app.tsx";
import { loadCore } from "./api/index.ts";
import { applyStoredTheme } from "./state/settings-store.ts";

applyStoredTheme();
document.documentElement.lang = localStorage.getItem("ne.locale") ?? "en";

const root = document.getElementById("root");
if (!root) throw new Error("Missing #root element");
render(() => <Bootstrap load={loadCore} />, root);
