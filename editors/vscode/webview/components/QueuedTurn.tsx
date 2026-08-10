/** A message typed mid-run, held at the thread tail until the turn finishes. */
export function QueuedTurn({ text, onRemove }: { text: string; onRemove: () => void }) {
  return (
    <div className="turn-user turn-queued">
      <div className="turn-user-text">{text}</div>
      <div className="queued-foot">
        <span className="queued-badge">Queued</span>
        <button className="link queued-remove" onClick={onRemove}>
          Cancel
        </button>
      </div>
    </div>
  );
}
