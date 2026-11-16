import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/api.dart';
import 'package:shelf_mobile/companion.dart';

const _digest =
    '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef';

Book _book({
  required int id,
  required String title,
  String author = 'Rowling',
  required String format,
}) {
  return Book.fromJson({
    'id': id,
    'title': title,
    'author': author,
    'total_pages': null,
    'pages_read': 0,
    'percentage': 0,
    'has_cover': false,
    'digest': _digest,
    'format': format,
    'reading_status': 'open',
  });
}

Work _work({
  required int id,
  required int epub,
  required int audio,
  int? readaloud,
  String status = 'ready',
}) {
  return Work.fromJson({
    'id': id,
    'epub_book_id': epub,
    'audio_book_id': audio,
    'readaloud_book_id': readaloud,
    'status': status,
    'stage': status == 'ready' ? 'SYNC_CHAPTERS' : 'SPLIT_TRACKS',
    'progress': status == 'ready' ? 1 : 0.2,
    'error': null,
  });
}

void main() {
  test(
    'pairs EPUB with audiobook when titles match ignoring parentheticals',
    () {
      final epub = _book(id: 1, title: 'Harry Potter', format: 'epub');
      final audio = _book(
        id: 2,
        title: 'Harry Potter (Full-Cast Edition)',
        format: 'm4b',
      );
      expect(findCompanionEdition(epub, [epub, audio])?.id, 2);
      expect(findCompanionEdition(audio, [epub, audio])?.id, 1);
    },
  );

  test('returns null when no companion edition exists', () {
    final epub = _book(id: 1, title: 'Only Text', format: 'epub');
    expect(findCompanionEdition(epub, [epub]), isNull);
  });

  test('hides audio companion and readaloud from the shelf list', () {
    final epub = _book(id: 1, title: 'Shared', format: 'epub');
    final audio = _book(id: 2, title: 'Shared', format: 'm4b');
    final readaloud = _book(id: 3, title: 'Shared (readaloud)', format: 'epub');
    final hide = hiddenEditionIds([epub, audio, readaloud]);
    expect(hide, {2, 3});
  });

  test('hides audio and readaloud from book_works even when titles differ', () {
    final epub = _book(id: 1, title: 'Text Edition', format: 'epub');
    final audio = _book(id: 2, title: 'Narrated Version', format: 'm4b');
    final readaloud = _book(
      id: 3,
      title: 'Text Edition (readaloud)',
      format: 'epub',
    );
    final works = [_work(id: 9, epub: 1, audio: 2, readaloud: 3)];
    final hide = hiddenEditionIds([epub, audio, readaloud], works: works);
    expect(hide, {2, 3});
    // Titles still differ ⇒ Manage can still offer merge; fold comes from works.
    expect(mergeCandidates(epub, [epub, audio]).single.id, 2);
  });

  test('hides failed-work epub when another ready work owns the audio', () {
    final good = _book(id: 1, title: 'Good', format: 'epub');
    final bad = _book(id: 4, title: 'Bad', format: 'epub');
    final audio = _book(id: 2, title: 'Audio', format: 'm4b');
    final works = [
      _work(id: 1, epub: 1, audio: 2, readaloud: 3, status: 'ready'),
      _work(id: 2, epub: 4, audio: 2, status: 'failed'),
    ];
    final hide = hiddenEditionIds([good, bad, audio], works: works);
    expect(hide.contains(2), isTrue);
    expect(hide.contains(4), isTrue);
    expect(hide.contains(1), isFalse);
  });

  test('mergeCandidates lists opposite format when titles do not match', () {
    final epub = _book(id: 1, title: 'Alpha', format: 'epub');
    final audio = _book(id: 2, title: 'Beta', format: 'm4b');
    expect(mergeCandidates(epub, [epub, audio]).single.id, 2);
    expect(mergeCandidates(audio, [epub, audio]).single.id, 1);
  });

  test('mergeCandidates empty when title/author companion already exists', () {
    final epub = _book(id: 1, title: 'Shared', format: 'epub');
    final audio = _book(id: 2, title: 'Shared', format: 'm4b');
    expect(mergeCandidates(epub, [epub, audio]), isEmpty);
  });
}
