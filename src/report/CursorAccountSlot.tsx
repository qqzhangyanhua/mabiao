import type { PosterCursorAccount } from "./posterTypes";

export function CursorAccountSlot({
  data,
  titleClass,
  tokensClass,
  noteClass,
}: {
  data: PosterCursorAccount;
  titleClass: string;
  tokensClass: string;
  noteClass: string;
}) {
  const amount = data.costLabel ? `${data.tokensLabel} token · ${data.costLabel}` : `${data.tokensLabel} token`;
  return (
    <>
      <h2 className={titleClass}>Cursor 账号用量</h2>
      <p className={tokensClass}>{amount}</p>
      {data.modelsLabel ? <p className={noteClass}>{data.modelsLabel}</p> : null}
      <p className={noteClass}>{data.note}</p>
    </>
  );
}
