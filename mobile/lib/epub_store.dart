import 'dart:io';

/// Local imported-file bytes keyed by catalogue id (EPUB or PDF).
class EpubStore {
  EpubStore(this.root);
  final Directory root;

  Directory get _dir => Directory('${root.path}/epubs');

  static String extensionFor({required bool isPdf}) => isPdf ? 'pdf' : 'epub';

  File fileFor(int bookId, {String extension = 'epub'}) =>
      File('${_dir.path}/$bookId.$extension');

  Future<bool> has(int bookId, {String extension = 'epub'}) =>
      fileFor(bookId, extension: extension).exists();

  Future<void> save(
    int bookId,
    List<int> bytes, {
    String extension = 'epub',
  }) async {
    if (bookId < 1) {
      throw ArgumentError.value(bookId, 'bookId');
    }
    await _dir.create(recursive: true);
    await fileFor(
      bookId,
      extension: extension,
    ).writeAsBytes(bytes, flush: true);
  }

  Future<void> remove(int bookId, {String extension = 'epub'}) async {
    final file = fileFor(bookId, extension: extension);
    if (await file.exists()) {
      await file.delete();
    }
  }
}
