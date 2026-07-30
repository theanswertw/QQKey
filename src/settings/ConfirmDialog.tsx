import { useEffect, useRef } from "react";

interface Props {
  title: string;
  message: string;
  confirmLabel: string;
  /** 確認鍵是否為破壞性動作。是的話染紅，提醒這一下按下去收不回來。 */
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * 確認對話框。
 *
 * 刪除是硬刪除、沒有 undo，而四顆同尺寸的 ghost 按鈕並排在一起，
 * 誤點的代價太高。
 */
export default function ConfirmDialog({
  title,
  message,
  confirmLabel,
  danger,
  onConfirm,
  onCancel,
}: Props) {
  const cancelRef = useRef<HTMLButtonElement>(null);

  // 初始焦點放在「取消」——按 Enter 的直覺反應不該是執行破壞性動作
  useEffect(() => {
    cancelRef.current?.focus();
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onCancel();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onCancel]);

  return (
    <div className="overlay" onClick={onCancel}>
      <div
        className="dialog dialog--confirm"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
        onClick={(event) => event.stopPropagation()}
      >
        <h2 className="dialog__title" id="confirm-title">
          {title}
        </h2>
        <p className="dialog__message">{message}</p>
        <div className="dialog__actions">
          <button ref={cancelRef} type="button" className="button" onClick={onCancel}>
            取消
          </button>
          <button
            type="button"
            className={
              danger ? "button button--primary button--danger" : "button button--primary"
            }
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
