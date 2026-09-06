import {
  resolveWorkNotesPosterStyle,
  type WorkNotesPosterRenderProps,
} from "./posterStyleRegistry";

export function WorkNotesPoster({
  styleId,
  ...renderProps
}: WorkNotesPosterRenderProps & { styleId?: string | null }) {
  const Style = resolveWorkNotesPosterStyle(styleId).Component;
  return <Style {...renderProps} />;
}
