/** One release note. Names in backticks become code, the way the notes are
 *  written, and the prose between them wraps on its own. */
export function Announcement({ text }: { text: string }) {
  const parts = text.split(/`([^`]+)`/g);
  return (
    <>
      {parts.map((part, i) => (i % 2 === 1 ? <code key={i}>{part}</code> : <span key={i}>{part}</span>))}
    </>
  );
}
