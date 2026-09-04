import { useEffect, useState } from "react";
import { Code } from "./Code";
import { Modal } from "./Modal";
import { onHostMessage, post } from "../lib/host";
import { onFilePreviewOpen } from "../lib/filePreview";

type PreviewFile = { path: string; lang?: string; content: string; truncated: boolean };

const LANG_BY_EXT: Record<string, string> = {
  rs: "rust",
  py: "python",
  sh: "bash",
  yml: "yaml",
  rb: "ruby",
  kt: "kotlin",
};

/** A peek at a file a reply or a tool call touched, opened over the thread. */
export function FilePreview() {
  const [file, setFile] = useState<PreviewFile | null>(null);
  const [open, setOpen] = useState(false);

  useEffect(
    () =>
      onFilePreviewOpen((path) => {
        setOpen(true);
        setFile(null);
        post({ type: "readFile", path, requestId: `preview-${Date.now()}` });
      }),
    [],
  );

  useEffect(
    () =>
      onHostMessage((message) => {
        if (message.type === "filePreview") setFile(message.file);
      }),
    [],
  );

  if (!open) return null;
  return (
    <Modal
      label={file?.path ?? "Loading…"}
      className="file-preview"
      align="center"
      onClose={() => setOpen(false)}
    >
      {file ? (
        <>
          <pre className="file-preview-code">
            <Code code={file.content} lang={file.lang && LANG_BY_EXT[file.lang]} />
          </pre>
          {file.truncated && (
            <div className="file-preview-note">Showing the head of the file.</div>
          )}
          <div className="file-preview-foot">
            <button className="ghost" onClick={() => post({ type: "openFile", path: file.path })}>
              Open file
            </button>
          </div>
        </>
      ) : (
        <div className="file-preview-note">Loading…</div>
      )}
    </Modal>
  );
}