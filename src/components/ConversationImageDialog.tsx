import { useEffect, useRef } from "react";
import { Icon } from "../icons";
import { Button } from "./ui/Button";

export function ConversationImageDialog({
  name,
  url,
  onClose,
}: {
  name: string;
  url: string;
  onClose: () => void;
}) {
  const titleId = `conversation-image-${encodeURIComponent(name).replaceAll("%", "")}`;
  const dialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const previousFocus =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = dialogRef.current;
    const focusable = () =>
      Array.from(
        dialog?.querySelectorAll<HTMLElement>(
          'button:not([disabled]), a[href], input:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ) ?? [],
      );
    focusable()[0]?.focus();

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose();
        return;
      }
      if (event.key !== "Tab") {
        return;
      }
      const controls = focusable();
      if (controls.length === 0) {
        event.preventDefault();
        dialog?.focus();
        return;
      }
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      previousFocus?.focus();
    };
  }, [onClose]);

  return (
    <div
      className="conversation-image-backdrop"
      onClick={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div
        ref={dialogRef}
        className="conversation-image-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        <header>
          <h3 id={titleId}>{name}</h3>
          <Button variant="icon" onClick={onClose} aria-label="关闭图片预览">
            <Icon name="close" size={15} />
          </Button>
        </header>
        <div className="conversation-image-stage">
          <img src={url} alt={name} />
        </div>
      </div>
    </div>
  );
}
