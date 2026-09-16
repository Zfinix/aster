import { describe, expect, it } from "vitest";
import { ACCENTS, applyAccent } from "./accent";

function target() {
  const store = new Map<string, string>();
  const el = {
    style: {
      setProperty: (key: string, value: string) => {
        store.set(key, value);
      },
      getPropertyValue: (key: string) => store.get(key) ?? "",
      removeProperty: (key: string) => {
        store.delete(key);
      },
    },
  };
  return el as unknown as HTMLElement;
}

describe("applyAccent", () => {
  it("clears the override on the default so the theme keeps its wash", () => {
    const el = target();
    applyAccent("pink", el);
    applyAccent("steel", el);
    expect(el.style.getPropertyValue("--tool-bg")).toBe("");
    expect(el.style.getPropertyValue("--tool-bg-strong")).toBe("");
  });

  it("keeps the theme's wash for an unknown name", () => {
    const el = target();
    applyAccent("chartreuse", el);
    expect(el.style.getPropertyValue("--tool-bg")).toBe("");
  });

  it("swaps the wash for a named hue", () => {
    const el = target();
    applyAccent("dracula", el);
    expect(el.style.getPropertyValue("--tool-bg")).toContain("255, 121, 198");
    expect(el.style.getPropertyValue("--tool-bg-strong")).toContain("255, 121, 198");
  });

  it("keeps every wash faint enough for text to read over", () => {
    for (const name of ACCENTS) {
      const el = target();
      applyAccent(name, el);
      for (const key of ["--tool-bg", "--tool-bg-strong"]) {
        const value = el.style.getPropertyValue(key);
        if (!value || value === "transparent") continue;
        const alpha = Number(/([\d.]+)\)$/.exec(value)?.[1]);
        expect(alpha).toBeLessThanOrEqual(0.2);
      }
    }
  });

  it("strips the wash for none", () => {
    const el = target();
    applyAccent("none", el);
    expect(el.style.getPropertyValue("--tool-bg")).toBe("transparent");
  });

  it("offers every tone to the settings picker", () => {
    expect(ACCENTS).toEqual(["steel", "midnight", "nord", "forest", "gruvbox", "dracula", "none"]);
  });
});
