import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { QuestionPrompt } from "./QuestionPrompt";
import type { Question } from "../lib/thread";

const question: Question = {
  header: "Where to wire it",
  question: "Which one?",
  options: ["This project", "A smoke script"],
};

describe("QuestionPrompt", () => {
  it("offers a free-text answer next to the lettered options", () => {
    const out = renderToStaticMarkup(
      <QuestionPrompt question={question} onAnswer={() => {}} />,
    );
    expect(out).toContain("question-other");
    expect(out).toContain("Other: type your own answer and press Enter");
  });
});
