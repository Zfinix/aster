import type { ComponentType } from "react";
import type { Finding, Severity } from "./types";
import { severityOf, SEV_RANK } from "./severity";
import {
  SevCriticalIcon,
  SevHighIcon,
  SevMediumIcon,
  SevLowIcon,
  SevInfoIcon,
} from "../components/icons";

export const SEV_CLS: Record<Severity, string> = {
  critical: "crit",
  high: "high",
  medium: "med",
  low: "low",
  info: "info",
};

export const SEV_ICON: Record<Severity, ComponentType<{ size?: number }>> = {
  critical: SevCriticalIcon,
  high: SevHighIcon,
  medium: SevMediumIcon,
  low: SevLowIcon,
  info: SevInfoIcon,
};

export const SEV_WORD: Record<Severity, string> = {
  critical: "critical",
  high: "high",
  medium: "medium",
  low: "low",
  info: "info",
};

export function money(n: number | null | undefined): string {
  if (n == null) return "";
  return `$${n.toFixed(n < 0.01 ? 4 : 3)}`;
}

export function topSeverity(findings: Finding[]): Severity {
  return findings
    .map((f) => severityOf(f.severity))
    .sort((a, b) => SEV_RANK[a] - SEV_RANK[b])[0];
}

export function clause(text: string): string {
  const first = text.split(/(?<=[.!?])\s/)[0] ?? text;
  return first.length > 110 ? `${first.slice(0, 107)}…` : first;
}
