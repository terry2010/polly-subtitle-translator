import { useRef, useEffect, useCallback } from "react";
import { Textarea } from "./ui/textarea";

interface AutoTextareaProps {
  value: string;
  onChange: (value: string) => void;
  className?: string;
  placeholder?: string;
  onClick?: (e: React.MouseEvent) => void;
  onKeyDown?: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  onContextMenu?: (e: React.MouseEvent) => void;
  autoFocus?: boolean;
}

export function AutoTextarea({ value, onChange, className, placeholder, onClick, onKeyDown, onContextMenu, autoFocus }: AutoTextareaProps) {
  const ref = useRef<HTMLTextAreaElement>(null);

  const adjustHeight = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, []);

  useEffect(() => {
    adjustHeight();
  }, [value, adjustHeight]);

  useEffect(() => {
    if (autoFocus && ref.current) {
      ref.current.focus();
      adjustHeight();
    }
  }, [autoFocus, adjustHeight]);

  return (
    <Textarea
      ref={ref}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className={className}
      placeholder={placeholder}
      onClick={onClick}
      onKeyDown={onKeyDown}
      onContextMenu={onContextMenu}
      rows={1}
      style={{ minHeight: "28px", overflow: "hidden" }}
    />
  );
}
