import { useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

interface DialogProps {
  title: string;
  subtitle?: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  wide?: boolean;
  alert?: boolean;
  descriptionId?: string;
}

export function Dialog({ title, subtitle, onClose, children, footer, wide, alert, descriptionId }: DialogProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const focusable = () =>
      [...dialog.querySelectorAll<HTMLElement>(
        'button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])',
      )].filter((element) => element.getClientRects().length > 0);
    const initialFocus = window.matchMedia("(pointer: fine)").matches && navigator.maxTouchPoints === 0
      ? dialog.querySelector<HTMLElement>("[data-dialog-autofocus]") ?? focusable()[0]
      : dialog.querySelector<HTMLElement>(".dialog-head .icon-button") ?? dialog;
    const focusFrame = window.requestAnimationFrame(() => initialFocus?.focus({ preventScroll: true }));

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (event.key === "Escape") {
        event.preventDefault();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const elements = focusable();
      if (elements.length === 0) {
        event.preventDefault();
        dialog.focus();
        return;
      }
      const first = elements[0];
      const last = elements[elements.length - 1];
      const active = document.activeElement;
      if (!dialog.contains(active)) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
      } else if (event.shiftKey && active === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", onKeyDown);
    return () => {
      window.cancelAnimationFrame(focusFrame);
      document.removeEventListener("keydown", onKeyDown);
      if (previousFocus?.isConnected && previousFocus !== document.body) {
        previousFocus.focus({ preventScroll: true });
      } else {
        document.querySelector<HTMLElement>("[data-dialog-fallback-focus]")?.focus({ preventScroll: true });
      }
    };
  }, []);

  return createPortal(
    <div
      className="overlay"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={dialogRef}
        className={wide ? "dialog dialog--wide" : "dialog"}
        role={alert ? "alertdialog" : "dialog"}
        aria-modal="true"
        aria-labelledby="dialog-title"
        aria-describedby={descriptionId}
        tabIndex={-1}
      >
        <header className="dialog-head">
          <div>
            <h2 id="dialog-title">{title}</h2>
            {subtitle ? <p className="dialog-sub">{subtitle}</p> : null}
          </div>
          <button type="button" className="icon-button" onClick={onClose} aria-label="Закрыть">
            ×
          </button>
        </header>
        <div className="dialog-body">{children}</div>
        {footer ? <footer className="dialog-actions">{footer}</footer> : null}
      </div>
    </div>,
    document.body,
  );
}

interface CopyButtonProps {
  text: string;
  label?: string;
  className?: string;
}

export function CopyButton({ text, label = "Копировать", className }: CopyButtonProps) {
  const [state, setState] = useState<"idle" | "done" | "failed">("idle");

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setState("done");
    } catch {
      setState("failed");
    }
    window.setTimeout(() => setState("idle"), 1600);
  };

  return (
    <button type="button" className={className ?? "copy"} onClick={copy}>
      {state === "done" ? "Скопировано" : state === "failed" ? "Выделите вручную" : label}
    </button>
  );
}

export function StatusDot({ health }: { health: "ok" | "warn" | "fail" | "idle" | "offline" }) {
  return <i className={`dot dot--${health}`} aria-hidden="true" />;
}
