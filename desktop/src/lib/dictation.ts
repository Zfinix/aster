import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { cancelDictation, startDictation, stopDictation } from "./aster";

export type DictationState = "idle" | "listening" | "transcribing";

type DictationEvent =
  | { type: "listening" }
  | { type: "transcribing" }
  | { type: "transcript"; text: string }
  | { type: "error"; message: string; detail: string | null }
  | { type: "closed" };

const STOPPED = "Recording stopped unexpectedly. Try again.";

/** One press starts listening, the next turns the recording into text. */
export function useDictation(onText: (text: string) => void, onError: (message: string) => void) {
  const [state, setState] = useState<DictationState>("idle");
  const handlers = useRef({ onText, onError });
  handlers.current = { onText, onError };
  const pending = useRef(false);

  useEffect(() => {
    const settle = () => {
      pending.current = false;
      setState("idle");
    };
    const unlisten = listen<string>("aster://dictation", (e) => {
      let ev: DictationEvent;
      try {
        ev = JSON.parse(e.payload) as DictationEvent;
      } catch {
        return;
      }
      switch (ev.type) {
        case "listening":
        case "transcribing":
          if (pending.current) setState(ev.type);
          break;
        case "transcript":
          settle();
          if (ev.text) handlers.current.onText(ev.text);
          break;
        case "error":
          settle();
          if (ev.detail) console.warn("dictation:", ev.detail);
          handlers.current.onError(ev.message);
          break;
        case "closed":
          if (!pending.current) break;
          settle();
          handlers.current.onError(STOPPED);
          break;
      }
    });
    return () => {
      unlisten.then((u) => u());
      if (pending.current) cancelDictation().catch(() => {});
    };
  }, []);

  const fail = (err: unknown) => {
    pending.current = false;
    setState("idle");
    handlers.current.onError(String(err));
  };

  const toggle = () => {
    if (state === "idle") {
      pending.current = true;
      setState("listening");
      startDictation().catch(fail);
    } else if (state === "listening") {
      setState("transcribing");
      stopDictation().catch(fail);
    }
  };

  const cancel = () => {
    if (state !== "listening") return;
    pending.current = false;
    setState("idle");
    cancelDictation().catch(() => {});
  };

  return { state, toggle, cancel };
}
