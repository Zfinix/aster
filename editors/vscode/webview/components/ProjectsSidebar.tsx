import { useState } from "react";
import { ChevronIcon, FolderIcon, NewChatIcon, SearchIcon } from "./icons";

export interface SidebarChat {
  id: string;
  title: string;
  when: string;
}

export interface SidebarProject {
  name: string;
  path: string;
  chats: SidebarChat[];
}

/** One row a chat, folded under the project it was opened in. Data-driven, so
 *  the host can hand real repos and sessions to it later without a redesign. */
export function ProjectsSidebar({
  projects,
  activeChat,
  onOpenChat,
  onNewChat,
}: {
  projects: SidebarProject[];
  activeChat: string | null;
  onOpenChat: (project: SidebarProject, chat: SidebarChat) => void;
  onNewChat: (project: SidebarProject) => void;
}) {
  const [query, setQuery] = useState("");
  // Minimalist by default: the sidebar opens as a plain list of projects, and
  // a chat list only unfolds for the project you ask for.
  const [collapsed, setCollapsed] = useState<Set<string>>(
    () => new Set(projects.map((p) => p.name)),
  );

  const needle = query.trim().toLowerCase();
  const searching = needle.length > 0;
  const visible = searching
    ? projects
        .map((p) => ({
          ...p,
          chats: p.chats.filter((c) => c.title.toLowerCase().includes(needle)),
        }))
        .filter((p) => p.chats.length > 0)
    : projects;

  const toggle = (name: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      next.has(name) ? next.delete(name) : next.add(name);
      return next;
    });

  return (
    <aside className="projects-sidebar">
      <div className="projects-sidebar-top">
        <button
          type="button"
          className="projects-new-chat"
          onClick={() => {
            const project = visible[0];
            if (project) onNewChat(project);
          }}
        >
          <NewChatIcon />
          <span>New chat</span>
        </button>
      </div>

      <div className="projects-search">
        <SearchIcon />
        <input
          placeholder="Search chats…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          spellCheck={false}
          aria-label="Search chats"
        />
      </div>

      <nav className="projects-list">
        {visible.length === 0 && (
          <div className="projects-empty">
            {projects.length === 0 ? "No projects yet." : `Nothing matches "${query}".`}
          </div>
        )}
        {visible.map((project) => {
          // A search has to show its hits, whatever the fold says.
          const open = searching || !collapsed.has(project.name);
          return (
            <div key={project.path} className="projects-group">
              <button
                type="button"
                className="projects-group-head"
                aria-expanded={open}
                title={project.path}
                onClick={() => toggle(project.name)}
              >
                <FolderIcon />
                <span className="projects-group-name">{project.name}</span>
                <span className="projects-group-count">{project.chats.length}</span>
                <ChevronIcon open={open} />
              </button>
              {open &&
                project.chats.map((chat) => (
                  <button
                    key={chat.id}
                    type="button"
                    className="projects-chat-row"
                    data-active={chat.id === activeChat}
                    onClick={() => onOpenChat(project, chat)}
                  >
                    <span className="projects-chat-title">{chat.title || "Untitled chat"}</span>
                    <span className="projects-chat-when">{chat.when}</span>
                  </button>
                ))}
            </div>
          );
        })}
      </nav>
    </aside>
  );
}
