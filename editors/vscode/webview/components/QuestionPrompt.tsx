import { useEffect } from "react";
import type { Question } from "../lib/thread";
import { Modal } from "./Modal";

const letter = (index: number) => String.fromCharCode(65 + index);

/** Modal shown while `ask_user` blocks the turn: lettered options, answerable
 *  by click or by pressing A/B/C or 1/2/3. Skipping answers `null`, which hands
 *  the decision back to the agent rather than cancelling the turn. */
export function QuestionPrompt({
  question,
  onAnswer,
}: {
  question: Question;
  onAnswer: (choice: string | null) => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const target = e.target as HTMLElement | null;
      if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA")) return;
      let index = -1;
      if (/^[1-9]$/.test(e.key)) {
        index = Number(e.key) - 1;
      } else if (/^[a-z]$/i.test(e.key)) {
        index = e.key.toUpperCase().charCodeAt(0) - 65;
      }
      if (index >= 0 && index < question.options.length) {
        e.preventDefault();
        onAnswer(question.options[index]);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [question, onAnswer]);

  return (
    <Modal label={question.header} className="question" align="bottom">
        <div className="question-head">{question.header}</div>
        <div className="question-body">{question.question}</div>
        <div className="question-options">
          {question.options.map((option, i) => (
            <button key={option} className="question-option" onClick={() => onAnswer(option)}>
              <span className="question-key">{letter(i)}</span>
              <span className="question-option-text">{option}</span>
            </button>
          ))}
        </div>
        <button className="link question-skip" onClick={() => onAnswer(null)}>
          Skip and let Aster decide
        </button>
    </Modal>
  );
}
