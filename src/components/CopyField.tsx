"use client";

import { useState } from "react";

export function CopyField({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);

  return (
    <div className="flex gap-2">
      <input
        readOnly
        value={value}
        onFocus={(e) => e.currentTarget.select()}
        className="min-w-0 flex-1 rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 font-mono text-xs outline-none"
      />
      <button
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(value);
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          } catch {
            // Clipboard needs a secure context; the field is selectable anyway.
          }
        }}
        className="shrink-0 rounded-md border border-neutral-700 px-3 py-2 text-sm transition hover:border-neutral-500"
      >
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
  );
}
