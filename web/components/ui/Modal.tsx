"use client";

import { type ReactNode } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/shadcn/dialog";
import { cn } from "@/lib/cn";

type ModalProps = {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  children: ReactNode;
  footer?: ReactNode;
  width?: "sm" | "md" | "lg";
};

const WIDTH = {
  sm: "sm:max-w-sm",
  md: "sm:max-w-lg",
  lg: "sm:max-w-2xl",
};

export function Modal({ open, onClose, title, description, children, footer, width = "md" }: ModalProps) {
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <DialogContent
        className={cn(
          "max-h-[calc(100vh-2rem)] flex flex-col overflow-hidden p-6 animate-pop-in",
          WIDTH[width]
        )}
      >
        <DialogHeader>
          <DialogTitle className="font-display text-2xl font-extrabold">{title}</DialogTitle>
          {description && <DialogDescription>{description}</DialogDescription>}
        </DialogHeader>
        <div className="mt-5 min-h-0 flex-1 overflow-y-auto pr-1">{children}</div>
        {footer && <div className="mt-6 flex flex-wrap justify-end gap-3">{footer}</div>}
      </DialogContent>
    </Dialog>
  );
}

export function ModalActions({ children }: { children: ReactNode }) {
  return <div className="flex flex-wrap justify-end gap-3">{children}</div>;
}
