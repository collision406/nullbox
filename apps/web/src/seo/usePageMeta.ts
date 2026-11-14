import { useEffect } from "react";
import { useLocale } from "../copy/CopyProvider";

export const PRODUCTION_ORIGIN = "https://team-09.fasthacks.cloud";

export interface PageMetaOptions {
  title: string;
  description: string;
  /** Canonical path starting with `/`, e.g. `"/"` or `"/inbox"`. */
  canonicalPath: string;
  /** Defaults to `"index, follow"`. Set `"noindex, nofollow"` on private/worker/404 routes. */
  robots?: "index, follow" | "noindex, nofollow";
  /**
   * Defaults to `"strict-origin-when-cross-origin"`. Set `"no-referrer"` on
   * secret customer ticket pages and worker pages so URLs/tokens never leak.
   */
  referrer?: "strict-origin-when-cross-origin" | "no-referrer";
}

function resolveOrigin(): string {
  if (typeof window !== "undefined" && window.location?.origin) {
    const origin = window.location.origin;
    if (origin.startsWith("http://") || origin.startsWith("https://")) {
      return origin;
    }
  }
  return PRODUCTION_ORIGIN;
}

function upsertMeta(attr: "name" | "property", key: string, content: string): void {
  let tag = document.head.querySelector<HTMLMetaElement>(`meta[${attr}="${key}"]`);
  if (!tag) {
    tag = document.createElement("meta");
    tag.setAttribute(attr, key);
    document.head.appendChild(tag);
  }
  tag.setAttribute("content", content);
}

function upsertCanonical(href: string): void {
  let link = document.head.querySelector<HTMLLinkElement>('link[rel="canonical"]');
  if (!link) {
    link = document.createElement("link");
    link.setAttribute("rel", "canonical");
    document.head.appendChild(link);
  }
  link.setAttribute("href", href);
}

/**
 * Synchronizes `<title>`, `<meta name="description">`, `<meta name="robots">`,
 * `<meta name="referrer">`, `<link rel="canonical">`, Open Graph, and Twitter
 * Card tags for the active route and locale.
 */
export function usePageMeta({
  title,
  description,
  canonicalPath,
  robots = "index, follow",
  referrer = "strict-origin-when-cross-origin",
}: PageMetaOptions): void {
  const [locale] = useLocale();

  useEffect(() => {
    const origin = resolveOrigin();
    const normalizedPath = canonicalPath.startsWith("/") ? canonicalPath : `/${canonicalPath}`;
    const canonicalUrl = `${origin}${normalizedPath}`;
    const ogLocale = locale === "ja" ? "ja_JP" : "en_US";

    document.title = title;
    upsertMeta("name", "description", description);
    upsertMeta("name", "robots", robots);
    upsertMeta("name", "referrer", referrer);
    upsertCanonical(canonicalUrl);

    upsertMeta("property", "og:title", title);
    upsertMeta("property", "og:description", description);
    upsertMeta("property", "og:url", canonicalUrl);
    upsertMeta("property", "og:locale", ogLocale);

    upsertMeta("name", "twitter:title", title);
    upsertMeta("name", "twitter:description", description);
  }, [title, description, canonicalPath, robots, referrer, locale]);
}
