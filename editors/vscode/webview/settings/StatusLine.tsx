/** Whether a value is stored, and where: a dot says it at a glance and the
 *  file it came from reads as a sentence. A value is shown only when the
 *  control beside it does not already hold it. */
export function StatusLine({
  set,
  value,
  source,
}: {
  set: boolean;
  value?: string;
  source?: string;
}) {
  return (
    <p className="set-status" data-on={set}>
      <span className="set-status-dot" aria-hidden="true" />
      {set ? (
        <>
          {value ? <span className="mono">{value}</span> : "Set"}
          {source && <span className="set-status-from">in {source}</span>}
        </>
      ) : (
        "Not set"
      )}
    </p>
  );
}
