import { useEffect, useState } from "react";
import { inEditor, onHostMessage, post } from "../lib/host";
import { openFilePreview } from "../lib/filePreview";
import { FileTypeIcon } from "./icons";

const IMAGE_EXT = /\.(png|jpe?g|gif|webp|svg|bmp|ico)$/i;

/** In a browser the server hands the bytes over; in the editor the extension reads. */
export function fileUrl(path: string): string {
  return `/api/file?path=${encodeURIComponent(path)}`;
}
// A mention may contain spaces (macOS screenshots do), so it runs to the last
// segment that ends in a known file extension, and never eats a later @mention.
// The boundary is a lookbehind, so the text around a mention keeps its spaces.
const MENTION =
  /(^|\s)@([^\s@]+(?: [^\s@]+)*?\.(?:png|jpe?g|gif|webp|svg|bmp|ico|pdf|docx?|xlsx?|pptx?|odt|ods|odp|rtf|epub|mp4|mkv|mov|avi|webm|mp3|wav|flac|ogg|m4a))(?=\s|$)/gi;

/** What a mention shows: a staged paste keeps the name it was given, not the
 *  stamped copy it became on disk. */
export function displayName(path: string): string {
  const base = path.split("/").pop() ?? path;
  return base.replace(/^\d[0-9A-HJKMNP-TV-Z]{9,}-/, "");
}

export type PreviewFile = {
  path: string;
  lang?: string;
  content: string;
  truncated: boolean;
  image?: string;
  doc?: string;
  size?: number;
};

export function formatBytes(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}

function useFile(path: string, kind: "image" | "doc"): PreviewFile | null {
  const [file, setFile] = useState<PreviewFile | null>(null);

  useEffect(() => {
    // In a browser an image is one GET away, and a document is only ever its
    // chip, so neither needs the host to read bytes.
    if (!inEditor) {
      if (kind === "image") {
        setFile({ path, content: "", truncated: false, image: fileUrl(path) });
      }
      return;
    }
    const requestId = `mention-${Math.random().toString(36).slice(2)}`;
    let alive = true;
    const off = onHostMessage((message) => {
      if (message.type === "filePreview" && message.requestId === requestId) {
        if (alive && message.file) setFile(message.file);
        off();
      }
    });
    post({ type: "readFile", path, requestId });
    return () => {
      alive = false;
      off();
    };
  }, [path, kind]);

  return file;
}

/** One image mention, shown as the image itself once the host has read it. */
function ImageMention({ path }: { path: string }) {
  const file = useFile(path, "image");
  const src = file?.image ?? null;

  if (!src) return <span className="mention-chip">{displayName(path)}</span>;
  return (
    <img
      className="mention-image"
      src={src}
      alt={displayName(path)}
      title={path}
      onClick={() => openFilePreview(path)}
    />
  );
}

/** Which of our own glyphs fits this file. */
export function fileIconKind(path: string): "file" | "image" | "doc" | "zip" {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  if (["zip", "gz", "tar", "rar", "7z"].includes(ext)) return "zip";
  if (
    [
      "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico",
      "mp4", "mkv", "mov", "avi", "webm", "mp3", "wav", "flac", "ogg", "m4a",
    ].includes(ext)
  ) {
    return "image";
  }
  return "doc";
}

/** One document mention: a card with our file glyph, the name, and the size. */
export function DocCard({ file, path }: { file?: PreviewFile | null; path: string }) {
  const name = displayName(path);
  return (
    <button
      className="doc-card"
      title={path}
      onClick={() => openFilePreview(path)}
    >
      <FileTypeIcon kind={fileIconKind(path)} />
      <span className="doc-card-meta">
        <span className="doc-card-name">{name}</span>
        {file?.size != null && (
          <span className="doc-card-size">{formatBytes(file.size)}</span>
        )}
      </span>
    </button>
  );
}

function DocMention({ path }: { path: string }) {
  const file = useFile(path, "doc");
  return <DocCard file={file} path={path} />;
}

export type MentionPart =
  | { kind: "text"; text: string }
  | { kind: "image" | "doc"; path: string };

/** Split user text into plain runs and the image or document mentions in it. */
export function splitMentions(text: string): MentionPart[] {
  const parts: MentionPart[] = [];
  let last = 0;
  for (const match of text.matchAll(MENTION)) {
    const path = match[2];
    const at = (match.index ?? 0) + match[1].length;
    if (at > last) parts.push({ kind: "text", text: text.slice(last, at) });
    parts.push({ kind: IMAGE_EXT.test(path) ? "image" : "doc", path });
    last = at + path.length + 1;
  }
  if (parts.length === 0) return [{ kind: "text", text }];
  if (last < text.length) parts.push({ kind: "text", text: text.slice(last) });
  return parts;
}

/** User turn text, with an `@image.png` or `@report.pdf` mention drawn as what it names. */
export function UserText({ text }: { text: string }) {
  return (
    <>
      {splitMentions(text).map((part, i) => {
        if (part.kind === "text") return <span key={i}>{part.text}</span>;
        return part.kind === "image" ? (
          <ImageMention key={i} path={part.path} />
        ) : (
          <DocMention key={i} path={part.path} />
        );
      })}
    </>
  );
}
