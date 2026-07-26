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

const SMALL_NUMS = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
const num = (n: number) => (n < 10 ? SMALL_NUMS[n] : String(n));
const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/** How a colleague would sum up the review: lead with what matters, name the
 *  worst problems in prose, keep the numbers casual. */
export function reviewMessage(findings: Finding[], refutedCount: number): string {
  const n = findings.length;

  if (n === 0) {
    if (refutedCount > 0) {
      return `This diff looks good. I chased ${num(refutedCount)} possible issue${
        refutedCount === 1 ? "" : "s"
      }, but ${refutedCount === 1 ? "it" : "none of them"} held up under verification — nothing needs your attention.`;
    }
    return "This diff looks good — I didn't find anything worth flagging.";
  }

  const of = (sev: Severity) => findings.filter((f) => severityOf(f.severity) === sev);
  const crit = of("critical");
  const high = of("high");
  const rest = n - crit.length - high.length;
  const sentences: string[] = [];

  if (n === 1) {
    const f = findings[0];
    const sev = severityOf(f.severity);
    sentences.push(
      sev === "critical" || sev === "high"
        ? `One real problem here: “${f.title}”. It's ${SEV_WORD[sev]}, so I'd fix it before this goes anywhere.`
        : `Just one small thing: “${f.title}”. Not urgent, but worth a quick fix.`,
    );
  } else if (crit.length > 0) {
    const names = crit.slice(0, 2).map((f) => `“${f.title}”`).join(" and ");
    sentences.push(`I found ${num(n)} issues in this diff, and I'd hold off merging.`);
    sentences.push(
      crit.length === 1
        ? `The serious one is ${names} — fix that first.`
        : `${cap(num(crit.length))} are outright critical — ${names}${
            crit.length > 2 ? ", among others" : ""
          } — so start there.`,
    );
    if (rest + high.length > 0) {
      sentences.push(`The rest are smaller and can wait until those are handled.`);
    }
  } else if (high.length > 0) {
    sentences.push(
      `I found ${num(n)} issues. Nothing critical, but “${high[0].title}” stands out and deserves a look first.`,
    );
    if (rest > 0) sentences.push(`The other ${rest === 1 ? "one is" : `${num(rest)} are`} routine.`);
  } else {
    sentences.push(
      `I found ${num(n)} smaller issues — nothing urgent, mostly cleanups worth doing while you're in here.`,
    );
  }

  if (refutedCount > 0) {
    sentences.push(
      `I also ruled out ${num(refutedCount)} false alarm${refutedCount === 1 ? "" : "s"} along the way.`,
    );
  }
  return sentences.join(" ");
}

export function clause(text: string): string {
  const first = text.split(/(?<=[.!?])\s/)[0] ?? text;
  return first.length > 110 ? `${first.slice(0, 107)}…` : first;
}
