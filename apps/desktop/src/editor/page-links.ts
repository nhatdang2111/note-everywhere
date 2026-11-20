/**
 * Inline links to another page are ordinary `link` marks whose `href` is
 * `ne-page:<pageId>`. They need no schema change and the core can read them
 * the same way it reads any link mark.
 */
export const PAGE_HREF_PREFIX = "ne-page:";

export const pageHref = (pageId: string): string => `${PAGE_HREF_PREFIX}${pageId}`;

export function parsePageHref(href: string): string | null {
  return href.startsWith(PAGE_HREF_PREFIX) && href.length > PAGE_HREF_PREFIX.length ? href.slice(PAGE_HREF_PREFIX.length) : null;
}

/** Image blocks point at stored attachments with `attachment:<hash>`, resolved to a URL at display time. */
export const ATTACHMENT_SRC_PREFIX = "attachment:";

export const attachmentSrc = (hash: string): string => `${ATTACHMENT_SRC_PREFIX}${hash}`;

export function parseAttachmentSrc(src: string): string | null {
  return src.startsWith(ATTACHMENT_SRC_PREFIX) ? src.slice(ATTACHMENT_SRC_PREFIX.length) : null;
}

/**
 * A copied reference to a synced block travels as `ne-synced:<syncId>` text (and
 * under `SYNCED_MIME`), so pasting it into another page places the same block.
 */
export const SYNCED_REFERENCE_PREFIX = "ne-synced:";
export const SYNCED_MIME = "application/x-ne-synced";

export const syncedReference = (syncId: string): string => `${SYNCED_REFERENCE_PREFIX}${syncId}`;

export function parseSyncedReference(text: string): string | null {
  const trimmed = text.trim();
  return trimmed.startsWith(SYNCED_REFERENCE_PREFIX) && trimmed.length > SYNCED_REFERENCE_PREFIX.length && !/\s/.test(trimmed)
    ? trimmed.slice(SYNCED_REFERENCE_PREFIX.length)
    : null;
}
