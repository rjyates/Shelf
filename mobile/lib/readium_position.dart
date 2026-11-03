import 'dart:convert';

import 'package:flutter_readium/flutter_readium.dart';

/// Encode a Readium [Locator] into Shelf's `epubcfi(...)` position column.
String shelfPositionFromLocator(Locator locator) {
  final fragments = locator.locations?.fragments ?? const <String>[];
  for (final fragment in fragments) {
    if (fragment.startsWith('epubcfi(') && fragment.endsWith(')')) {
      return fragment;
    }
  }
  final encoded = base64UrlEncode(utf8.encode(jsonEncode(locator.toJson())));
  return 'epubcfi(/shelf-readium!$encoded)';
}

/// Restore a Readium [Locator] from a Shelf position string, if possible.
Locator? locatorFromShelfPosition(String? position) {
  if (position == null || position.isEmpty) return null;
  final packed = RegExp(r'^epubcfi\(/shelf-readium!([A-Za-z0-9_\-=]+)\)$')
      .firstMatch(position);
  if (packed != null) {
    try {
      final json = jsonDecode(
        utf8.decode(base64Url.decode(packed.group(1)!)),
      ) as Map<String, dynamic>;
      return Locator.fromJson(json);
    } catch (_) {
      return null;
    }
  }
  if (position.startsWith('epubcfi(') && position.endsWith(')')) {
    return Locator(
      href: '#',
      type: 'application/xhtml+xml',
      locations: Locations(fragments: [position]),
    );
  }
  return null;
}

double shelfPercentageFromLocator(Locator locator) {
  final total = locator.locations?.totalProgression;
  if (total != null && total.isFinite) return total.clamp(0.0, 1.0);
  final progression = locator.locations?.progression;
  if (progression != null && progression.isFinite) {
    return progression.clamp(0.0, 1.0);
  }
  return 0;
}
