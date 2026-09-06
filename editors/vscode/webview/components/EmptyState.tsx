import { useState } from "react";
import type { Provider, SetupInfo } from "../../src/protocol";
import type { LoginState } from "../lib/login";
import { Announcement } from "./Announcement";
import { InstallCard } from "./InstallCard";
import { Mark } from "./Mark";
import { SetupCard } from "./SetupCard";
import { Tip } from "./Tip";
import { CloseIcon } from "./icons";
import { post } from "../lib/host";
import { OPENERS, TIPS, useRotation } from "../lib/greeting";

const TIP_MS = 11000;

export function EmptyState({
  binaryOk,
  setup,
  announcements,
  login,
  providers,
}: {
  binaryOk: boolean;
  setup: SetupInfo | null;
  announcements: { id: string; text: string }[] | null;
  login: LoginState | null;
  providers: Provider[];
}) {
  const opener = useRotation(OPENERS);
  const tip = useRotation(TIPS, TIP_MS);
  const [announcementsGone, setAnnouncementsGone] = useState(false);

  if (!binaryOk) {
    return (
      <div className="empty">
        <Mark px={2.6} interactive />
        <div className="setup-stream">
          <InstallCard />
        </div>
      </div>
    );
  }

  if (setup) {
    return (
      <div className="empty">
        <Mark px={2.6} interactive />
        <div className="setup-stream">
          <SetupCard login={login} providers={providers} />
        </div>
      </div>
    );
  }

  const showAnnouncements = announcements && announcements.length > 0 && !announcementsGone;

  return (
    <div className="empty">
      <Mark px={2.6} interactive />
      <h1 className="empty-title">{opener}</h1>
      {showAnnouncements && (
        <div className="empty-announcements">
          <div className="empty-announcements-head">
            <span className="empty-announcements-title">What's new</span>
            <button
              type="button"
              className="icon-btn empty-announcements-dismiss"
              title="Dismiss"
              aria-label="Dismiss announcements"
              onClick={() => {
                setAnnouncementsGone(true);
                post({ type: "dismissAnnouncements", ids: announcements.map((a) => a.id) });
              }}
            >
              <CloseIcon />
            </button>
          </div>
          <ul className="empty-announcements-items">
            {announcements.map((a) => (
              <li key={a.id}>
                <Announcement text={a.text} />
              </li>
            ))}
          </ul>
        </div>
      )}
      {/* The card takes the tip's slot: one footnote under the greeting, not two.
          Remounting on the text replays the fade, so a new tip arrives rather
          than swapping in place. */}
      {!showAnnouncements && (
        <p className="empty-tip" key={tip}>
          <Tip text={tip} />
        </p>
      )}
    </div>
  );
}
