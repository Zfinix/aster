import { describe, expect, it } from "vitest";
import type { Finding } from "../../src/types";
import {
  applyFixResult,
  findingKey,
  fixAllRunning,
  setFix,
  startFixAll,
  takeNextFix,
  tally,
} from "./review";
import { emptyReview, type ReviewData } from "./thread";

const finding = (title: string): Finding => ({
  file_path: "src/lib.rs",
  line: 10,
  severity: "high",
  category: "correctness",
  title,
  description: "",
  suggestion: "",
});

const review = (...titles: string[]): ReviewData => ({
  ...emptyReview(),
  status: "done",
  findings: titles.map(finding),
});

describe("applyFixResult", () => {
  it("settles only an issue that is being fixed", () => {
    const a = finding("a");
    const idle = review("a");
    expect(applyFixResult(idle, a, "applied")).toBe(idle);

    const fixing = setFix(idle, findingKey(a), { status: "fixing" });
    expect(applyFixResult(fixing, a, "applied", undefined, "- x\n+ y\n").fixes).toEqual({
      [findingKey(a)]: { status: "fixed", patch: "- x\n+ y\n" },
    });
    expect(applyFixResult(fixing, a, "cannot_fix", "no safe edit").fixes).toEqual({
      [findingKey(a)]: { status: "failed", reason: "no safe edit" },
    });
    expect(applyFixResult(fixing, a, "blocked", "edits need approval").fixes).toEqual({
      [findingKey(a)]: { status: "blocked", reason: "edits need approval", patch: undefined },
    });
  });
});

describe("fix all", () => {
  it("sends one issue at a time, skips dismissed ones, and retries failures", () => {
    const [a, b, c] = ["a", "b", "c"].map(finding);
    let data = setFix(review("a", "b", "c"), findingKey(c), { status: "failed", reason: "x" });
    data = startFixAll(data);
    expect(data.fixAll).toEqual({
      keys: [findingKey(a), findingKey(b), findingKey(c)],
      queue: [findingKey(a), findingKey(b), findingKey(c)],
    });

    const first = takeNextFix(data);
    expect(first?.finding).toEqual(a);
    data = first!.data;
    expect(takeNextFix(data)).toBeNull();
    expect(fixAllRunning(data)).toBe(true);

    data = applyFixResult(data, a, "applied");
    data = setFix(data, findingKey(b), { status: "dismissed" });
    const second = takeNextFix(data);
    expect(second?.finding).toEqual(c);
    data = applyFixResult(second!.data, c, "error", "timed out");

    expect(takeNextFix(data)).toBeNull();
    expect(fixAllRunning(data)).toBe(false);
    expect(tally(data, data.fixAll!.keys)).toEqual({ open: 0, fixed: 1, dismissed: 1, failed: 1 });
  });
});
