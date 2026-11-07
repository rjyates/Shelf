import 'package:flutter_readium/flutter_readium.dart';

import 'media_overlay_playlist.dart';

/// E-ink: repaint MO highlight only on chapter change or every [minSeconds].
bool shouldRefreshMediaOverlayHighlight({
  required String? paintedTextHref,
  required String? currentTextHref,
  required double secondsSincePaint,
  double minSeconds = 2.0,
}) {
  if (paintedTextHref == null) return true;
  final page = (currentTextHref ?? '').replaceAll('\\', '/');
  final painted = paintedTextHref.replaceAll('\\', '/');
  if (page != painted) return true;
  return secondsSincePaint >= minSeconds;
}

double mediaOverlayItemDuration(MediaOverlayItem item) {
  final end = item.clipEndSeconds;
  if (end != null && end > item.clipBeginSeconds) {
    return end - item.clipBeginSeconds;
  }
  return 0.25;
}

/// Build a Readium locator for the SMIL text target (sentence/word fragment).
Locator? locatorForMediaOverlayItem({
  required MediaOverlayItem item,
  required Publication publication,
}) {
  final textHref = item.textHref;
  if (textHref == null || textHref.isEmpty) return null;
  final wanted = _hrefPath(textHref);
  Link? link;
  for (final candidate in publication.readingOrder) {
    final path = _hrefPath(candidate.href);
    if (path == wanted || path.endsWith(wanted) || wanted.endsWith(path)) {
      link = candidate;
      break;
    }
  }
  final href = link != null ? _hrefPath(link.href) : wanted;
  if (href.isEmpty) return null;
  final frag = item.textFragment?.trim();
  final hasFrag = frag != null && frag.isNotEmpty;
  return Locator(
    href: href.startsWith('/') ? href : '/$href',
    type: link?.type ?? 'application/xhtml+xml',
    title: link?.title,
    locations: Locations(
      fragments: hasFrag ? [frag] : const [],
      cssSelector: hasFrag ? '#$frag' : null,
    ),
  );
}

String _hrefPath(String href) {
  final noFragment = href.split('#').first;
  final noQuery = noFragment.split('?').first;
  return noQuery.replaceFirst(RegExp(r'^/+'), '');
}
