import { useState, type ReactNode } from "react";
import { compactCount, relativeTime } from "../phase3/logic";
import type { MediaItem } from "../phase3/types";
import { muted } from "./ui";

export interface CardMetrics {
  replies: number;
  reposts: number;
  likes: number;
  bookmarks: number;
  views: number | null;
}

export interface XPostCardProps {
  handle: string;
  name?: string;
  avatarUrl?: string | null;
  verified?: boolean;
  postedAt?: string | null;
  text: string;
  media?: MediaItem[];
  url?: string | null;
  metrics: CardMetrics;
  /** Small tags shown right of the header, e.g. a score. */
  badge?: ReactNode;
  /** Rendered inside the card, under the metrics row, separated by a rule. */
  children?: ReactNode;
  /** Clamp long text to a few lines. */
  compact?: boolean;
  selected?: boolean;
  onSelect?: () => void;
}

function Icon({ children }: { children: ReactNode }) {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {children}
    </svg>
  );
}

const ICONS = {
  replies: (
    <Icon>
      <path d="M4 5.5h16v10H12l-5 4v-4H4z" />
    </Icon>
  ),
  reposts: (
    <Icon>
      <path d="m17 3 3.5 3.5L17 10" />
      <path d="M4 11V9.5A3 3 0 0 1 7 6.5h13" />
      <path d="m7 21-3.5-3.5L7 14" />
      <path d="M20 13v1.5a3 3 0 0 1-3 3H4" />
    </Icon>
  ),
  likes: (
    <Icon>
      <path d="M12 20s-7.5-4.6-7.5-10A4.3 4.3 0 0 1 12 7.4 4.3 4.3 0 0 1 19.5 10c0 5.4-7.5 10-7.5 10Z" />
    </Icon>
  ),
  bookmarks: (
    <Icon>
      <path d="M6.5 4h11v16.5L12 16.5l-5.5 4Z" />
    </Icon>
  ),
  views: (
    <Icon>
      <path d="M5 20v-6" />
      <path d="M12 20V5" />
      <path d="M19 20v-9" />
    </Icon>
  ),
} as const;

const LABELS = { replies: "Replies", reposts: "Reposts", likes: "Likes", bookmarks: "Bookmarks", views: "Views" } as const;

export function Avatar({ src, name, handle, size = 40 }: { src?: string | null; name?: string; handle: string; size?: number }) {
  const [failed, setFailed] = useState(false);
  const initial = (name || handle || "?").trim().charAt(0).toUpperCase();
  const style = { width: size, height: size };
  if (src && !failed) {
    return <img src={src} alt="" width={size} height={size} style={style} loading="lazy" referrerPolicy="no-referrer" onError={() => setFailed(true)} className="shrink-0 rounded-full bg-zinc-200 object-cover dark:bg-zinc-800" />;
  }
  return (
    <span aria-hidden="true" style={style} className="flex shrink-0 items-center justify-center rounded-full bg-zinc-200 text-sm font-semibold text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300">
      {initial}
    </span>
  );
}

function isPlayableUrl(url: string | null): url is string {
  return Boolean(url && !url.startsWith("blob:"));
}

export function PostMedia({ media, postUrl }: { media: MediaItem[]; postUrl?: string | null }) {
  if (!media || media.length === 0) return null;
  const images = media.filter((m) => m.kind === "image" && m.url);
  const videos = media.filter((m) => m.kind === "video" || m.kind === "gif");
  const fallbackHref = postUrl ?? undefined;
  const frame = "overflow-hidden rounded-2xl border border-zinc-200 bg-zinc-100 dark:border-zinc-800 dark:bg-zinc-900";

  return (
    <div className="mt-2.5 space-y-2" onClick={(e) => e.stopPropagation()}>
      {images.length > 0 && (
        <div className={`${frame} grid gap-0.5 ${images.length === 1 ? "grid-cols-1" : "grid-cols-2"}`}>
          {images.slice(0, 4).map((img, idx) => (
            <a key={`${img.url}-${idx}`} href={fallbackHref ?? img.url ?? undefined} target="_blank" rel="noreferrer" className="block">
              <img src={img.url!} alt={img.alt ?? "Post image"} loading="lazy" referrerPolicy="no-referrer" className={`w-full object-cover ${images.length === 1 ? "max-h-80" : "h-40"}`} />
            </a>
          ))}
        </div>
      )}

      {videos.map((vid, idx) =>
        isPlayableUrl(vid.url) ? (
          <div key={`${vid.url}-${idx}`} className={`${frame} bg-zinc-950 dark:bg-zinc-950`}>
            <video
              src={vid.url}
              poster={vid.poster_url ?? undefined}
              controls={vid.kind === "video"}
              autoPlay={vid.kind === "gif"}
              loop={vid.kind === "gif"}
              muted={vid.kind === "gif"}
              playsInline
              preload="metadata"
              className="max-h-80 w-full object-contain"
            />
          </div>
        ) : (
          <a key={`${vid.poster_url ?? "video"}-${idx}`} href={fallbackHref} target="_blank" rel="noreferrer" className={`group relative block ${frame} bg-zinc-950 dark:bg-zinc-950`}>
            {vid.poster_url ? (
              <img src={vid.poster_url} alt={vid.alt ?? "Video preview"} loading="lazy" referrerPolicy="no-referrer" className="max-h-80 w-full object-cover opacity-90 transition-opacity group-hover:opacity-100" />
            ) : (
              <div className="flex h-40 items-center justify-center text-xs text-zinc-400">Video preview unavailable</div>
            )}
            <span aria-hidden="true" className="absolute inset-0 m-auto flex h-12 w-12 items-center justify-center rounded-full bg-zinc-950/70 text-white">
              <svg viewBox="0 0 24 24" width="22" height="22" fill="currentColor">
                <path d="M8 5.5v13l11-6.5z" />
              </svg>
            </span>
            {vid.duration && <span className="absolute right-2 bottom-2 rounded bg-zinc-950/80 px-1.5 py-0.5 font-mono text-[11px] text-zinc-100">{vid.duration}</span>}
          </a>
        ),
      )}
    </div>
  );
}

