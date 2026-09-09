import { describe, expect, test } from "bun:test";
import { money, reviewMessage, topSeverity } from "../src/lib/review-format";
import type { Finding, Severity } from "../src/lib/types";

const finding = (severity: Severity, title: string): Finding =>
  ({ severity, title, file_path: "x.ts", line: 1 }) as Finding;

describe("money", () => {
  test("nothing to show for a missing cost", () => {
    expect(money(null)).toBe("");
    expect(money(undefined)).toBe("");
  });

  test("three decimals for ordinary amounts", () => {
    expect(money(1.2345)).toBe("$1.234");
  });

  test("four decimals below a cent, so a small run is not shown as $0.000", () => {
    expect(money(0.0009)).toBe("$0.0009");
  });

  test("zero is a cost, not an absence", () => {
    expect(money(0)).toBe("$0.0000");
  });
});

describe("topSeverity", () => {
  test("returns the worst of the set regardless of order", () => {
    expect(topSeverity([finding("low", "a"), finding("critical", "b"), finding("medium", "c")])).toBe("critical");
  });

  test("an unrecognised severity is treated as info", () => {
    expect(topSeverity([finding("bogus" as Severity, "a")])).toBe("info");
  });
});

describe("reviewMessage", () => {
  test("a clean diff with nothing chased says so plainly", () => {
    expect(reviewMessage([], 0)).toBe("This diff looks good. I didn't find anything worth flagging.");
  });

  test("a clean diff after one refutation stays singular", () => {
    const message = reviewMessage([], 1);
    expect(message).toContain("one possible issue");
    expect(message).toContain("it held up");
  });

  test("a clean diff after several refutations goes plural", () => {
    const message = reviewMessage([], 3);
    expect(message).toContain("three possible issues");
    expect(message).toContain("none of them held up");
  });

  test("a lone serious finding urges a fix before merge", () => {
    expect(reviewMessage([finding("high", "Unbounded cache")], 0)).toContain("One real problem here");
  });

  test("a lone minor finding is framed as small", () => {
    expect(reviewMessage([finding("low", "Stray import")], 0)).toContain("Just one small thing");
  });

  test("any critical finding advises holding the merge and names it", () => {
    const message = reviewMessage([finding("critical", "Token logged"), finding("low", "Typo")], 0);
    expect(message).toContain("hold off merging");
    expect(message).toContain("The serious one is “Token logged”");
    expect(message).toContain("The rest are smaller");
  });

  test("more than two criticals are counted, named, and marked as partial", () => {
    const message = reviewMessage(
      ["a", "b", "c"].map((t) => finding("critical", t)),
      0,
    );
    expect(message).toContain("Three are outright critical");
    expect(message).toContain("among others");
  });

  test("with no criticals the worst high one leads", () => {
    const message = reviewMessage([finding("high", "Race on save"), finding("low", "Typo")], 0);
    expect(message).toContain("Nothing critical");
    expect(message).toContain("“Race on save” stands out");
    expect(message).toContain("The other one is routine");
  });

  test("only minor findings read as cleanups", () => {
    const message = reviewMessage([finding("low", "a"), finding("info", "b")], 0);
    expect(message).toContain("two smaller issues");
    expect(message).toContain("Nothing urgent");
  });

  test("refutations are appended to a non-empty review", () => {
    expect(reviewMessage([finding("low", "a")], 2)).toContain("ruled out two false alarms");
  });

  test("counts of ten and above are written as digits", () => {
    const message = reviewMessage(
      Array.from({ length: 12 }, (_, i) => finding("low", `f${i}`)),
      0,
    );
    expect(message).toContain("12 smaller issues");
  });
});
