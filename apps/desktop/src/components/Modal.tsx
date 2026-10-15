import { useEffect, useId, useRef, type ReactNode } from "react";

/**
 * Accessible modal built on the native <dialog> element: the browser moves
 * focus into it, keeps the rest of the window inert, and handles Escape
 * (routed to `onClose`). Focus returns to the previously focused element
 * when it closes.
 */
export function Modal({
  title,
  description,
  onClose,
  children,
  footer,
  width = "md",
}: {
  title: string;
  description?: ReactNode;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  width?: "sm" | "md" | "lg";
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const descriptionId = useId();
  const onCloseRef = useRef(onClose);
  useEffect(() => {
    onCloseRef.current = onClose;
  });

  useEffect(() => {
    const dialog = ref.current;
    const previous = document.activeElement as HTMLElement | null;
    if (dialog && !dialog.open) dialog.showModal();
    return () => {
      if (dialog?.open) dialog.close();
      previous?.focus?.();
    };
  }, []);

  const widths = { sm: "max-w-[420px]", md: "max-w-[600px]", lg: "max-w-[760px]" };

  return (
    <dialog
      ref={ref}
      aria-labelledby={titleId}
      aria-describedby={description ? descriptionId : undefined}
      onCancel={(event) => {
        event.preventDefault();
        onCloseRef.current();
      }}
      className={`m-auto w-[calc(100%-2rem)] ${widths[width]} rounded-lg border border-line bg-canvas p-0 text-ink shadow-[0_12px_40px_rgb(37_42_40/0.18)]`}
    >
      <div className="flex max-h-[85vh] flex-col">
        <div className="border-b border-line px-5 pt-4 pb-3">
          <h2 id={titleId} className="text-lg font-semibold">
            {title}
          </h2>
          {description && (
            <div id={descriptionId} className="mt-0.5 text-sm text-muted-text">
              {description}
            </div>
          )}
        </div>
        <div className="flex-1 overflow-y-auto px-5 py-4">{children}</div>
        {footer && (
          <div className="flex flex-wrap items-center justify-end gap-2 border-t border-line px-5 py-3">{footer}</div>
        )}
      </div>
    </dialog>
  );
}
