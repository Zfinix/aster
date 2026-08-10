import { HistoryIcon, NewChatIcon } from "./icons";

/** Compact action bar above the thread: two buttons on the left, conversation
 *  title centered. No border underline. */
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
      <div className="toolbar-actions">
        <button className="ghost" onClick={onNewChat} title="New conversation" aria-label="New conversation">
          <NewChatIcon />
        </button>
        <button className="ghost" onClick={onHistory} title="Reopen a session" aria-label="History">
          <HistoryIcon />
        </button>
      </div>
      <span className="toolbar-title">{title}</span>
    </div>
  );
}
