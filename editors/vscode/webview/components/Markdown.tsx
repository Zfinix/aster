import type { ReactElement, ReactNode } from "react";
import { CodeBlock } from "./CodeBlock";
import { Link } from "./Link";

/**
 * Markdown rendering for assistant replies. Deliberately hand-rolled rather
 * than a library: the webview runs under a strict CSP and this covers what the
 * agent actually emits — fences, headings, lists, quotes, links, and emphasis.
 */
export function Markdown({ text }: { text: string }) {
  const blocks: ReactElement[] = [];
  const fence = /```(\w*)\n([\s\S]*?)```/g;
  let cursor = 0;
  let match: RegExpExecArray | null;
  let key = 0;

  while ((match = fence.exec(text)) !== null) {
    if (match.index > cursor) {
      blocks.push(<Prose key={key++} text={text.slice(cursor, match.index)} />);
    }
    blocks.push(<CodeBlock key={key++} code={match[2].replace(/\n$/, "")} lang={match[1]} />);
    cursor = match.index + match[0].length;
  }
  if (cursor < text.length) {
    blocks.push(<Prose key={key++} text={text.slice(cursor)} />);
  }
  return <>{blocks}</>;
}

const BULLET = /^\s*[-*+]\s+(.*)$/;
const NUMBERED = /^\s*\d+[.)]\s+(.*)$/;
const HEADING = /^(#{1,6})\s+(.*)$/;
const QUOTE = /^\s*>\s?/;
const RULE = /^\s*([-*_])\1{2,}\s*$/;
const TABLE_ROW = /^\s*\|.*\|\s*$/;
const TABLE_SEP = /^\s*\|[\s:|-]+\|\s*$/;
/** Panel headings start at h3: the thread is a transcript, not a document. */
const HEADING_TAGS = ["h3", "h4", "h5", "h6", "h6", "h6"] as const;

function isBlockStart(line: string): boolean {
  return (
    RULE.test(line) ||
    HEADING.test(line) ||
    QUOTE.test(line) ||
    BULLET.test(line) ||
    NUMBERED.test(line) ||
    TABLE_ROW.test(line)
  );
}

function Prose({ text }: { text: string }) {
  const lines = text.split("\n");
  const out: ReactElement[] = [];
  let i = 0;
  let key = 0;

  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i++;
      continue;
    }

    if (RULE.test(line)) {
      out.push(<hr key={key++} className="md-rule" />);
      i++;
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      const Tag = HEADING_TAGS[heading[1].length - 1];
      out.push(
        <Tag key={key++} className="md-heading">
          {inline(heading[2])}
        </Tag>
      );
      i++;
      continue;
    }

    if (QUOTE.test(line)) {
      const buf: string[] = [];
      while (i < lines.length && QUOTE.test(lines[i])) {
        buf.push(lines[i].replace(QUOTE, ""));
        i++;
      }
      out.push(
        <blockquote key={key++} className="md-quote">
          {inline(buf.join(" "))}
        </blockquote>
      );
      continue;
    }

    if (TABLE_ROW.test(line) && i + 1 < lines.length && TABLE_SEP.test(lines[i + 1])) {
      const header = splitRow(line);
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length && TABLE_ROW.test(lines[i])) {
        rows.push(splitRow(lines[i]));
        i++;
      }
      out.push(
        <div key={key++} className="md-table-wrap">
          <table className="md-table">
            <thead>
              <tr>
                {header.map((cell, n) => (
                  <th key={n}>{inline(cell)}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((row, r) => (
                <tr key={r}>
                  {row.map((cell, n) => (
                    <td key={n}>{inline(cell)}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
      continue;
    }

    const ordered = !BULLET.test(line) && NUMBERED.test(line);
    if (ordered || BULLET.test(line)) {
      const pattern = ordered ? NUMBERED : BULLET;
      const items: string[] = [];
      while (i < lines.length && pattern.test(lines[i])) {
        items.push(pattern.exec(lines[i])![1]);
        i++;
        // An indented continuation belongs to the item above, not a new one.
        while (i < lines.length && /^\s{2,}\S/.test(lines[i]) && !isBlockStart(lines[i])) {
          items[items.length - 1] += ` ${lines[i].trim()}`;
          i++;
        }
      }
      const List = ordered ? "ol" : "ul";
      out.push(
        <List key={key++} className="md-list">
          {items.map((item, n) => (
            <li key={n}>{inline(item)}</li>
          ))}
        </List>
      );
      continue;
    }

    // The first line is taken unconditionally. A block start that reached here
    // is one no branch above claimed — a table row whose separator has not
    // streamed in yet — and leaving it for the loop condition to reject would
    // spin on the same line forever.
    const buf: string[] = [line.trim()];
    i++;
    while (i < lines.length && lines[i].trim() && !isBlockStart(lines[i])) {
      buf.push(lines[i].trim());
      i++;
    }
    out.push(
      <p key={key++} className="prose">
        {inline(buf.join(" "))}
      </p>
    );
  }

  return <>{out}</>;
}

/** Splits a `| a | b | c |` row into trimmed cells, dropping the outer pipes. */
function splitRow(line: string): string[] {
  return line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((c) => c.trim());
}

/**
 * Splits on ``code``, `code`, **bold**, *italic*, [text](url), and bare URLs.
 * Double backticks come first: matching the inner pair of a ``…`` span would
 * leave stray backticks beside the styled chip. A bare URL comes last, so a
 * URL already inside a markdown link is not matched twice.
 */
function inline(text: string): ReactNode[] {
  const parts: ReactNode[] = [];
  const pattern =
    /``([^`]+)``|`([^`]+)`|\*\*([^*]+)\*\*|\*([^*\n]+)\*|\[([^\]]+)\]\(([^)\s]+)\)|((?:https?|file):\/\/[^\s<>"'`]+)/g;
  let cursor = 0;
  let match: RegExpExecArray | null;
  let key = 0;

  while ((match = pattern.exec(text)) !== null) {
    if (match.index > cursor) {
      parts.push(text.slice(cursor, match.index));
    }
    const code = match[1] ?? match[2];
    if (code !== undefined) {
      parts.push(<code key={key++}>{code.trim()}</code>);
    } else if (match[3] !== undefined) {
      parts.push(<strong key={key++}>{inline(match[3])}</strong>);
    } else if (match[4] !== undefined) {
      parts.push(<em key={key++}>{inline(match[4])}</em>);
    } else if (match[5] !== undefined) {
      parts.push(
        <Link key={key++} url={match[6]}>
          {match[5]}
        </Link>
      );
    } else {
      // Trailing punctuation reads as the sentence's, not the URL's.
      const url = match[7].replace(/[.,;:!?)\]]+$/, "");
      parts.push(
        <Link key={key++} url={url}>
          {url}
        </Link>
      );
      cursor = match.index + url.length;
      continue;
    }
    cursor = match.index + match[0].length;
  }
  if (cursor < text.length) {
    parts.push(text.slice(cursor));
  }
  return parts;
}
