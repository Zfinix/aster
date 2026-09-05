import { latestReview, type Conversation } from "../lib/session";
import type { SourceKind } from "../lib/types";
import { Composer, type ComposerBinding } from "../components/Composer";
import { ReviewBadge } from "../components/ReviewBadge";
import { Mark } from "../components/Mark";
import { Tip } from "../components/Tip";
import {
  DiffIcon,
  GitBranchIcon,
  GitPullRequestIcon,
  PencilIcon,
} from "../components/icons";
import {
  CHAT_OPENERS,
  REVIEW_OPENERS,
  TIPS,
  useRotation,
} from "../lib/greeting";

const TIP_MS = 11000;

const QUICK: { kind: SourceKind; label: string; icon: React.ReactNode }[] = [
  { kind: "working", label: "Review working tree", icon: <PencilIcon /> },
  { kind: "range", label: "Review branch", icon: <GitBranchIcon /> },
  { kind: "pr", label: "Review a pull request", icon: <GitPullRequestIcon /> },
  { kind: "diff", label: "Review a diff file", icon: <DiffIcon /> },
];

export function HomeView({
  composer,
  conversations,
  onOpen,
  intent = "chat",
}: {
  composer: ComposerBinding;
  conversations: Conversation[];
  onOpen: (id: string) => void;
  intent?: "review" | "chat";
}) {
  const opener = useRotation(
    intent === "review" ? REVIEW_OPENERS : CHAT_OPENERS,
  );
  const tip = useRotation(TIPS, TIP_MS);

  const quick = (kind: SourceKind) => {
    composer.onIntent("review");
    if (kind === "diff") {
      composer.onAttach();
      return;
    }
    composer.onSource(kind);
  };

  return (
    <>
      <div className="empty">
        <Mark px={2.6} interactive />
        <h1 className="empty-title">{opener}</h1>
        <Composer variant="home" {...composer} />
        <div className="quick-row">
          {QUICK.map((q) => (
            <button
              key={q.kind}
              type="button"
              className="quick"
              data-active={
                intent === "review" && composer.opts.sourceKind === q.kind
              }
              onClick={() => quick(q.kind)}
            >
              {q.icon}
              {q.label}
            </button>
          ))}
        </div>
        {conversations.length > 0 && (
          <div className="recent">
            <div className="recent-label">Recent</div>
            {conversations.slice(0, 6).map((c) => {
              const review = latestReview(c);
              return (
                <button
                  key={c.id}
                  type="button"
                  className="recent-row"
                  onClick={() => onOpen(c.id)}
                >
                  <span className="recent-title">{c.title}</span>
                  {review && <ReviewBadge review={review} />}
                  <span className="recent-meta">
                    {c.repoName} · {c.whenLabel}
                  </span>
                </button>
              );
            })}
          </div>
        )}
      </div>
      <p className="empty-tip" key={tip}>
        <Tip text={tip} />
      </p>
    </>
  );
}
