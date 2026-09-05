import { useEffect, useState } from "react";
import { Code } from "./Code";
import { Modal } from "./Modal";
import { onHostMessage, post } from "../lib/host";
import { onFilePreviewOpen } from "../lib/filePreview";
import { DocCard, fileUrl, formatBytes, type PreviewFile } from "./UserText";
import { inEditor } from "../lib/host";

const IMAGE_EXT = /\.(png|jpe?g|gif|webp|svg|bmp|ico)$/i;
const DOC_EXT = /\.(pdf|docx?|xlsx?|pptx?|odt|ods|odp|rtf|epub|zip|gz|tar|rar|7z)$/i;

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
        // In a browser an image is one GET away and a document is only its
        // card; text still needs the host to read it.
        if (!inEditor && IMAGE_EXT.test(path)) {
          setFile({ path, content: "", truncated: false, image: fileUrl(path) });
          return;
        }
        if (!inEditor && DOC_EXT.test(path)) {
          setFile({ path, content: "", truncated: false });
          return;
        }
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
          {file.image ? (
            <img className="file-preview-image" src={file.image} alt={file.path} />
          ) : file.doc || DOC_EXT.test(file.path) ? (
            <div className="file-preview-doc">
              <DocCard file={file} path={file.path} />
              {file.size != null && (
                <div className="file-preview-note">
                  No inline preview for this format. {formatBytes(file.size)}.
                </div>
              )}
            </div>
          ) : (
            <pre className="file-preview-code">
              <Code code={file.content} lang={file.lang && LANG_BY_EXT[file.lang]} />
            </pre>
          )}
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