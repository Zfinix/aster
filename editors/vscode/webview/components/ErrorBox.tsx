import { parseError } from "../lib/error";
import { AlertIcon } from "./icons";

export function ErrorBox({ message }: { message?: string }) {
  if (!message) return null;
  const { label, detail } = parseError(message);
  return (
    <div className="error-box">
      <div className="error-box-head">
        <span className="error-box-icon">
          <AlertIcon />
        </span>
        <span className="error-box-label">{label}</span>
      </div>
      {detail && <div className="error-box-detail">{detail}</div>}
    </div>
  );
}