import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateStage =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "none" }
  | { kind: "available"; version: string; notes?: string }
  | { kind: "downloading"; version: string; done: number; total?: number }
  | { kind: "ready"; version: string }
  | { kind: "error"; message: string };

/** Ask the release endpoint whether a newer signed build exists. Returns the
 *  pending `Update` handle (to download later) or `null` when up to date. */
export async function checkForUpdate(): Promise<Update | null> {
  return check();
}

/** Download and install a pending update, reporting byte progress through
 *  `onStage`, then relaunch into the new version. The caller owns the `Update`
 *  handle from {@link checkForUpdate}. */
export async function installUpdate(
  update: Update,
  onStage: (stage: UpdateStage) => void,
): Promise<void> {
  let total: number | undefined;
  let done = 0;
  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started":
        total = event.data.contentLength;
        onStage({ kind: "downloading", version: update.version, done: 0, total });
        break;
      case "Progress":
        done += event.data.chunkLength;
        onStage({ kind: "downloading", version: update.version, done, total });
        break;
      case "Finished":
        onStage({ kind: "ready", version: update.version });
        break;
    }
  });
  await relaunch();
}
