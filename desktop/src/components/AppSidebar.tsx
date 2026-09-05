import { useState } from "react";
import { type Conversation } from "../lib/session";
import { SessionRow } from "./SessionRow";
import {
  ChevronIcon,
  FolderIcon,
  GearIcon,
  NewChatIcon,
  ReviewIcon,
  SidebarIcon,
} from "./icons";

const COLLAPSED_KEY = "aster.collapsedRepos";

export function AppSidebar({
  conversations,
  activeId,
  repoPath,
  onNewChat,
  onNewReview,
  onOpen,
  onRename,
  onDelete,
  onRerun,
  onCopyBrief,
  onCollapse,
  onOpenSettings,
}: {
  conversations: Conversation[];
  activeId: string | null;
  repoPath: string;
  onNewChat: () => void;
  onNewReview: () => void;
  onOpen: (id: string) => void;
  onRename: (id: string, title: string) => void;
  onDelete: (id: string) => void;
  onRerun: (id: string) => void;
  onCopyBrief: (id: string) => void;
  onCollapse: () => void;
  onOpenSettings: () => void;
}) {
  const [collapsed, setCollapsed] = useState<Set<string>>(() => {
    try {
      return new Set(JSON.parse(localStorage.getItem(COLLAPSED_KEY) || "[]"));
    } catch {
      return new Set();
    }
  });

  const toggleGroup = (repo: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      next.has(repo) ? next.delete(repo) : next.add(repo);
      localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...next]));
      return next;
    });

  const groups = new Map<string, Conversation[]>();
  for (const c of conversations) {
    groups.set(c.repoName, [...(groups.get(c.repoName) ?? []), c]);
  }

  const repoName = repoPath ? repoPath.split(/[\\/]/).pop() : null;

  return (
    <aside className="sidebar">
      <div className="sidebar-top" data-tauri-drag-region>
        <button
          type="button"
          className="ghost icon-action"
          aria-label="Hide sidebar"
          title="Hide sidebar"
          onClick={onCollapse}
        >
          <SidebarIcon />
        </button>
      </div>

      <nav className="sidebar-nav">
        <button type="button" className="nav-row" onClick={onNewChat}>
          <NewChatIcon />
          <span>New chat</span>
          <kbd className="nav-kbd">⌘N</kbd>
        </button>
        <button type="button" className="nav-row" onClick={onNewReview}>
          <ReviewIcon />
          <span>New review</span>
          <kbd className="nav-kbd">⌘R</kbd>
        </button>
      </nav>

      <div className="sidebar-list">
        {conversations.length === 0 && (
          <div className="sidebar-empty">Conversations show up here, grouped by project.</div>
        )}
        {[...groups.entries()].map(([repo, list]) => {
          const open = !collapsed.has(repo);
          return (
            <div key={repo} className="group">
              <button
                type="button"
                className="group-head"
                aria-expanded={open}
                onClick={() => toggleGroup(repo)}
              >
                <FolderIcon />
                <span className="group-name">{repo}</span>
                <span className="group-count">{list.length}</span>
                <ChevronIcon open={open} />
              </button>
              {open &&
                list.map((c) => (
                  <SessionRow
                    key={c.id}
                    convo={c}
                    active={c.id === activeId}
                    onOpen={() => onOpen(c.id)}
                    onRename={(title) => onRename(c.id, title)}
                    onDelete={() => onDelete(c.id)}
                    onRerun={() => onRerun(c.id)}
                    onCopyBrief={() => onCopyBrief(c.id)}
                  />
                ))}
            </div>
          );
        })}
      </div>

      <div className="sidebar-foot">
        <span className="workspace" title={repoPath || "No project open"}>
          <span className="workspace-dot" data-on={!!repoName} />
          <span className="workspace-name">{repoName ?? "No project"}</span>
        </span>
        <button
          type="button"
          className="ghost icon-action"
          title="Settings"
          aria-label="Settings"
          onClick={onOpenSettings}
        >
          <GearIcon />
        </button>
      </div>
    </aside>
  );
}
