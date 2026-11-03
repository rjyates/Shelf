import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/display_profile.dart';
import 'package:shelf_mobile/eink_reader.dart';

void main() {
  test('local reader gate covers e-ink online and any offline cache', () {
    expect(
      shouldOpenLocalEpubReader(
        profile: DisplayProfile.eink,
        isAndroid: true,
        hasCachedEpub: true,
        online: true,
      ),
      isTrue,
    );
    expect(
      shouldOpenLocalEpubReader(
        profile: DisplayProfile.lcd,
        isAndroid: true,
        hasCachedEpub: true,
        online: true,
      ),
      isFalse,
    );
    expect(
      shouldOpenLocalEpubReader(
        profile: DisplayProfile.lcd,
        isAndroid: false,
        hasCachedEpub: true,
        online: false,
      ),
      isTrue,
    );
    expect(
      shouldOpenLocalEpubReader(
        profile: DisplayProfile.eink,
        isAndroid: true,
        hasCachedEpub: false,
        online: false,
      ),
      isFalse,
    );
  });
}
