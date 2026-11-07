import 'dart:io';

/// Local imported-file bytes keyed by catalogue id (+ content digest when known).
class EpubStore {
  EpubStore(this.root);
  final Directory root;

  Directory get _dir => Directory('${root.path}/epubs');

  static String extensionFor({bool isPdf = false, bool isCbz = false}) {
    if (isCbz) return 'cbz';
    if (isPdf) return 'pdf';
    return 'epub';
  }

  /// Prefer `{id}-{digest}.ext` so a reused catalogue id cannot open stale bytes.
  File fileFor(int bookId, {String extension = 'epub', String? digest}) {
    final name = (digest != null && digest.isNotEmpty)
        ? '$bookId-$digest.$extension'
        : '$bookId.$extension';
    return File('${_dir.path}/$name');
  }

  Future<bool> has(int bookId, {String extension = 'epub', String? digest}) =>
      fileFor(bookId, extension: extension, digest: digest).exists();

  Future<void> save(
    int bookId,
    List<int> bytes, {
    String extension = 'epub',
    String? digest,
  }) async {
    if (bookId < 1) {
      throw ArgumentError.value(bookId, 'bookId');
    }
    await _dir.create(recursive: true);
    if (digest != null && digest.isNotEmpty) {
      await _purgeStale(bookId, extension: extension, keepDigest: digest);
    }
    await fileFor(
      bookId,
      extension: extension,
      digest: digest,
    ).writeAsBytes(bytes, flush: true);
  }

  /// Download [write] straight into the cache file (for multi‑GB books).
  Future<void> saveViaDownload(
    int bookId,
    Future<void> Function(File dest) write, {
    String extension = 'epub',
    String? digest,
  }) async {
    if (bookId < 1) {
      throw ArgumentError.value(bookId, 'bookId');
    }
    await _dir.create(recursive: true);
    if (digest != null && digest.isNotEmpty) {
      await _purgeStale(bookId, extension: extension, keepDigest: digest);
    }
    final dest = fileFor(bookId, extension: extension, digest: digest);
    await write(dest);
  }

  Future<void> remove(
    int bookId, {
    String extension = 'epub',
    String? digest,
  }) async {
    if (digest != null && digest.isNotEmpty) {
      await _purgeStale(bookId, extension: extension, keepDigest: '');
      return;
    }
    final file = fileFor(bookId, extension: extension);
    if (await file.exists()) {
      await file.delete();
    }
  }

  /// Drop legacy `{id}.ext` and other `{id}-*.ext` copies for this book.
  Future<void> _purgeStale(
    int bookId, {
    required String extension,
    required String keepDigest,
  }) async {
    if (!await _dir.exists()) return;
    final legacy = File('${_dir.path}/$bookId.$extension');
    if (await legacy.exists()) {
      await legacy.delete();
    }
    final prefix = '$bookId-';
    final suffix = '.$extension';
    await for (final entity in _dir.list()) {
      if (entity is! File) continue;
      final name = entity.uri.pathSegments.last;
      if (!name.startsWith(prefix) || !name.endsWith(suffix)) continue;
      final mid = name.substring(prefix.length, name.length - suffix.length);
      if (mid.isEmpty || mid == keepDigest) continue;
      await entity.delete();
    }
  }
}
