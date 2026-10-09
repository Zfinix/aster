import { createContext } from "react";

/** What a rendered reply can do beyond showing text: know whether it is still
 *  arriving, and send a follow-up message on the user's behalf. */
export interface Reply {
  streaming: boolean;
  send: (text: string) => void;
}

export const ReplyContext = createContext<Reply>({ streaming: false, send: () => {} });
