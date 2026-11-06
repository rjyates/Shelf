/// Headphones for server-streamed TTS (not device voices, not standalone audiobooks).
bool shelfServerTtsListenAvailable({
  required bool isEpub,
  required bool online,
  required bool hasEnabledProviders,
}) => isEpub && online && hasEnabledProviders;

/// Match a Readium locator href to a 0-based spine/chapter index for `/tts/text/{id}/{chapter}`.
int shelfTtsChapterIndex({
  required List<String> readingOrderHrefs,
  required String? locatorHref,
}) {
  if (readingOrderHrefs.isEmpty) return 0;
  if (locatorHref == null || locatorHref.isEmpty) return 0;
  final needle = _hrefPath(locatorHref);
  for (var i = 0; i < readingOrderHrefs.length; i++) {
    if (_hrefPath(readingOrderHrefs[i]) == needle) return i;
  }
  for (var i = 0; i < readingOrderHrefs.length; i++) {
    final spine = _hrefPath(readingOrderHrefs[i]);
    if (needle.endsWith(spine) || spine.endsWith(needle)) return i;
  }
  return 0;
}

String _hrefPath(String href) {
  final noFragment = href.split('#').first;
  final noQuery = noFragment.split('?').first;
  return noQuery.replaceFirst(RegExp(r'^/+'), '');
}
