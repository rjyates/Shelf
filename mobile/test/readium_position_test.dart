import 'package:flutter_readium/flutter_readium.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/readium_position.dart';

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
}
