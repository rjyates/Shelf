import 'dart:convert';

import 'package:flutter_readium/flutter_readium.dart';

/// Page-based formats that share Shelf `*page(N)` tokens with the web readers.
enum ShelfPagedFormat { pdf, cbz }

String shelfPageToken(ShelfPagedFormat format) =>
    format == ShelfPagedFormat.pdf ? 'pdfpage' : 'cbzpage';

/// 1-based page from a Readium locator (`locations.position`, `page=`, or spine index).
int? pageFromLocator(Locator locator, {Publication? publication}) {
  final position = locator.locations?.position;
  if (position != null && position >= 1) return position;
  final fromFragment = locator.locations?.page;
  if (fromFragment != null && fromFragment >= 1) return fromFragment;
  if (publication != null) {
    final order = publication.readingOrder;
    final index = order.indexWhere((link) => link.href == locator.href);
    if (index >= 0) return index + 1;
  }
  return null;
}

/// Encode a Readium [Locator] into Shelf's position column.
///
/// PDFs use `pdfpage(N)`; CBZ uses `cbzpage(N)` for the web comic reader.
String shelfPositionFromLocator(
  Locator locator, {
  ShelfPagedFormat? paged,
  Publication? publication,
}) {
  if (paged != null) {
    final page = pageFromLocator(locator, publication: publication);
    if (page != null) return '${shelfPageToken(paged)}($page)';
  }
  final fragments = locator.locations?.fragments ?? const <String>[];
  for (final fragment in fragments) {
    if (fragment.startsWith('epubcfi(') && fragment.endsWith(')')) {
      return fragment;
    }
  }
  final encoded = base64UrlEncode(utf8.encode(jsonEncode(locator.toJson())));
  return 'epubcfi(/shelf-readium!$encoded)';
}

/// 1-based page from a Shelf `pdfpage(N)` / `cbzpage(N)` position, if any.
int? pageFromShelfPosition(String? position, ShelfPagedFormat format) {
  if (position == null || position.isEmpty) return null;
  final match = RegExp('^${shelfPageToken(format)}\\((\\d+)\\)\$')
      .firstMatch(position);
  if (match == null) return null;
  final page = int.tryParse(match.group(1)!);
  return page != null && page >= 1 ? page : null;
}

/// Locator aimed at a 1-based PDF page.
///
/// A PDF is usually one reading-order resource; [Locations.position] is the
/// page number inside that file (not an index into [Publication.readingOrder]).
Locator locatorForPdfPage(Publication publication, int page) {
  final order = publication.readingOrder;
  final safePage = page < 1 ? 1 : page;
  final link = order.isEmpty ? null : order.first;
  final pageCount = publication.metadata.numberOfPages;
  final total = pageCount != null && pageCount > 0
      ? (safePage / pageCount).clamp(0.0, 1.0)
      : null;
  return Locator(
    href: link?.href ?? '/',
    type: link?.type ?? 'application/pdf',
    title: link?.title,
    locations: Locations(
      position: safePage,
      progression: total,
      totalProgression: total,
    ),
  );
}

/// Locator aimed at a 1-based CBZ page (one reading-order image per page).
Locator locatorForCbzPage(Publication publication, int page) {
  final order = publication.readingOrder;
  final safePage = page < 1 ? 1 : page;
  if (order.isEmpty) {
    return Locator(
      href: '/',
      type: 'image/jpeg',
      locations: Locations(position: safePage, totalProgression: 0),
    );
  }
  final index = (safePage - 1).clamp(0, order.length - 1);
  final link = order[index];
  final total = (index + 1) / order.length;
  return Locator(
    href: link.href,
    type: link.type ?? 'image/jpeg',
    title: link.title,
    locations: Locations(
      position: index + 1,
      progression: 0,
      totalProgression: total,
    ),
  );
}

/// Restore a Readium [Locator] from a Shelf position string, if possible.
Locator? locatorFromShelfPosition(String? position) {
  if (position == null || position.isEmpty) return null;
  // *page(N) needs the publication reading order — see [restoreReadiumLocator].
  if (pageFromShelfPosition(position, ShelfPagedFormat.pdf) != null ||
      pageFromShelfPosition(position, ShelfPagedFormat.cbz) != null) {
    return null;
  }
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

double shelfPercentageFromLocator(
  Locator locator, {
  ShelfPagedFormat? paged,
  int? pageCount,
  Publication? publication,
}) {
  if (paged != null) {
    final page = pageFromLocator(locator, publication: publication);
    final knownPages =
        pageCount ??
        (paged == ShelfPagedFormat.cbz
            ? publication?.readingOrder.length
            : locator.locations?.totalPages);
    if (page != null && knownPages != null && knownPages > 0) {
      return (page / knownPages).clamp(0.0, 1.0);
    }
  }
  final total = locator.locations?.totalProgression;
  if (total != null && total.isFinite) return total.clamp(0.0, 1.0);
  final progression = locator.locations?.progression;
  if (progression != null && progression.isFinite) {
    return progression.clamp(0.0, 1.0);
  }
  return 0;
}

/// Prefer a prior Readium locator; otherwise seed from Shelf %. Foliate CFIs
/// are not portable — percentage is the cross-engine contract. PDF/CBZ use
/// `*page(N)` shared with the web readers.
Locator? restoreReadiumLocator({
  required Publication publication,
  required String? position,
  required double percentage,
}) {
  final pdfPage = pageFromShelfPosition(position, ShelfPagedFormat.pdf);
  if (pdfPage != null) {
    final seed = locatorForPdfPage(publication, pdfPage);
    return publication.resolveLocator(seed) ?? seed;
  }
  final cbzPage = pageFromShelfPosition(position, ShelfPagedFormat.cbz);
  if (cbzPage != null) {
    final seed = locatorForCbzPage(publication, cbzPage);
    return publication.resolveLocator(seed) ?? seed;
  }
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

/// Status line for the reader chrome (matches web “page · %” style).
String readerProgressLabel({
  required ShelfPagedFormat? paged,
  required Locator? locator,
  required double bookPercentage,
  int? pageCount,
  Publication? publication,
}) {
  final percentage = locator == null
      ? bookPercentage
      : shelfPercentageFromLocator(
          locator,
          paged: paged,
          pageCount: pageCount,
          publication: publication,
        );
  final pct = '${(percentage * 100).round()}%';
  if (paged == null) return pct;
  final page = locator == null
      ? null
      : pageFromLocator(locator, publication: publication);
  final pages =
      pageCount ??
      (paged == ShelfPagedFormat.cbz ? publication?.readingOrder.length : null);
  if (page != null && pages != null && pages > 0) {
    return 'Page $page of $pages · $pct';
  }
  if (page != null) return 'Page $page · $pct';
  return pct;
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
