import { execFile } from "child_process";
import { accessSync, constants } from "fs";
import { mkdir, lstat, readlink, symlink, unlink, writeFile } from "fs/promises";
import * as os from "os";
import * as path from "path";

/** Where a link goes, in the order we would rather use them. `~/.local/bin` is
 *  the one a user owns and modern shells already carry. */
function candidates(): string[] {
  const home = os.homedir();
  return [
    path.join(home, ".local", "bin"),
    "/opt/homebrew/bin",
    "/usr/local/bin",
    path.join(home, "bin"),
  ];
}

export function linkName(): string {
  return process.platform === "win32" ? "aster.cmd" : "aster";
}

/** The PATH a terminal has, which is the login shell's and not the editor's:
 *  an editor started from the Dock inherits neither the profile nor the PATH. */
async function shellPath(): Promise<string[]> {
  const shell = process.env.SHELL;
  if (process.platform === "win32" || !shell) {
    return (process.env.PATH ?? "").split(path.delimiter).filter(Boolean);
  }
  const printed = await new Promise<string>((resolve) => {
    execFile(shell, ["-lc", "printf %s \"$PATH\""], { timeout: 5000 }, (err, stdout) =>
      resolve(err ? (process.env.PATH ?? "") : stdout)
    );
  });
  return printed.split(path.delimiter).filter(Boolean);
}

/** The link this extension owns, if it is already in place somewhere. */
async function existingLink(): Promise<string | undefined> {
  for (const dir of candidates()) {
    const link = path.join(dir, linkName());
    try {
      const stat = await lstat(link);
      if (stat.isSymbolicLink() || stat.isFile()) return link;
    } catch {
      // Not there, try the next.
    }
  }
  return undefined;
}

async function pointsAtUs(link: string, binary: string): Promise<boolean> {
  try {
    return path.resolve(path.dirname(link), await readlink(link)) === binary;
  } catch {
    return false;
  }
}

/** True when a terminal would find some `aster` of its own. */
export async function onShellPath(): Promise<boolean> {
  const dirs = await shellPath();
  for (const dir of dirs) {
    try {
      await lstat(path.join(dir, linkName()));
      return true;
    } catch {
      // Keep looking.
    }
  }
  return false;
}

export interface LinkResult {
  ok: boolean;
  message: string;
  link?: string;
}

/** Puts `aster` on the user's PATH, the way an editor offers to install its own
 *  shell command. The link points at the CLI this build ships with, so the
 *  terminal and the panel run the same binary. */
export async function installShellCommand(binary: string): Promise<LinkResult> {
  const dirs = await shellPath();
  const onPath = (dir: string) => dirs.includes(dir);
  const home = os.homedir();
  const preferred = candidates().find((dir) => onPath(dir) && writable(dir));
  const target = preferred ?? path.join(home, ".local", "bin");

  try {
    await mkdir(target, { recursive: true });
    const link = path.join(target, linkName());
    await unlink(link).catch(() => {});
    if (process.platform === "win32") {
      // A symlink needs a privilege a normal account does not have, so a shim
      // that forwards its arguments does the same job.
      await writeFile(link, `@echo off\r\n"${binary}" %*\r\n`, "utf8");
    } else {
      await symlink(binary, link);
    }
    const reachable = onPath(target);
    return {
      ok: true,
      link,
      message: reachable
        ? `aster is on your PATH at ${link}. Open a new terminal to use it.`
        : `Linked at ${link}. Add it to your PATH with: export PATH="${target}:$PATH"`,
    };
  } catch (err) {
    return {
      ok: false,
      message: err instanceof Error ? err.message : String(err),
    };
  }
}

/** An extension folder carries its version, so the link a previous version made
 *  dangles after an update. Repoint it rather than leaving a broken command. */
export async function refreshShellCommand(binary: string): Promise<void> {
  const link = await existingLink();
  if (!link || process.platform === "win32") return;
  if (await pointsAtUs(link, binary)) return;
  try {
    // Only a link into some extension folder of ours is ours to repair; a real
    // binary the user installed themselves is left alone.
    const current = await readlink(link);
    if (!/aster-vscode|aster-dev\./.test(current)) return;
    await unlink(link);
    await symlink(binary, link);
  } catch {
    // A link we cannot read is not ours to fix.
  }
}

function writable(dir: string): boolean {
  try {
    accessSync(dir, constants.W_OK);
    return true;
  } catch {
    return false;
  }
}
