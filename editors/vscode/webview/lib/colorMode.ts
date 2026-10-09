import { useEffect, useState } from "react";

function currentMode(): "light" | "dark" {
  const body = document.body.classList;
  if (body.contains("vscode-light") || body.contains("vscode-high-contrast-light")) return "light";
  if (body.contains("vscode-dark") || body.contains("vscode-high-contrast")) return "dark";
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

/** Light or dark, tracking VS Code's body class or, in a browser, the OS. */
export function useColorMode(): "light" | "dark" {
  const [mode, setMode] = useState(currentMode);
  useEffect(() => {
    const update = () => setMode(currentMode());
    const observer = new MutationObserver(update);
    observer.observe(document.body, { attributes: true, attributeFilter: ["class"] });
    const media = window.matchMedia("(prefers-color-scheme: light)");
    media.addEventListener("change", update);
    return () => {
      observer.disconnect();
      media.removeEventListener("change", update);
    };
  }, []);
  return mode;
}
