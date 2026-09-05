import { describe, expect, it } from "vitest";
import { fileUrisFromTransfer, filesFromTransfer } from "./dataTransfer";

describe("filesFromTransfer", () => {
  it("reads file items with the standard drag API", () => {
    const file = { name: "Screenshot 2026-09-05.png" } as File;
    const data = {
      items: [{ kind: "file", getAsFile: () => file }],
      files: [],
    } as unknown as DataTransfer;

    expect(filesFromTransfer(data)).toEqual([file]);
  });

  it("falls back to the legacy files list", () => {
    const file = { name: "report.pdf" } as File;
    const data = { items: [], files: [file] } as unknown as DataTransfer;

    expect(filesFromTransfer(data)).toEqual([file]);
  });
});

describe("fileUrisFromTransfer", () => {
  it("keeps only file URLs from the standard URI payload", () => {
    const data = {
      getData: (type: string) =>
        type === "text/uri-list" ? "# files\nfile:///tmp/a%20file.txt\nhttps://example.test" : "",
    } as unknown as DataTransfer;

    expect(fileUrisFromTransfer(data)).toEqual(["file:///tmp/a%20file.txt"]);
  });
});
