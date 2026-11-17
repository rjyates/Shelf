import 'api.dart';

/// Normalize catalogue titles so "(Full-Cast Edition)" etc. do not block pairing.
String companionKey(String title, String author) {
  var t = title.toLowerCase().trim();
  t = t.replaceAll(RegExp(r'\([^)]*\)'), ' ');
  t = t.replaceAll(RegExp(r'\s+'), ' ').trim();
  final a = author.toLowerCase().trim();
  return '$t|$a';
}

/// Find the other edition (EPUB text ↔ audiobook) for the same work when present.
/// shortcut: title/author pairing only; works API owns STAlign pairs with different titles.
Book? findCompanionEdition(Book book, Iterable<Book> catalogue) {
  final key = companionKey(book.title, book.author);
  if (key.startsWith('|') || book.title.trim().isEmpty) return null;
  final wantAudio = book.opensInReader;
  final wantReader = book.opensAsAudiobook;
  if (!wantAudio && !wantReader) return null;
  for (final other in catalogue) {
    if (other.id == book.id) continue;
    if (companionKey(other.title, other.author) != key) continue;
    if (wantAudio && other.opensAsAudiobook) return other;
    if (wantReader && other.opensInReader) return other;
  }
  return null;
}

/// Opposite-format editions for Manage → Merge & Combine (titles may differ).
List<Book> mergeCandidates(Book book, Iterable<Book> catalogue) {
  if (findCompanionEdition(book, catalogue) != null) return [];
  if (book.isEpub) {
    return [
      for (final other in catalogue)
        if (other.id != book.id && other.opensAsAudiobook) other,
    ];
  }
  if (book.opensAsAudiobook) {
    return [
      for (final other in catalogue)
        if (other.id != book.id && other.isEpub) other,
    ];
  }
  return [];
}

/// Editions folded into another library card (audio companion + readaloud).
/// Mirrors web `hiddenEditionIds`: book_works hide audio/readaloud; title fold remains.
Set<int> hiddenEditionIds(
  Iterable<Book> catalogue, {
  Iterable<Work> works = const [],
}) {
  final hide = <int>{};
  final readyByAudio = <int, Work>{};
  for (final w in works) {
    if (w.isReady) readyByAudio[w.audioBookId] = w;
  }
  for (final w in works) {
    if (w.readaloudBookId != null) hide.add(w.readaloudBookId!);
    hide.add(w.audioBookId);
    final ready = readyByAudio[w.audioBookId];
    if (ready != null && w.epubBookId != ready.epubBookId) {
      hide.add(w.epubBookId);
    }
  }
  for (final book in catalogue) {
    if (RegExp(
      r'\(readaloud\)\s*$',
      caseSensitive: false,
    ).hasMatch(book.title)) {
      hide.add(book.id);
      continue;
    }
    if (book.opensAsAudiobook &&
        findCompanionEdition(book, catalogue) != null) {
      hide.add(book.id);
    }
  }
  return hide;
}
