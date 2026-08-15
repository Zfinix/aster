import { HistoryIcon, NewChatIcon } from "./icons";

/** Compact action bar above the thread: the conversation title reads first,
 *  its actions sit out at the trailing edge where a toolbar's controls belong. */
export function Toolbar({
  title,
  onNewChat,
  onHistory,
}: {
  title: string;
  onNewChat: () => void;
  onHistory: () => void;
}) {
  return (
    <div className="toolbar">
      <span className="toolbar-title">{title}</span>
      <div className="toolbar-actions">
        <button
          className="ghost icon-action"
          onClick={onNewChat}
          title="New conversation"
          aria-label="New conversation"
        >
          <NewChatIcon />
        </button>
        <button
          className="ghost icon-action"
          onClick={onHistory}
          title="Reopen a session"
          aria-label="History"
        >
          <HistoryIcon />
        </button>
      </div>
    </div>
  );
}
