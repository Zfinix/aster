import { Button } from "@heroui/react";
import { latestReview, type Conversation } from "../lib/session";
import { money } from "../lib/review-format";
import { Composer, type ComposerBinding } from "../components/Composer";
import { ReviewBadge } from "../components/ReviewBadge";
import { Mark } from "../components/Mark";
import { Tip } from "../components/Tip";
import { CHAT_OPENERS, REVIEW_OPENERS, TIPS, useRotation } from "../lib/greeting";

const TIP_MS = 11000;

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
  const opener = useRotation(intent === "review" ? REVIEW_OPENERS : CHAT_OPENERS);
  const tip = useRotation(TIPS, TIP_MS);

  return (
    <div className="hero">
      <Mark px={3.2} interactive />
      <h1>{opener}</h1>
      <Composer variant="home" {...composer} />
      {/* Remounting on the text replays the fade, so a new tip arrives rather
          than swapping in place. */}
      <p className="hero-tip" key={tip}>
        <Tip text={tip} />
      </p>

      {conversations.length > 0 && (
        <div className="home-work">
          <div className="h-label">Recent</div>
          {conversations.slice(0, 6).map((c) => {
            const review = latestReview(c);
            return (
              <Button key={c.id} className="h-row" onPress={() => onOpen(c.id)}>
                <div className="h-main">
                  <div className="h-title">{c.title}</div>
                  <div className="h-meta">
                    {c.whenLabel} · {c.repoName}
                  </div>
                </div>
                {review && <ReviewBadge review={review} />}
                <span className="h-stats">
                  {review
                    ? `${review.findings.length} finding${review.findings.length === 1 ? "" : "s"}${
                        review.usage?.estimated_cost_usd != null
                          ? ` · ${money(review.usage.estimated_cost_usd)}`
                          : ""
                      }`
                    : `${c.turns.length} message${c.turns.length === 1 ? "" : "s"}`}
                </span>
              </Button>
            );
          })}
        </div>
      )}
    </div>
  );
}
