import DOMPurify from "dompurify";

/**
 * Cleans HTML before it is drawn with `dangerouslySetInnerHTML`. Project pages and changelogs come
 * from Modrinth, CurseForge or GitHub, and in a Monolink window from a friend's PC, so none of it
 * is trusted: no scripts, event handlers, frames, forms or `javascript:` links get through.
 */
export function safeHtml(html: string | null | undefined): string {
  if (!html) return "";
  return DOMPurify.sanitize(html, {
    USE_PROFILES: { html: true },
    FORBID_TAGS: ["style", "iframe", "frame", "object", "embed", "form", "input", "button", "textarea", "select", "link", "meta", "base"],
    FORBID_ATTR: ["style", "srcset", "formaction"],
    ALLOWED_URI_REGEXP: /^(?:https?:|mailto:|#|\/(?!\/)|data:image\/(?:png|gif|jpe?g|webp);)/i,
  });
}
