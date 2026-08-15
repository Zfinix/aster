import { describe, expect, it } from "vitest";
import { nearBottom } from "./useStickToBottom";

describe("nearBottom", () => {
  it("counts the exact bottom as pinned", () => {
    expect(nearBottom({ scrollHeight: 1000, scrollTop: 800, clientHeight: 200 })).toBe(true);
  });

  it("tolerates a few px of slack", () => {
    expect(nearBottom({ scrollHeight: 1000, scrollTop: 760, clientHeight: 200 })).toBe(true);
  });

  it("counts a reader who scrolled up as away", () => {
    expect(nearBottom({ scrollHeight: 1000, scrollTop: 200, clientHeight: 200 })).toBe(false);
  });

  /**
   * A hidden webview keeps rendering but measures zero. Reading that as
   * "scrolled away" un-pinned the thread while the panel was in the background,
   * and nothing re-pinned it on the way back, so following stayed off until the
   * reader scrolled to the bottom by hand.
   */
  it("refuses to judge a hidden panel", () => {
    expect(nearBottom({ scrollHeight: 1000, scrollTop: 800, clientHeight: 0 })).toBeNull();
    expect(nearBottom({ scrollHeight: 0, scrollTop: 0, clientHeight: 0 })).toBeNull();
  });

  it("counts a thread shorter than its viewport as pinned", () => {
    expect(nearBottom({ scrollHeight: 120, scrollTop: 0, clientHeight: 400 })).toBe(true);
  });
});
