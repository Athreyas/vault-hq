import { PLATFORM_LABEL, PLATFORM_SHORT, type Platform } from "../api/bookmarks";

/** `[yt]`, `[gh]`, `[pdf]`: where a link points, from its URL. */
export default function PlatformBadge({ platform }: { platform: Platform }) {
  return (
    <span className={`bm-badge is-${platform}`} title={PLATFORM_LABEL[platform]} aria-label={PLATFORM_LABEL[platform]}>
      {PLATFORM_SHORT[platform]}
    </span>
  );
}
