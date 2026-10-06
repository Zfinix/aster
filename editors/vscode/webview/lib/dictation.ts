import { useCallback, useEffect, useRef, useState } from "react";
import { inEditor, onHostMessage, post } from "./host";

export type DictationState = "idle" | "listening" | "transcribing";

/** One press starts listening, the next turns the recording into text. The
 *  host runs `aster dictate` and relays its events as `dictation` messages.
 *  An editor shows failures as a notification; a page gets `error` to show. */
export function useDictation(onText: (text: string) => void) {
  const [state, setState] = useState<DictationState>("idle");
  const [error, setError] = useState<string | null>(null);
  const [model, setModel] = useState<string | null>(null);
  const onTextRef = useRef(onText);
  onTextRef.current = onText;
  const pending = useRef(false);

  useEffect(() => {
    const off = onHostMessage((message) => {
      if (message.type !== "dictation" || !pending.current) return;
      const event = message.event;
      switch (event.type) {
        case "listening":
          setState("listening");
          setModel(event.model ?? null);
          break;
        case "transcribing":
          setState("transcribing");
          break;
        case "transcript":
          pending.current = false;
          setState("idle");
          if (event.text) onTextRef.current(event.text);
          break;
        case "error":
          pending.current = false;
          setState("idle");
          if (!inEditor) setError(event.message);
          break;
      }
    });
    return () => {
      off();
      if (pending.current) post({ type: "dictation", action: "cancel" });
    };
  }, []);

  const toggle = () => {
    setError(null);
    if (state === "idle") {
      pending.current = true;
      setState("listening");
      post({ type: "dictation", action: "start" });
    } else if (state === "listening") {
      setState("transcribing");
      post({ type: "dictation", action: "stop" });
    }
  };

  const cancel = () => {
    if (state !== "listening") return;
    pending.current = false;
    setState("idle");
    post({ type: "dictation", action: "cancel" });
  };

  const dismiss = useCallback(() => setError(null), []);

  return { state, error, model, toggle, cancel, dismiss };
}