const VERIFIED_PATH = "M12 2.5 14.4 4l2.8-.2 1 2.6 2.4 1.5-.8 2.7.8 2.7-2.4 1.5-1 2.6-2.8-.2L12 21.5 9.6 20l-2.8.2-1-2.6-2.4-1.5.8-2.7-.8-2.7 2.4-1.5 1-2.6L9.6 4Zm-1.2 12.9 5-5-1.1-1.1-3.9 3.9-1.7-1.7-1.1 1.1Z";

/**
 * One post as a row of an X timeline: avatar on the left, then name, handle and time, text, media and the
 * engagement row. Children (a reply box) continue the same column directly below the post, joined to the
 * avatar by a thread line, so the reply reads as part of the post. Rows are flat; the parent draws the dividers.
 */
export function XPostCard({ handle, name, avatarUrl, verified, postedAt, text, media = [], url, metrics, badge, children, compact, selected, onSelect }: XPostCardProps) {
  const when = relativeTime(postedAt ?? null);
  const stats = (["replies", "reposts", "likes", "views", "bookmarks"] as const).filter((k) => metrics[k] != null);
  const display = name || `@${handle}`;

  return (
    <article aria-current={selected ? "true" : undefined} onClick={onSelect} className={`flex gap-3 px-4 pt-3 pb-3 transition-colors ${selected ? "bg-zinc-50 dark:bg-zinc-900/40" : ""}`}>
      <div className="flex shrink-0 flex-col items-center">
        <Avatar src={avatarUrl} name={name} handle={handle} />
        {children && <span aria-hidden="true" className="mt-1 w-0.5 flex-1 rounded-full bg-zinc-200 dark:bg-zinc-800" />}
      </div>

      <div className="min-w-0 flex-1">
        <div className="flex items-start justify-between gap-2">
          <div className="flex min-w-0 items-center gap-1 text-[15px] leading-5">
            <span className="truncate font-bold">{display}</span>
            {verified && (
              <svg viewBox="0 0 24 24" width="16" height="16" className="shrink-0 text-sky-500" fill="currentColor" aria-label="Verified">
                <path d={VERIFIED_PATH} />
              </svg>
            )}
            {name && <span className={`truncate ${muted}`}>@{handle}</span>}
            {when && (
              <>
                <span aria-hidden="true" className={muted}>
                  ·
                </span>
                {url ? (
                  <a href={url} target="_blank" rel="noreferrer" onClick={(e) => e.stopPropagation()} title="Show in the X window" className={`shrink-0 hover:underline ${muted}`}>
                    {when}
                  </a>
                ) : (
                  <span className={`shrink-0 ${muted}`}>{when}</span>
                )}
              </>
            )}
          </div>
          {badge && <div className="shrink-0">{badge}</div>}
        </div>

        <p className={`mt-0.5 text-[15px] leading-[1.35] whitespace-pre-wrap ${compact ? "line-clamp-5" : ""}`}>{text}</p>
        <PostMedia media={media} postUrl={url} />

        <ul className={`mt-3 -ml-2 flex max-w-lg items-center justify-between text-[13px] ${muted}`} aria-label="Engagement">
          {stats.map((k) => (
            <li key={k} className="flex min-w-12 items-center gap-1.5 px-2 py-0.5" title={LABELS[k]}>
              {ICONS[k]}
              <span className="tabular-nums">{compactCount(metrics[k] ?? 0)}</span>
              <span className="sr-only">{LABELS[k]}</span>
            </li>
          ))}
          {url && (
            <li className="ml-auto">
              <a
                href={url}
                target="_blank"
                rel="noreferrer"
                onClick={(e) => e.stopPropagation()}
                data-testid="view-on-x"
                title={`Show this post in the X window: ${url}`}
                className="flex items-center gap-1.5 rounded-full px-2 py-0.5 font-medium text-sky-600 hover:bg-sky-50 hover:underline dark:text-sky-400 dark:hover:bg-sky-950/40"
              >
                View on X
                <Icon>
                  <path d="M14 4h6v6" />
                  <path d="M20 4 10.5 13.5" />
                  <path d="M18 14v4.5A1.5 1.5 0 0 1 16.5 20h-11A1.5 1.5 0 0 1 4 18.5v-11A1.5 1.5 0 0 1 5.5 6H10" />
                </Icon>
              </a>
            </li>
          )}
        </ul>

        {children && (
          <div className="mt-3" onClick={(e) => e.stopPropagation()}>
            {children}
          </div>
        )}
      </div>
    </article>
  );
}
