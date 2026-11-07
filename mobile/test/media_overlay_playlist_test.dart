import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/media_overlay_playlist.dart';

void main() {
  test('parses playlist items used for mobile read-along', () {
    final playlist = MediaOverlayPlaylist.fromJson({
      'bookId': 7,
      'fileId': 7,
      'items': [
        {
          'index': 0,
          'sectionIndex': 1,
          'audioHref': 'Audio/00001-00001.mp3',
          'clipBeginSeconds': 0.0,
          'clipEndSeconds': 1.5,
          'textHref': 'Text/chapter.xhtml',
          'textFragment': 's1',
        },
      ],
      'sections': [],
      'resources': [],
    });
    expect(playlist.hasNarration, isTrue);
    expect(playlist.items.single.audioHref, 'Audio/00001-00001.mp3');
    expect(playlist.items.single.clipEndSeconds, 1.5);
  });

  test('empty items means no read-along chrome', () {
    final playlist = MediaOverlayPlaylist.fromJson({
      'bookId': 1,
      'items': <dynamic>[],
    });
    expect(playlist.hasNarration, isFalse);
  });

  test('adjacent SMIL clips in one file keep continuous playback', () {
    final a = MediaOverlayItem(
      index: 0,
      sectionIndex: 0,
      audioHref: 'Audio/a.mp3',
      clipBeginSeconds: 0,
      clipEndSeconds: 1.5,
    );
    final b = MediaOverlayItem(
      index: 1,
      sectionIndex: 0,
      audioHref: 'Audio/a.mp3',
      clipBeginSeconds: 1.5,
      clipEndSeconds: 3.0,
    );
    final gap = MediaOverlayItem(
      index: 2,
      sectionIndex: 0,
      audioHref: 'Audio/a.mp3',
      clipBeginSeconds: 4.0,
      clipEndSeconds: 5.0,
    );
    final other = MediaOverlayItem(
      index: 3,
      sectionIndex: 0,
      audioHref: 'Audio/b.mp3',
      clipBeginSeconds: 0,
      clipEndSeconds: 1,
    );
    expect(mediaOverlayClipsAreAdjacent(a, b), isTrue);
    expect(mediaOverlayClipsAreAdjacent(b, gap), isFalse);
    expect(mediaOverlayClipsAreAdjacent(a, other), isFalse);
  });

  test('start index prefers the visible text fragment', () {
    final items = [
      MediaOverlayItem(
        index: 0,
        sectionIndex: 1,
        audioHref: 'a.mp3',
        clipBeginSeconds: 0,
        clipEndSeconds: 1,
        textFragment: 's0',
      ),
      MediaOverlayItem(
        index: 1,
        sectionIndex: 1,
        audioHref: 'a.mp3',
        clipBeginSeconds: 1,
        clipEndSeconds: 2,
        textFragment: 's14',
      ),
      MediaOverlayItem(
        index: 2,
        sectionIndex: 2,
        audioHref: 'b.mp3',
        clipBeginSeconds: 0,
        clipEndSeconds: 1,
        textFragment: 't0',
      ),
    ];
    expect(
      mediaOverlayStartIndex(
        items: items,
        sectionIndex: 1,
        textFragment: 's14',
      ),
      1,
    );
    expect(
      mediaOverlayStartIndex(items: items, sectionIndex: 2, textFragment: null),
      2,
    );
  });

  test('start index uses in-resource progression when fragment is missing', () {
    // Long enough that a "page" of audio (~40s) is a slice of the resource.
    final items = [
      for (var i = 0; i < 100; i++)
        MediaOverlayItem(
          index: i,
          sectionIndex: 4,
          audioHref: 'Audio/ch4.mp3',
          clipBeginSeconds: i * 1.0,
          clipEndSeconds: i * 1.0 + 1.0,
          textHref: 'OEBPS/index_split_010.html',
          textFragment: 'id142-s$i',
        ),
    ];
    // Cold start: mid-viewport progression pulls back to the page top.
    expect(
      mediaOverlayStartIndex(
        items: items,
        sectionIndex: 4,
        textHref: '/OEBPS/index_split_010.html',
        resourceProgression: 0.55,
        biasToVisiblePageTop: true,
      ),
      35, // 55s - 20s half-page pullback
    );
    // Known progress on this page: no pullback.
    expect(
      mediaOverlayStartIndex(
        items: items,
        sectionIndex: 4,
        textHref: '/OEBPS/index_split_010.html',
        resourceProgression: 0.55,
        biasToVisiblePageTop: false,
      ),
      55,
    );
    // Stop/restart on the same HTML resumes the saved cue.
    expect(
      mediaOverlayStartIndex(
        items: items,
        sectionIndex: 4,
        textHref: '/OEBPS/index_split_010.html',
        resourceProgression: 0.1,
        biasToVisiblePageTop: true,
        resumeIndex: 62,
        resumeTextHref: 'OEBPS/index_split_010.html',
      ),
      62,
    );
    expect(
      mediaOverlayStartIndex(
        items: items,
        sectionIndex: 4,
        textHref: 'index_split_010.html',
        resourceProgression: 0.0,
        biasToVisiblePageTop: true,
      ),
      0,
    );
  });
}
