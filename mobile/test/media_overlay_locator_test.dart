import 'package:flutter_readium/flutter_readium.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/media_overlay_locator.dart';
import 'package:shelf_mobile/media_overlay_playlist.dart';

void main() {
  test('e-ink highlight refresh waits ~2s on the same page', () {
    expect(
      shouldRefreshMediaOverlayHighlight(
        paintedTextHref: null,
        currentTextHref: 'ch1.xhtml',
        secondsSincePaint: 0,
      ),
      isTrue,
    );
    expect(
      shouldRefreshMediaOverlayHighlight(
        paintedTextHref: 'ch1.xhtml',
        currentTextHref: 'ch1.xhtml',
        secondsSincePaint: 0.5,
      ),
      isFalse,
    );
    expect(
      shouldRefreshMediaOverlayHighlight(
        paintedTextHref: 'ch1.xhtml',
        currentTextHref: 'ch1.xhtml',
        secondsSincePaint: 2.0,
      ),
      isTrue,
    );
    expect(
      shouldRefreshMediaOverlayHighlight(
        paintedTextHref: 'ch1.xhtml',
        currentTextHref: 'ch2.xhtml',
        secondsSincePaint: 0.1,
      ),
      isTrue,
    );
  });

  test('builds a fragment locator for the SMIL text target', () {
    final publication = Publication(
      metadata: Metadata(
        localizedTitle: LocalizedString.fromJsonString('Read aloud'),
      ),
      readingOrder: const [
        Link(href: '/OEBPS/chap01.xhtml', type: 'application/xhtml+xml'),
      ],
    );
    final locator = locatorForMediaOverlayItem(
      item: MediaOverlayItem(
        index: 0,
        sectionIndex: 0,
        audioHref: 'Audio/a.mp3',
        clipBeginSeconds: 0,
        clipEndSeconds: 1,
        textHref: 'OEBPS/chap01.xhtml',
        textFragment: 'sentence14',
      ),
      publication: publication,
    );
    expect(locator, isNotNull);
    expect(locator!.locations?.fragments, ['sentence14']);
    expect(locator.locations?.cssSelector, '#sentence14');
  });
}
