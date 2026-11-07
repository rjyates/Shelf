import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/media_overlay_cache.dart';
import 'package:shelf_mobile/media_overlay_playlist.dart';

void main() {
  test('round-trips playlist JSON beside the epub cache', () async {
    final dir = await Directory.systemTemp.createTemp('shelf-mo-cache');
    addTearDown(() async {
      if (await dir.exists()) await dir.delete(recursive: true);
    });
    final cache = MediaOverlayCache(dir);
    final playlist = MediaOverlayPlaylist(
      bookId: 42,
      items: [
        MediaOverlayItem(
          index: 0,
          sectionIndex: 0,
          audioHref: 'Audio/00001.mp3',
          clipBeginSeconds: 0.5,
          clipEndSeconds: 2.0,
          textHref: 'Text/ch.xhtml',
          textFragment: 's1',
        ),
      ],
    );
    await cache.save(42, playlist);
    final loaded = await cache.load(42);
    expect(loaded, isNotNull);
    expect(loaded!.bookId, 42);
    expect(loaded.items.single.audioHref, 'Audio/00001.mp3');
    expect(loaded.items.single.textFragment, 's1');
    expect(await cache.fileFor(42).exists(), isTrue);
  });
}
