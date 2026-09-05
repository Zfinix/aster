export function filesFromTransfer(data: DataTransfer): File[] {
  const files = Array.from(data.items)
    .filter((item) => item.kind === "file")
    .map((item) => item.getAsFile())
    .filter((file): file is File => file !== null);
  return files.length ? files : Array.from(data.files);
}

export function fileUrisFromTransfer(data: DataTransfer): string[] {
  const list = data.getData("text/uri-list") || data.getData("text/plain");
  return list
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.startsWith("file://"));
}
