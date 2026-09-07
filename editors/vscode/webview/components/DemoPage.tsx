import { useEffect, useState } from "react";
import { relativeTime } from "../lib/history";
import { Mark } from "./Mark";
import {
  ProjectsSidebar,
  type SidebarChat,
  type SidebarProject,
} from "./ProjectsSidebar";

const HOUR = 3_600_000;
const DAY = 24 * HOUR;

const ago = (ms: number) => new Date(Date.now() - ms).toISOString();

const DEMO_PROJECTS: SidebarProject[] = [
  {
    name: "aster",
    path: "~/projects/work-projects/aster",
    chats: [
      { id: "aster-1", title: "Sidebar for projects and their chats", when: relativeTime(ago(2 * HOUR)) },
      { id: "aster-2", title: "Fix flaky session roundtrip test", when: relativeTime(ago(2 * DAY)) },
      { id: "aster-3", title: "Rewrite the 0.5.0 release notes", when: relativeTime(ago(5 * DAY)) },
    ],
  },
  {
    name: "landing-site",
    path: "~/projects/landing-site",
    chats: [
      { id: "site-1", title: "Pricing page copy pass", when: relativeTime(ago(26 * HOUR)) },
      { id: "site-2", title: "Move the blog off the SPA router", when: relativeTime(ago(9 * DAY)) },
    ],
  },
  {
    name: "pinboard",
    path: "~/projects/pinboard",
    chats: [
      { id: "pin-1", title: "Tag search keeps missing pinned cards", when: relativeTime(ago(3 * DAY)) },
    ],
  },
];

/** The `/demo` tab: a scaffold for the projects sidebar that `aster serve`
 *  will grow. Sample data stands in for the real list until the host learns
 *  to name projects and their sessions; the shape of that list is what this
 *  page exists to settle. */
export function DemoPage() {
  const [active, setActive] = useState<string | null>(null);
  const [opened, setOpened] = useState<{ project: string; chat: SidebarChat } | null>(null);

  useEffect(() => {
    document.title = "Aster demo";
  }, []);

  return (
    <div className="demo-page">
      <ProjectsSidebar
        projects={DEMO_PROJECTS}
        activeChat={active}
        onOpenChat={(project, chat) => {
          setActive(chat.id);
          setOpened({ project: project.name, chat });
        }}
        onNewChat={(project) => setOpened({ project: project.name, chat: { id: "new", title: "New chat", when: "now" } })}
      />

      <main className="demo-main">
        {opened ? (
          <>
            <h1 className="demo-thread-title">{opened.chat.title}</h1>
            <p className="demo-thread-sub">
              {opened.project} · {opened.chat.when}
            </p>
            <p className="demo-thread-note">
              The conversation opens here once the sidebar is wired to saved sessions.
            </p>
          </>
        ) : (
          <>
            <Mark px={3} />
            <h1 className="demo-empty-title">Pick a chat, or start one</h1>
            <p className="demo-thread-note">
              This page is a stand-in for the real thing: the sidebar is the part being
              designed, and everything to its right is what a thread will grow into.
            </p>
          </>
        )}
      </main>
    </div>
  );
}
