import { describe, expect, it } from "vitest";
import { displayName, splitMentions } from "./UserText";

describe("splitMentions", () => {
  it("keeps plain text whole", () => {
    expect(splitMentions("no mentions here")).toEqual([
      { kind: "text", text: "no mentions here" },
    ]);
  });

  it("splits a macOS screenshot path with spaces", () => {
    const text =
      "look at @/Users/me/.local/share/aster/pasted/01M1RA-Screenshot 2026-07-28 at 4.55.05 PM.png thanks";
    expect(splitMentions(text)).toEqual([
      { kind: "text", text: "look at " },
      {
        kind: "image",
        path: "/Users/me/.local/share/aster/pasted/01M1RA-Screenshot 2026-07-28 at 4.55.05 PM.png",
      },
      { kind: "text", text: " thanks" },
    ]);
  });

  it("renders the full staged screenshot path from chat history", () => {
    const text =
      "@/Users/me/.local/share/aster/pasted/01M1REJY5B3RG24R6AN6VCSM2H-Screenshot 2026-08-24 at 5.57.30 PM.png";
    expect(splitMentions(text)).toEqual([
      {
        kind: "image",
        path: "/Users/me/.local/share/aster/pasted/01M1REJY5B3RG24R6AN6VCSM2H-Screenshot 2026-08-24 at 5.57.30 PM.png",
      },
    ]);
  });

  it("treats a document mention as a doc", () => {
    expect(splitMentions("see @docs/report.pdf")).toEqual([
      { kind: "text", text: "see " },
      { kind: "doc", path: "docs/report.pdf" },
    ]);
  });

  it("treats a video mention as a doc", () => {
    expect(splitMentions("watch @clips/Aster-desktop.mp4 now")).toEqual([
      { kind: "text", text: "watch " },
      { kind: "doc", path: "clips/Aster-desktop.mp4" },
      { kind: "text", text: " now" },
    ]);
  });

  it("does not eat a mention that follows another", () => {
    const parts = splitMentions("@a.png and @b.pdf");
    expect(parts).toEqual([
      { kind: "image", path: "a.png" },
      { kind: "text", text: " and " },
      { kind: "doc", path: "b.pdf" },
    ]);
  });

  it("keeps two pasted filenames with spaces out of the remaining text", () => {
    const text =
      "@/tmp/01M1RA-Screenshot 2026-07-28 at 4.55.05 PM.png @/tmp/01M1RB-Timesheet 13.pdf";
    expect(splitMentions(text)).toEqual([
      {
        kind: "image",
        path: "/tmp/01M1RA-Screenshot 2026-07-28 at 4.55.05 PM.png",
      },
      { kind: "text", text: " " },
      { kind: "doc", path: "/tmp/01M1RB-Timesheet 13.pdf" },
    ]);
  });

  it("shows a staged paste under the name it was given", () => {
    expect(displayName("/Users/me/.local/share/aster/pasted/01M1RBCZV654JRHMC8SPZWQ4VX-Invoice 13 - August 2026.pdf")).toBe(
      "Invoice 13 - August 2026.pdf"
    );
  });

  it("leaves an @word that is not a file as text", () => {
    expect(splitMentions("ping @casey about this")).toEqual([
      { kind: "text", text: "ping @casey about this" },
    ]);
  });
});
