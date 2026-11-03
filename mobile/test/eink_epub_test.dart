import 'dart:convert';
import 'dart:typed_data';

import 'package:archive/archive.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/display_profile.dart';
import 'package:shelf_mobile/eink_epub.dart';
import 'package:shelf_mobile/eink_reader.dart';

Uint8List sampleEinkEpub({bool twoChapters = false}) {
  final archive = Archive();
  void add(String name, String body) {
    final bytes = utf8.encode(body);
    archive.addFile(ArchiveFile(name, bytes.length, bytes));
  }

  add('mimetype', 'application/epub+zip');
  add(
    'META-INF/container.xml',
    '''<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>''',
  );
  final secondItem = twoChapters
      ? '<item id="chapter2" href="chapter2.xhtml" media-type="application/xhtml+xml"/>'
      : '';
  final secondSpine = twoChapters ? '<itemref idref="chapter2"/>' : '';
  add(
    'package.opf',
    '''<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">shelf-test</dc:identifier><dc:title>A test chapter</dc:title><dc:language>en</dc:language></metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>$secondItem<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest><spine><itemref idref="chapter"/>$secondSpine</spine></package>''',
  );
  add(
    'chapter.xhtml',
    '<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Chapter</title></head><body><h1>A test chapter</h1><p>Original synthetic test text.</p></body></html>',
  );
  if (twoChapters) {
    add(
      'chapter2.xhtml',
      '<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Chapter two</title></head><body><p>Second chapter body.</p></body></html>',
    );
  }
  add(
    'nav.xhtml',
    '<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Contents</title></head><body><nav><ol><li><a href="chapter.xhtml">Chapter</a></li></ol></nav></body></html>',
  );
  return Uint8List.fromList(ZipEncoder().encode(archive));
}

void main() {
  test('loads spine text from a local EPUB', () {
    final spine = loadEinkSpine(sampleEinkEpub());
    expect(spine, isNotEmpty);
    expect(spine.first.text, contains('Original synthetic test text'));
    expect(spine.first.title, 'Chapter');
  });

  test('CFI and percentage round-trip spine index', () {
    expect(parseEinkSpineIndex(einkSpineCfi(3)), 3);
    expect(parseEinkSpineIndex('epubcfi(/6/2!/4)'), isNull);
    expect(einkSpinePercentage(0, 1), 0);
    expect(einkSpinePercentage(1, 3), 0.5);
    expect(
      initialEinkSpineIndex(
        position: einkSpineCfi(1),
        percentage: 0,
        length: 3,
      ),
      1,
    );
    expect(initialEinkSpineIndex(position: null, percentage: 1, length: 3), 2);
  });

  test('e-ink reader gate requires Android, profile, and cache', () {
    expect(
      shouldOpenEinkReader(
        profile: DisplayProfile.eink,
        isAndroid: true,
        hasCachedEpub: true,
      ),
      isTrue,
    );
    expect(
      shouldOpenEinkReader(
        profile: DisplayProfile.lcd,
        isAndroid: true,
        hasCachedEpub: true,
      ),
      isFalse,
    );
    expect(
      shouldOpenEinkReader(
        profile: DisplayProfile.eink,
        isAndroid: false,
        hasCachedEpub: true,
      ),
      isFalse,
    );
    expect(
      shouldOpenEinkReader(
        profile: DisplayProfile.eink,
        isAndroid: true,
        hasCachedEpub: false,
      ),
      isFalse,
    );
  });

  test('two-chapter EPUB reports both spine items', () {
    final spine = loadEinkSpine(sampleEinkEpub(twoChapters: true));
    expect(spine.length, greaterThanOrEqualTo(2));
    expect(spine[1].text, contains('Second chapter body'));
  });
}
