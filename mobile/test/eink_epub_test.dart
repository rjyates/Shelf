import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/eink_reader.dart';

void main() {
  test('Readium open needs a cache or network to fetch the EPUB', () {
    expect(canOpenEpubReader(hasCachedEpub: true, online: true), isTrue);
    expect(canOpenEpubReader(hasCachedEpub: true, online: false), isTrue);
    expect(canOpenEpubReader(hasCachedEpub: false, online: true), isTrue);
    expect(canOpenEpubReader(hasCachedEpub: false, online: false), isFalse);
  });
}
