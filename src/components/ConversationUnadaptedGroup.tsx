import { useState, type ReactNode } from "react";
import { unadaptedGroupLabel } from "../lib/conversationEventDisplay";
import type { ConversationEvent } from "../types";

export function ConversationUnadaptedGroup({
  events,
  renderEvent,
  defaultOpen = false,
}: {
  events: ConversationEvent[];
  renderEvent: (event: ConversationEvent) => ReactNode;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <details
      className="conversation-unadapted-group"
      open={open}
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary>{unadaptedGroupLabel(events)}</summary>
      {open ? events.map((event) => renderEvent(event)) : null}
    </details>
  );
}
