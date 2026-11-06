import 'package:flutter_readium/flutter_readium.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/readium_position.dart';

Publication _pub(List<String> hrefs) {
  return Publication(
    metadata: Metadata(localizedTitle: LocalizedString.fromString('t')),
    readingOrder: [
      for (final href in hrefs) Link(href: href, type: 'application/xhtml+xml'),
    ],
  );
}

void main() {
  test('round-trips Readium locators through Shelf positions', () {
    final locator = Locator(
      href: '/OPS/chapter.xhtml',
      type: 'application/xhtml+xml',
      title: 'Chapter',
      locations: Locations(progression: 0.25, totalProgression: 0.4),
    );
    final position = shelfPositionFromLocator(locator);
    expect(position.startsWith('epubcfi(/shelf-readium!'), isTrue);
    expect(position.endsWith(')'), isTrue);
    final restored = locatorFromShelfPosition(position);
    expect(restored?.href, '/OPS/chapter.xhtml');
    expect(restored?.title, 'Chapter');
    expect(shelfPercentageFromLocator(locator), 0.4);
  });

  test('PDF locators persist as pdfpage(N) for web interop', () {
    final locator = Locator(
      href: '/doc.pdf',
      type: 'application/pdf',
      locations: Locations(position: 3, totalProgression: 0.5),
    );
    expect(
      shelfPositionFromLocator(locator, paged: ShelfPagedFormat.pdf),
      'pdfpage(3)',
    );
    expect(pageFromShelfPosition('pdfpage(3)', ShelfPagedFormat.pdf), 3);
    final pub = Publication(
      metadata: Metadata(
        localizedTitle: LocalizedString.fromString('t'),
        numberOfPages: 12,
      ),
      // Real PDFs are one reading-order resource; position is the page number.
      readingOrder: [Link(href: '/doc.pdf', type: 'application/pdf')],
    );
    final restored = restoreReadiumLocator(
      publication: pub,
      position: 'pdfpage(6)',
      percentage: 0.5,
    );
    expect(restored?.locations?.position, 6);
    expect(restored?.href, '/doc.pdf');
    expect(restored?.locations?.totalProgression, 0.5);
    expect(
      shelfPercentageFromLocator(
        Locator(
          href: '/doc.pdf',
          type: 'application/pdf',
          locations: Locations(position: 6),
        ),
        paged: ShelfPagedFormat.pdf,
        pageCount: 12,
      ),
      0.5,
    );
  });

  test('CBZ locators persist as cbzpage(N) via reading-order index', () {
    final pub = Publication(
      metadata: Metadata(localizedTitle: LocalizedString.fromString('t')),
      readingOrder: [
        for (var i = 1; i <= 4; i++)
          Link(href: '/page$i.png', type: 'image/png'),
      ],
    );
    final locator = Locator(
      href: '/page3.png',
      type: 'image/png',
      locations: Locations(),
    );
    expect(
      shelfPositionFromLocator(
        locator,
        paged: ShelfPagedFormat.cbz,
        publication: pub,
      ),
      'cbzpage(3)',
    );
    final restored = restoreReadiumLocator(
      publication: pub,
      position: 'cbzpage(3)',
      percentage: 0.75,
    );
    expect(restored?.href, '/page3.png');
    expect(restored?.locations?.position, 3);
    expect(
      shelfPercentageFromLocator(
        restored!,
        paged: ShelfPagedFormat.cbz,
        publication: pub,
      ),
      0.75,
    );
  });

  test('reader progress label matches web page · percent style', () {
    expect(
      readerProgressLabel(
        paged: ShelfPagedFormat.pdf,
        locator: Locator(
          href: '/doc.pdf',
          type: 'application/pdf',
          locations: Locations(position: 6),
        ),
        bookPercentage: 0,
        pageCount: 12,
      ),
      'Page 6 of 12 · 50%',
    );
    expect(
      readerProgressLabel(
        paged: null,
        locator: Locator(
          href: '/a.xhtml',
          type: 'application/xhtml+xml',
          locations: Locations(totalProgression: 0.42),
        ),
        bookPercentage: 0.1,
      ),
      '42%',
    );
  });

  test('PDF page= fragments encode as pdfpage(N)', () {
    final locator = Locator(
      href: '/doc.pdf',
      type: 'application/pdf',
      locations: Locations(fragments: ['page=6']),
    );
    expect(
      shelfPositionFromLocator(locator, paged: ShelfPagedFormat.pdf),
      'pdfpage(6)',
    );
    expect(pageFromLocator(locator), 6);
  });

  test('prefers an existing epubcfi fragment', () {
    final locator = Locator(
      href: '/OPS/chapter.xhtml',
      type: 'application/xhtml+xml',
      locations: Locations(
        fragments: ['epubcfi(/6/4!/4/2/1:0)'],
        totalProgression: 0.1,
      ),
    );
    expect(shelfPositionFromLocator(locator), 'epubcfi(/6/4!/4/2/1:0)');
  });

  test('Foliate CFIs are not treated as navigable Readium locators', () {
    expect(locatorFromShelfPosition('epubcfi(/6/4!/4/2/1:0)'), isNull);
    expect(
      isNavigableReadiumLocator(
        Locator(href: '#', type: 'application/xhtml+xml'),
      ),
      isFalse,
    );
  });

  test('maps total percentage onto a reading-order locator', () {
    final publication = _pub(['/a.xhtml', '/b.xhtml', '/c.xhtml', '/d.xhtml']);
    final locator = locatorForTotalProgression(publication, 0.58);
    expect(locator.href, '/c.xhtml');
    expect(locator.locations?.totalProgression, 0.58);
    expect(locator.locations?.position, 3);
  });

  test('restore prefers packed Readium locator over Foliate CFI', () {
    final publication = _pub(['/a.xhtml', '/b.xhtml', '/c.xhtml']);
    final packed = shelfPositionFromLocator(
      Locator(
        href: '/b.xhtml',
        type: 'application/xhtml+xml',
        locations: Locations(totalProgression: 0.4),
      ),
    );
    final restored = restoreReadiumLocator(
      publication: publication,
      position: packed,
      percentage: 0.4,
    );
    expect(restored?.href, '/b.xhtml');
  });

  test('restore falls back to percentage when Foliate CFI is stored', () {
    final publication = _pub(['/a.xhtml', '/b.xhtml', '/c.xhtml', '/d.xhtml']);
    final restored = restoreReadiumLocator(
      publication: publication,
      position: 'epubcfi(/6/24!/4/2/1:0)',
      percentage: 0.58,
    );
    expect(restored?.href, '/c.xhtml');
    expect(restored?.locations?.totalProgression, 0.58);
  });

  test('restore ignores a zero Readium locator when Shelf % is meaningful', () {
    final publication = _pub(['/a.xhtml', '/b.xhtml', '/c.xhtml', '/d.xhtml']);
    final packed = shelfPositionFromLocator(
      Locator(
        href: '/a.xhtml',
        type: 'application/xhtml+xml',
        locations: Locations(totalProgression: 0),
      ),
    );
    final restored = restoreReadiumLocator(
      publication: publication,
      position: packed,
      percentage: 0.58,
    );
    expect(restored?.href, '/c.xhtml');
    expect(restored?.locations?.totalProgression, 0.58);
  });

  test('defers persist while opening would clobber saved percentage', () {
    final opened = DateTime.utc(2026, 1, 1, 12);
    expect(
      shouldDeferPersistDuringRestore(
        openedAt: opened,
        now: opened.add(const Duration(seconds: 2)),
        bookPercentage: 0.58,
        locatorPercentage: 0,
      ),
      isTrue,
    );
    expect(
      shouldDeferPersistDuringRestore(
        openedAt: opened,
        now: opened.add(const Duration(seconds: 9)),
        bookPercentage: 0.58,
        locatorPercentage: 0,
      ),
      isFalse,
    );
    expect(
      shouldDeferPersistDuringRestore(
        openedAt: opened,
        now: opened.add(const Duration(seconds: 1)),
        bookPercentage: 0.58,
        locatorPercentage: 0.55,
      ),
      isFalse,
    );
  });
}
