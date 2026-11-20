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
