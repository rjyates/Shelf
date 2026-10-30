export function sanitizeChapter(purifier, html) {
  return purifier.sanitize(html, {
    WHOLE_DOCUMENT: true,
    ADD_TAGS: ["link"],
    // epub.js rewrites local images and stylesheets to blob URLs before this hook.
    ALLOWED_URI_REGEXP:
      /^(?:(?:https?|blob):|[^a-z]|[a-z+.\-]+(?:[^a-z+.\-:]|$))/i,
    FORBID_TAGS: [
      "script",
      "iframe",
      "object",
      "embed",
      "form",
      "base",
      "meta",
    ],
    FORBID_ATTR: ["srcset"],
  });
}
