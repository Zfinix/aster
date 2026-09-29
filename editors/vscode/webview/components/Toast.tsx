import { useEffect } from "react";

const SHOWN_MS = 6000;

/** A short message over the composer that clears itself. The page has no
 *  editor to raise a notification, so it raises its own. */
export function Toast({ message, onDone }: { message: string; onDone: () => void }) {
  useEffect(() => {
    const timer = setTimeout(onDone, SHOWN_MS);
    return () => clearTimeout(timer);
  }, [message, onDone]);

  return (
    <div className="toast" role="alert" onClick={onDone}>
      {message}
    </div>
  );
}
