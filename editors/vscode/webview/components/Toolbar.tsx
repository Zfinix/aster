import { HistoryIcon, NewChatIcon, SoundOffIcon, SoundOnIcon } from "./icons";

/** Compact action bar above the thread: the conversation title reads first,
 *  its actions sit out at the trailing edge where a toolbar's controls belong. */
export function Toolbar({
  title,
  onNewChat,
  onHistory,
  soundsOn,
  onToggleSounds,
}: {
  title: string;
  onNewChat: () => void;
  onHistory: () => void;
  soundsOn: boolean;
  onToggleSounds: () => void;
}) {
  return (
    <div className="toolbar">
      <span className="toolbar-title">{title}</span>
      <div className="toolbar-actions">
        <button
          className="ghost icon-action"
          onClick={onToggleSounds}
          title={soundsOn ? "Mute sounds" : "Unmute sounds"}
          aria-label={soundsOn ? "Mute sounds" : "Unmute sounds"}
          aria-pressed={soundsOn}
        >
          {soundsOn ? <SoundOnIcon /> : <SoundOffIcon />}
        </button>
        <button
          className="ghost icon-action"
          onClick={onHistory}
          title="Reopen a session"
          aria-label="History"
        >
          <HistoryIcon />
        </button>
        <button
          className="ghost icon-action"
          onClick={onNewChat}
          title="New conversation"
          aria-label="New conversation"
        >
          <NewChatIcon />
        </button>
      </div>
    </div>
  );
}
