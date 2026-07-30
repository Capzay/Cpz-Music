const base = { viewBox: "0 0 24 24", fill: "currentColor", "aria-hidden": true } as const;

export const PlayIcon = (p: { className?: string }) => (
  <svg {...base} {...p}>
    <path d="M8 5.14v13.72a1 1 0 0 0 1.54.84l10.5-6.86a1 1 0 0 0 0-1.68L9.54 4.3A1 1 0 0 0 8 5.14Z" />
  </svg>
);

export const PauseIcon = (p: { className?: string }) => (
  <svg {...base} {...p}>
    <path d="M7 4h3.5v16H7zM13.5 4H17v16h-3.5z" />
  </svg>
);

export const NextIcon = (p: { className?: string }) => (
  <svg {...base} {...p}>
    <path d="M6 5.14v13.72a1 1 0 0 0 1.54.84l9-6.86a1 1 0 0 0 0-1.68l-9-6.86A1 1 0 0 0 6 5.14ZM17.5 4H20v16h-2.5z" />
  </svg>
);

export const PrevIcon = (p: { className?: string }) => (
  <svg {...base} {...p}>
    <path d="M18 5.14v13.72a1 1 0 0 1-1.54.84l-9-6.86a1 1 0 0 1 0-1.68l9-6.86A1 1 0 0 1 18 5.14ZM4 4h2.5v16H4z" />
  </svg>
);

export const ShuffleIcon = (p: { className?: string }) => (
  <svg {...base} {...p}>
    <path d="M17 3l4 4-4 4V8h-2.2l-2.3 3-1.3-1.6L13.7 6H17V3ZM3 6h4.3l7 9H17v-3l4 4-4 4v-3h-3.7l-7-9H3V6Zm0 12h4.3l2.3-3 1.3 1.6L8.3 20H3v-2Z" />
  </svg>
);

export const RepeatIcon = (p: { className?: string }) => (
  <svg {...base} {...p}>
    <path d="M7 7h10v3l4-4-4-4v3H5v6h2V7Zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4Z" />
  </svg>
);

export const RepeatOneIcon = (p: { className?: string }) => (
  <svg {...base} {...p}>
    <path d="M7 7h10v3l4-4-4-4v3H5v6h2V7Zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4Z" />
    <text x="12" y="15" textAnchor="middle" fontSize="9" fontWeight="700" fill="currentColor">
      1
    </text>
  </svg>
);

export const QueueIcon = (p: { className?: string }) => (
  <svg {...base} {...p}>
    <path d="M3 6h13v2H3V6Zm0 5h13v2H3v-2Zm0 5h9v2H3v-2Zm15-6.5v6.28A2.75 2.75 0 1 0 20 18V11h2V9.5h-4Z" />
  </svg>
);

export const VolumeIcon = (p: { className?: string; muted?: boolean }) => (
  <svg {...base} className={p.className}>
    <path d="M4 9v6h4l5 4V5L8 9H4Z" />
    {p.muted ? (
      <path d="M16.5 8.5l5 5m0-5l-5 5" stroke="currentColor" strokeWidth="1.8" fill="none" />
    ) : (
      <path d="M16 8.8a4.5 4.5 0 0 1 0 6.4l1.1 1.1a6 6 0 0 0 0-8.6L16 8.8Z" />
    )}
  </svg>
);

export const CloseIcon = (p: { className?: string }) => (
  <svg {...base} {...p}>
    <path d="M6.4 5 5 6.4 10.6 12 5 17.6 6.4 19 12 13.4 17.6 19 19 17.6 13.4 12 19 6.4 17.6 5 12 10.6 6.4 5Z" />
  </svg>
);
