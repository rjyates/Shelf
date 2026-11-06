import 'package:flutter/material.dart' show Color;
import 'package:flutter_readium/flutter_readium.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Local comic view prefs (scroll / RTL / spread) for Readium CBZ/DiViNa.
class ComicViewPrefs {
  const ComicViewPrefs({
    this.scroll = false,
    this.rtl = false,
    this.spread = false,
  });

  final bool scroll;
  final bool rtl;

  /// Dual-page synthetic spread when the navigator supports it.
  final bool spread;

  ComicViewPrefs copyWith({bool? scroll, bool? rtl, bool? spread}) =>
      ComicViewPrefs(
        scroll: scroll ?? this.scroll,
        rtl: rtl ?? this.rtl,
        spread: spread ?? this.spread,
      );

  EPUBPreferences toReadium() => EPUBPreferences(
    scroll: scroll,
    readingProgression: rtl
        ? EpubReadingProgression.rtl
        : EpubReadingProgression.ltr,
    // Readium FXL spread: "auto" | "never" | "always"
    spread: spread ? 'auto' : 'never',
    backgroundColor: const Color(0xffffffff),
    publisherStyles: false,
  );

  static String _key(int bookId, String field) => 'comic.$bookId.$field';

  static Future<ComicViewPrefs> load(int bookId) async {
    final prefs = await SharedPreferences.getInstance();
    return ComicViewPrefs(
      scroll: prefs.getBool(_key(bookId, 'scroll')) ?? false,
      rtl: prefs.getBool(_key(bookId, 'rtl')) ?? false,
      spread: prefs.getBool(_key(bookId, 'spread')) ?? false,
    );
  }

  Future<void> save(int bookId) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setBool(_key(bookId, 'scroll'), scroll);
    await prefs.setBool(_key(bookId, 'rtl'), rtl);
    await prefs.setBool(_key(bookId, 'spread'), spread);
  }
}
