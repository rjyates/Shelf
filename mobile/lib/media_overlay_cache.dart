import 'dart:convert';
import 'dart:io';

import 'media_overlay_playlist.dart';

/// Persists Shelf media-overlay playlists beside cached EPUBs across restarts.
class MediaOverlayCache {
  MediaOverlayCache(this.root);
  final Directory root;

  File fileFor(int bookId) =>
      File('${root.path}/epubs/$bookId.media-overlay.json');

  Future<void> save(int bookId, MediaOverlayPlaylist playlist) async {
    if (bookId < 1) return;
    final file = fileFor(bookId);
    await file.parent.create(recursive: true);
    await file.writeAsString(jsonEncode(playlist.toJson()), flush: true);
  }

  Future<MediaOverlayPlaylist?> load(int bookId) async {
    if (bookId < 1) return null;
    final file = fileFor(bookId);
    if (!await file.exists()) return null;
    try {
      final decoded = jsonDecode(await file.readAsString());
      if (decoded is! Map<String, dynamic>) return null;
      return MediaOverlayPlaylist.fromJson(decoded);
    } on Object {
      return null;
    }
  }

  Future<void> remove(int bookId) async {
    final file = fileFor(bookId);
    if (await file.exists()) await file.delete();
  }
}
