import { useEffect, useRef } from "react";
import { Button, TextArea } from "@heroui/react";
import type { ReviewOpts, SourceKind } from "../lib/types";
import { SOURCE_LABELS } from "../lib/session";
import { RepoIcon, SendIcon } from "./icons";
import { ModelMenu } from "./ModelMenu";
import { PlusMenu } from "./PlusMenu";

export type ComposerBinding = Omit<Props, "variant">;

interface Props {
  variant: "home" | "foot";
  prompt: string;
  setPrompt: (s: string) => void;
  onAsk: () => void;
  onReview: () => void;
  busy: boolean;
  reviewing: boolean;
  canReview: boolean;
  opts: ReviewOpts;
  repoName: string;
  repoOptions: { value: string; label: string; hint?: string }[];
  onRepo: (value: string) => void;
  onSource: (kind: SourceKind) => void;
  model: string;
  models: string[];
  onModel: (value: string) => void;
  onAddModel: (value: string) => void;
  onAttach: () => void;
}

export function Composer(props: Props) {
  const {
    variant,
    prompt,
    setPrompt,
    onAsk,
    onReview,
    busy,
    reviewing,
    canReview,
    opts,
    repoName,
    repoOptions,
    onRepo,
    onSource,
    model,
    models,
    onModel,
    onAddModel,
    onAttach,
  } = props;
  const ref = useRef<HTMLTextAreaElement>(null);

  const canAsk = !!prompt.trim() && !busy;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 140)}px`;
  }, [prompt]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey && canAsk) {
      e.preventDefault();
      onAsk();
    }
  };

  const plusMenu = (
    <PlusMenu
      opts={opts}
      repoOptions={repoOptions}
      onRepo={onRepo}
      onSource={onSource}
      onAttach={onAttach}
      direction="up"
    />
  );

  const modelPill = (
    <ModelMenu
      model={model}
      models={models}
      onModel={onModel}
      onAddModel={onAddModel}
      direction="up"
    />
  );

  return (
    <div className={variant === "home" ? "home-composer" : ""}>
      <div className="composer">
        <TextArea
          ref={ref}
          value={prompt}
          rows={1}
          spellCheck={false}
          placeholder={
            variant === "home"
              ? "Ask Aster anything (Enter), or set a focus and hit Review"
              : "Ask a follow-up (Enter), or run another review"
          }
          aria-label="Message Aster"
          onChange={(e) => setPrompt(e.target.value)}
          onKeyDown={onKeyDown}
        />

        {variant === "home" ? (
          <div className="composer-row">
            {plusMenu}
            {modelPill}
            <span className="spacer" />
            <Button className="btn btn-ghost" isDisabled={!canAsk} onPress={onAsk}>
              Ask
            </Button>
            <Button
              className="btn btn-primary"
              isDisabled={!canReview || busy}
              onPress={onReview}
            >
              {reviewing ? "Reviewing" : "Review"}
            </Button>
          </div>
        ) : (
          <div className="composer-row">
            {plusMenu}
            {modelPill}
            <span className="spacer" />
            <Button
              className="btn btn-ghost"
              isDisabled={!canReview || busy}
              onPress={onReview}
            >
              {reviewing ? "Reviewing" : "Review"}
            </Button>
            <Button
              className="send"
              aria-label="Send message"
              isDisabled={!canAsk}
              onPress={onAsk}
            >
              <SendIcon />
            </Button>
          </div>
        )}
      </div>

      {variant === "foot" && (
        <div className="under-row">
          <span className="u-item">
            <RepoIcon />
            {repoName || "no repo"}
          </span>
          <span className="u-item mono">{SOURCE_LABELS[opts.sourceKind]}</span>
          <span className="grow" />
          <span className="u-item mono">
            confidence ≥ {opts.minConfidence.toFixed(2)}
          </span>
        </div>
      )}
    </div>
  );
}
