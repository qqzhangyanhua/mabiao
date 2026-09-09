import { useState, type ReactNode } from "react";
import { Icon } from "../icons";
import { readSectionOpen, writeSectionOpen } from "../lib/sectionCollapse";
import { Button } from "./ui/Button";

export function CollapsibleSection({
  sectionId,
  title,
  collapsedSummary,
  defaultOpen = true,
  forceOpen = false,
  extra,
  className,
  children,
}: {
  sectionId: string;
  title: string;
  collapsedSummary: string;
  defaultOpen?: boolean;
  forceOpen?: boolean;
  extra?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(() => forceOpen || readSectionOpen(sectionId, defaultOpen));
  if (forceOpen && !open) {
    setOpen(true);
    writeSectionOpen(sectionId, true);
  }
  const bodyId = `overview-section-${sectionId}`;

  function toggle() {
    setOpen((prev) => {
      const next = !prev;
      writeSectionOpen(sectionId, next);
      return next;
    });
  }

  return (
    <section
      className={["collapsible-section", open ? "is-open" : "is-collapsed", className]
        .filter(Boolean)
        .join(" ")}
    >
      <div className="panel-head collapsible-head">
        <h2 title={title}>{title}</h2>
        <div className="collapsible-actions">
          {open ? extra : <span className="muted collapsible-summary">{collapsedSummary}</span>}
          <Button
            variant="icon"
            className="collapsible-toggle"
            aria-expanded={open}
            aria-controls={bodyId}
            aria-label={open ? "收起" : "展开"}
            title={open ? "收起" : "展开"}
            onClick={toggle}
          >
            <Icon name="chevron" size={13} className="caret" />
          </Button>
        </div>
      </div>
      {open ? (
        <div className="collapsible-body" id={bodyId}>
          {children}
        </div>
      ) : null}
    </section>
  );
}
