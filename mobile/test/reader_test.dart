import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/api.dart';
import 'package:shelf_mobile/reader.dart';

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
  test('reader confines navigation to its server and sandbox frames', () {
    final base = Uri.parse('https://shelf.example.com');
    expect(
      readerNavigationAllowed(
        base,
        'https://shelf.example.com/reader?id=1',
        true,
      ),
      isTrue,
    );
    expect(
      readerNavigationAllowed(
        base,
        'https://shelf.example.com/reader/1/1?format=epub',
        true,
      ),
      isTrue,
    );
    expect(
      readerNavigationAllowed(
        base,
        'https://shelf.example.com/orbit-reader/orbit-reader.js',
        true,
      ),
      isTrue,
    );
    expect(
      readerNavigationAllowed(base, 'https://other.example.com/reader', true),
      isFalse,
    );
    expect(readerNavigationAllowed(base, 'javascript:alert(1)', true), isFalse);
    expect(readerNavigationAllowed(base, 'about:blank', true), isFalse);
    expect(readerNavigationAllowed(base, 'about:srcdoc', false), isTrue);
    expect(
      readerNavigationAllowed(
        base,
        'blob:https://other.example.com/123',
        false,
      ),
      isFalse,
    );
    expect(
      readerNavigationAllowed(
        base,
        'blob:https://shelf.example.com/123',
        false,
      ),
      isTrue,
    );
    expect(
      readerNavigationAllowed(
        base,
        'https://shelf.example.com/api/v1/logout',
        true,
      ),
      isFalse,
    );
    expect(
      readerNavigationAllowed(base, 'https://shelf.example.com/', true),
      isFalse,
    );
  });

  test('reader exit URLs are the server root including library hash', () {
    final base = Uri.parse('https://shelf.example.com');
    expect(readerIsExitUrl(base, 'https://shelf.example.com/'), isTrue);
    expect(readerIsExitUrl(base, 'https://shelf.example.com/#library'), isTrue);
    expect(
      readerIsExitUrl(base, 'https://shelf.example.com/reader?id=1'),
      isFalse,
    );
    expect(
      readerIsExitUrl(base, 'https://other.example.com/#library'),
      isFalse,
    );
  });

  test('reader cookie domain is the server host', () {
    expect(
      readerCookieDomain(Uri.parse('https://shelf.example.com')),
      'shelf.example.com',
    );
    expect(readerCookieDomain(Uri.parse('http://127.0.0.1:8080')), '127.0.0.1');
  });
}
