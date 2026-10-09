import "@openuidev/react-ui/styles/index.css";
import "../openui.css";

import { BuiltinActionType, Renderer, type ActionEvent } from "@openuidev/react-lang";
import { ThemeProvider } from "@openuidev/react-ui";
import { useContext } from "react";
import { post } from "../lib/host";
import { useColorMode } from "../lib/colorMode";
import { library, theme, withAnswers } from "../lib/openui";
import { ReplyContext } from "../lib/reply";

/** An ```openui block from the model, drawn with OpenUI's components as it
 *  streams. Buttons that talk back to the assistant send a new message. */
export function OpenUIBlock({ code, open }: { code: string; open: boolean }) {
  const reply = useContext(ReplyContext);
  const mode = useColorMode();
  const onAction = (event: ActionEvent) => {
    if (event.type === BuiltinActionType.ContinueConversation) {
      reply.send(withAnswers(event.humanFriendlyMessage, event.formState));
    } else if (event.type === BuiltinActionType.OpenUrl && typeof event.params.url === "string") {
      post({ type: "openExternal", url: event.params.url });
    }
  };
  return (
    <div className="openui-block">
      <ThemeProvider mode={mode} lightTheme={theme} darkTheme={theme} cssSelector=".openui-block">
        <Renderer
          response={code}
          library={library}
          isStreaming={open && reply.streaming}
          onAction={onAction}
        />
      </ThemeProvider>
    </div>
  );
}
