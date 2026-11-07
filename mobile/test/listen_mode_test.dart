import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/listen_mode.dart';

void main() {
  test('server TTS listen requires epub, online, and providers', () {
    expect(
      shelfServerTtsListenAvailable(
        isEpub: true,
        online: true,
        hasEnabledProviders: true,
      ),
      isTrue,
    );
    expect(
      shelfServerTtsListenAvailable(
        isEpub: false,
        online: true,
        hasEnabledProviders: true,
      ),
      isFalse,
    );
    expect(
      shelfServerTtsListenAvailable(
        isEpub: true,
        online: false,
        hasEnabledProviders: true,
      ),
      isFalse,
    );
    expect(
      shelfServerTtsListenAvailable(
        isEpub: true,
        online: true,
        hasEnabledProviders: false,
      ),
      isFalse,
    );
  });

  test('media overlay listen works offline once playlist is known', () {
    expect(
      shelfMediaOverlayListenAvailable(isEpub: true, hasMediaOverlay: true),
      isTrue,
    );
    expect(
      shelfListenChromeAvailable(
        isEpub: true,
        online: false,
        hasEnabledProviders: false,
        hasMediaOverlay: true,
      ),
      isTrue,
    );
    expect(
      shelfListenChromeAvailable(
        isEpub: true,
        online: false,
        hasEnabledProviders: false,
        hasMediaOverlay: false,
      ),
      isFalse,
    );
  });

  test('chapter index matches reading-order href paths', () {
    const spine = [
      'OEBPS/cover.xhtml',
      'OEBPS/chap01.xhtml',
      'OEBPS/chap02.xhtml',
    ];
    expect(
      shelfTtsChapterIndex(
        readingOrderHrefs: spine,
        locatorHref: 'OEBPS/chap01.xhtml#para-2',
      ),
      1,
    );
    expect(
      shelfTtsChapterIndex(
        readingOrderHrefs: spine,
        locatorHref: '/OEBPS/chap02.xhtml',
      ),
      2,
    );
    expect(
      shelfTtsChapterIndex(readingOrderHrefs: spine, locatorHref: null),
      0,
    );
  });
}
