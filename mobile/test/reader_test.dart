import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/api.dart';

void main() {
  test('reads the shared EPUB contract fixture', () {
    final book = Book.fromJson(
      jsonDecode(File('../protocol/fixtures/epub-book.json').readAsStringSync())
          as Map<String, dynamic>,
    );
    expect(book.totalPages, isNull);
    expect(book.percentage, 0.5);
    expect(book.isEpub, isTrue);
  });
  test('EPUB entries use percentage without inventing page counts', () {
    final book = Book.fromJson({
      'id': 1,
      'title': 'Test EPUB',
      'author': 'Tests',
      'total_pages': null,
      'pages_read': 0,
      'format': 'epub',
      'percentage': 0.42,
    });
    expect(book.isEpub, isTrue);
    expect(book.totalPages, isNull);
    expect(book.percentage, 0.42);
    expect(
      () => Book.fromJson({
        'id': 1,
        'title': 'Bad',
        'author': '',
        'total_pages': null,
        'pages_read': 0,
        'format': 'epub',
        'percentage': 2,
      }),
      throwsA(isA<ShelfException>()),
    );
  });

  test('library blurb uses average progress instead of EPUB pages_read', () {
    expect(libraryReadingBlurb([]), '0 books');
    expect(
      libraryReadingBlurb([
        Book.fromJson({
          'id': 1,
          'title': 'A',
          'author': 'B',
          'total_pages': null,
          'pages_read': 0,
          'format': 'epub',
          'percentage': 0,
        }),
      ]),
      '1 books',
    );
    expect(
      libraryReadingBlurb([
        Book.fromJson({
          'id': 1,
          'title': 'A',
          'author': 'B',
          'total_pages': null,
          'pages_read': 0,
          'format': 'epub',
          'percentage': 0.5,
        }),
        Book.fromJson({
          'id': 2,
          'title': 'C',
          'author': 'D',
          'total_pages': 100,
          'pages_read': 25,
        }),
      ]),
      '2 books · 38% average progress',
    );
  });
}
