import { useEffect, useRef, useState } from "react";
import { onHostMessage, post } from "./host";

export type DictationState = "idle" | "listening" | "transcribing";

/** One press starts listening, the next turns the recording into text. The
 *  host runs `aster dictate`, relays its events as `dictation` messages, and
 *  shows failures as a notification. */
export function useDictation(onText: (text: string) => void) {
  const [state, setState] = useState<DictationState>("idle");
  const onTextRef = useRef(onText);
  onTextRef.current = onText;
  const pending = useRef(false);

  useEffect(() => {
    const off = onHostMessage((message) => {
      if (message.type !== "dictation" || !pending.current) return;
      const event = message.event;
      switch (event.type) {
        case "listening":
        case "transcribing":
          setState(event.type);
          break;
        case "transcript":
          pending.current = false;
          setState("idle");
          if (event.text) onTextRef.current(event.text);
          break;
        case "error":
          pending.current = false;
          setState("idle");
          break;
      }
    });
    return () => {
      off();
      if (pending.current) post({ type: "dictation", action: "cancel" });
    };
  }, []);

  const toggle = () => {
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

  return { state, toggle, cancel };
}
