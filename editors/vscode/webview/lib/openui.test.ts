import { describe, expect, it } from "vitest";
import { withAnswers } from "./openui";

describe("withAnswers", () => {
  it("adds what the user picked to the button's message", () => {
    const state = {
      choose: {
        option: { value: "redis", componentType: "RadioGroup" },
        regions: { value: ["eu", "us"], componentType: "CheckBoxGroup" },
        note: { value: "", componentType: "TextArea" },
      },
      $open: true,
    };
    expect(withAnswers("Set it up", state)).toBe(
      "Set it up\n\n(Form answers: option: redis; regions: eu, us)"
    );
  });

  it("leaves a message with no answers alone", () => {
    expect(withAnswers("Explain more", {})).toBe("Explain more");
  });

  it("clips a long answer", () => {
    const sent = withAnswers("Go", { f: { essay: { value: "x".repeat(5000) } } });
    expect(sent.length).toBeLessThan(1100);
    expect(sent.endsWith("…)")).toBe(true);
  });
});
