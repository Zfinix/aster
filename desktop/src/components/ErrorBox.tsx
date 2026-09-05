import type { ReactNode } from "react";
import { parseError } from "../lib/error";

export function ErrorBox({ message, children }: { message?: string; children?: ReactNode }) {
  if (!message) return null;
  const { label, hint, detail } = parseError(message);
  return (
    <div className="error-box">
      <div className="error-box-head">
        <span className="error-box-label">{label}</span>
      </div>
      {hint && <div className="error-box-hint">{hint}</div>}
      {detail && <div className="error-box-detail">{detail}</div>}
      {children && <div className="error-box-actions">{children}</div>}
    </div>
  );
}
