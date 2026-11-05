import 'dart:io';

/// Local EPUB bytes keyed by catalogue id (prep for offline reading / M6).
class EpubStore {
  EpubStore(this.root);
  final Directory root;

  Directory get _dir => Directory('${root.path}/epubs');

  File fileFor(int bookId) => File('${_dir.path}/$bookId.epub');

  Future<bool> has(int bookId) => fileFor(bookId).exists();

  Future<void> save(int bookId, List<int> bytes) async {
    if (bookId < 1) {
      throw ArgumentError.value(bookId, 'bookId');
    }
    await _dir.create(recursive: true);
    await fileFor(bookId).writeAsBytes(bytes, flush: true);
  }

  Future<void> remove(int bookId) async {
    final file = fileFor(bookId);
    if (await file.exists()) {
      await file.delete();
    }
  }
}
