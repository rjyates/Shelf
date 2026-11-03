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
  // Foliate/web CFIs are not Readium Locators. Returning a fake href:'#'
  // locator makes goToLocator "succeed" and skips percentage restore.
  return null;
}

/// Whether [locator] has enough structure for Readium navigation (not a stub).
bool isNavigableReadiumLocator(Locator locator) {
  final href = locator.href.trim();
  return href.isNotEmpty && href != '#';
}

/// Map Shelf's publication-wide percentage onto a reading-order [Locator].
///
/// Readium's `goToProgression` on Android scrolls *within the current
/// resource* (`scrollToPosition`), so a bare 0.58 leaves you on the title
/// page. Approximate total progress by spine index instead.
Locator locatorForTotalProgression(Publication publication, double total) {
  final order = publication.readingOrder;
  final t = total.clamp(0.0, 1.0);
  if (order.isEmpty) {
    return Locator(
      href: '/',
      type: 'application/xhtml+xml',
      locations: Locations(totalProgression: t),
    );
  }
  final scaled = t * order.length;
  final index = scaled.floor().clamp(0, order.length - 1);
  final within = (scaled - index).clamp(0.0, 1.0);
  final link = order[index];
  return Locator(
    href: link.href,
    type: link.type ?? 'application/xhtml+xml',
    title: link.title,
    locations: Locations(
      position: index + 1,
      progression: within,
      totalProgression: t,
    ),
  );
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

/// Prefer a prior Readium locator; otherwise seed from Shelf %. Foliate CFIs
/// are not portable — percentage is the cross-engine contract.
Locator? restoreReadiumLocator({
  required Publication publication,
  required String? position,
  required double percentage,
}) {
  final stored = locatorFromShelfPosition(position);
  final storedPct = stored == null ? null : shelfPercentageFromLocator(stored);
  final storedUsable =
      stored != null &&
      isNavigableReadiumLocator(stored) &&
      !(percentage > 0.01 && (storedPct == null || storedPct < 0.01));
  if (storedUsable) {
    return publication.resolveLocator(stored) ?? stored;
  }
  if (percentage <= 0) return null;
  final seed = locatorForTotalProgression(publication, percentage);
  return publication.resolveLocator(seed) ?? seed;
}

/// Ignore title-page locators while restoring a meaningful Shelf percentage.
bool shouldDeferPersistDuringRestore({
  required DateTime? openedAt,
  required DateTime now,
  required double bookPercentage,
  required double locatorPercentage,
  Duration window = const Duration(seconds: 8),
  double slack = 0.08,
}) {
  if (openedAt == null) return false;
  if (now.difference(openedAt) >= window) return false;
  if (bookPercentage <= 0.05) return false;
  return locatorPercentage + slack < bookPercentage;
}
