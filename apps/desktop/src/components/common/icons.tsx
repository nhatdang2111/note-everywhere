import type { JSX } from "solid-js";

/**
 * The icon set, drawn for this project: 16x16 grid, 1.5 px round strokes in
 * `currentColor`. Each entry is the `d` attribute of one path.
 */
const PATHS = {
  "chevron-right": "M6 3.5l4.5 4.5L6 12.5",
  "chevron-down": "M3.5 6L8 10.5 12.5 6",
  plus: "M8 3v10M3 8h10",
  search: "M7 2.75a4.25 4.25 0 100 8.5 4.25 4.25 0 000-8.5zM10.2 10.2l3.3 3.3",
  folder: "M2 4.5a1 1 0 011-1h3l1.5 1.5H13a1 1 0 011 1v6a1 1 0 01-1 1H3a1 1 0 01-1-1z",
  page: "M4 1.75h5l3 3v9.5H4zM9 1.75v3h3",
  "page-link": "M4 1.75h5l3 3v9.5H4zM6.5 10.5l3-3M7.5 7.5h2v2",
  star: "M8 2l1.8 3.7 4 .6-2.9 2.8.7 4L8 11.2 4.4 13.1l.7-4L2.2 6.3l4-.6z",
  trash: "M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.5 8.5h6l.5-8.5M7 7v4M9 7v4",
  more: "M3.5 8h.01M8 8h.01M12.5 8h.01",
  edit: "M10.5 2.5l3 3L6 13H3v-3z",
  check: "M3 8.5l3.2 3.2L13 4.8",
  close: "M4 4l8 8M12 4l-8 8",
  copy: "M5.5 5.5h7v8h-7zM3.5 10.5v-8h7",
  eye: "M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8zM8 10a2 2 0 100-4 2 2 0 000 4z",
  "eye-off": "M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8zM8 10a2 2 0 100-4 2 2 0 000 4zM3 13L13 3",
  lock: "M4.5 7.5h7v6h-7zM6 7.5v-2a2 2 0 014 0v2",
  unlock: "M4.5 7.5h7v6h-7zM6 7.5v-2a2 2 0 013.8-.9",
  key: "M5.5 10.5a3 3 0 110-6 3 3 0 010 6zM8 8l5.5 5.5M11 11l1.5-1.5",
  settings: "M2.5 4.5h6M11.5 4.5h2M2.5 11.5h2M7.5 11.5h6M10 3v3M6 10v3",
  sync: "M13 6.5A5 5 0 004 4.5M3 9.5a5 5 0 009 2M3.5 2v3h3M12.5 14v-3h-3",
  "cloud-off": "M4.5 12.5a3 3 0 01-.4-6A4 4 0 0111.6 6a2.8 2.8 0 01.9 5.5H6.5M2.5 2.5l11 11",
  alert: "M8 2.5l6 10.5H2zM8 7v3M8 11.5h.01",
  info: "M8 2a6 6 0 100 12A6 6 0 008 2zM8 7.2V11M8 5h.01",
  mic: "M6.5 2.5h3v5a1.5 1.5 0 01-3 0zM4.5 7a3.5 3.5 0 007 0M8 10.5v3M6 13.5h4",
  image: "M2.5 3.5h11v9h-11zM2.5 11l3.5-3.5 3 3 2-2 2.5 2.5M10.5 6h.01",
  code: "M5.5 4.5L2 8l3.5 3.5M10.5 4.5L14 8l-3.5 3.5",
  quote: "M3 3.5v9M6 5.5h7M6 8h7M6 10.5h5",
  "bullet-list": "M5.5 4h8M5.5 8h8M5.5 12h8M2.5 4h.01M2.5 8h.01M2.5 12h.01",
  "number-list": "M6 4h7.5M6 8h7.5M6 12h7.5M2.5 3l1-.5v3M2.3 9.5c.3-.6 1.4-.6 1.4.1 0 .8-1.4 1.1-1.4 1.9h1.5",
  checkbox: "M2.5 2.5h11v11h-11zM5.2 8.2l2 2 3.6-4",
  toggle: "M5.5 3.5L11 8l-5.5 4.5z",
  divider: "M2 8h12M2 4.5h3M11 4.5h3M2 11.5h3M11 11.5h3",
  heading: "M3.5 3.5v9M12.5 3.5v9M3.5 8h9",
  paragraph: "M2.5 4h11M2.5 8h11M2.5 12h6",
  callout: "M2.5 3.5h11v8H8l-3 2.5v-2.5H2.5zM8 5.5v2.2M8 9h.01",
  link: "M6.8 9.2a2.5 2.5 0 003.5 0l2.2-2.2a2.5 2.5 0 00-3.5-3.5l-.7.7M9.2 6.8a2.5 2.5 0 00-3.5 0L3.5 9a2.5 2.5 0 003.5 3.5l.7-.7",
  bold: "M5 3.5h3.5a2.25 2.25 0 010 4.5H5zM5 8h4a2.25 2.25 0 010 4.5H5z",
  italic: "M6.5 3.5h5M4.5 12.5h5M9.5 3.5l-3 9",
  strike: "M2.5 8h11M10.5 5c0-1-1-1.8-2.5-1.8S5.3 4 5.3 5.2M5.5 11c0 .9 1 1.8 2.6 1.8s2.6-.8 2.6-1.8",
  "arrow-left": "M13 8H3M7 4L3 8l4 4",
  "arrow-right": "M3 8h10M9 4l4 4-4 4",
  "arrow-up": "M8 13V3M4 7l4-4 4 4",
  "arrow-down": "M8 3v10M4 9l4 4 4-4",
  sidebar: "M2.5 3h11v10h-11zM6 3v10",
  sun: "M8 5.5a2.5 2.5 0 100 5 2.5 2.5 0 000-5zM8 1.5V3M8 13v1.5M1.5 8H3M13 8h1.5M3.4 3.4l1 1M11.6 11.6l1 1M12.6 3.4l-1 1M4.4 11.6l-1 1",
  moon: "M12.5 9.5A5 5 0 016.5 3.5a5 5 0 106 6z",
  monitor: "M2 3h12v8H2zM6 13.5h4M8 11v2.5",
  download: "M8 2.5v8M4.5 7.5L8 11l3.5-3.5M3 13.5h10",
  upload: "M8 11V3M4.5 6L8 2.5 11.5 6M3 13.5h10",
  dice: "M3 3h10v10H3zM5.8 5.8h.01M10.2 5.8h.01M8 8h.01M5.8 10.2h.01M10.2 10.2h.01",
  grip: "M6 4h.01M10 4h.01M6 8h.01M10 8h.01M6 12h.01M10 12h.01",
} as const;

export type IconName = keyof typeof PATHS;

export interface IconProps {
  name: IconName;
  size?: number;
  /** Fill the shape, for a "on" state such as a pinned star. */
  filled?: boolean;
  /** Set for an icon that carries meaning on its own; otherwise it is hidden from assistive tech. */
  label?: string;
  class?: string;
}

export function Icon(props: IconProps): JSX.Element {
  return (
    <svg
      class={`icon ${props.class ?? ""}`}
      viewBox="0 0 16 16"
      width={props.size ?? 16}
      height={props.size ?? 16}
      fill={props.filled ? "currentColor" : "none"}
      stroke="currentColor"
      stroke-width="1.5"
      stroke-linecap="round"
      stroke-linejoin="round"
      role={props.label ? "img" : undefined}
      aria-label={props.label}
      aria-hidden={props.label ? undefined : "true"}
    >
      <path d={PATHS[props.name]} />
    </svg>
  );
}
