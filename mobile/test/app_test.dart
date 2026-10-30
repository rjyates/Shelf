import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/api.dart';
import 'package:shelf_mobile/main.dart';

void main() {
  test('reads the same book contract fixture as the Rust server', () {
    final fixture = jsonDecode(
      File('../protocol/fixtures/book.json').readAsStringSync(),
    ) as Map<String, dynamic>;
    final book = Book.fromJson(fixture);
    expect(book.totalPages, 245);
    expect(book.pagesRead, 163);
  });
  testWidgets('requires a server and credentials before connecting', (
    tester,
  ) async {
    await tester.pumpWidget(const ShelfApp());
    expect(find.text('Your reading room.'), findsOneWidget);
    await tester.ensureVisible(find.text('Open your library'));
    await tester.tap(find.text('Open your library'));
    await tester.pump();
    expect(find.text('Enter your server address.'), findsOneWidget);
    expect(find.text('Enter your username.'), findsOneWidget);
    expect(find.text('Enter your password.'), findsOneWidget);
  });

  testWidgets('book form rejects an invalid page count', (tester) async {
    await tester.pumpWidget(
      const MaterialApp(home: Scaffold(body: AddBookDialog())),
    );
    await tester.enterText(find.byType(TextFormField).at(0), 'Piranesi');
    await tester.enterText(find.byType(TextFormField).at(2), '0');
    await tester.tap(find.text('Add book'));
    await tester.pump();
    expect(find.text('Enter 1–1,000,000 pages.'), findsOneWidget);
  });

  test('rejects credentials in URLs and remote plain HTTP', () {
    expect(
      () => ShelfApi(Uri.parse('https://user:password@example.com')),
      throwsA(isA<ShelfException>()),
    );
    expect(
      () => ShelfApi(Uri.parse('http://example.com')),
      throwsA(isA<ShelfException>()),
    );
  });

  test(
    'client signs in, fetches books, and records progress over the API',
    () async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      final requests = <String>[];
      final subscription = server.listen((request) async {
        requests.add('${request.method} ${request.uri.path}');
        request.response.headers.contentType = ContentType.json;
        if (request.uri.path.endsWith('/login')) {
          request.response.write(jsonEncode({'token': 'test-session-only'}));
        } else {
          expect(
            request.headers.value(HttpHeaders.authorizationHeader),
            'Bearer test-session-only',
          );
          if (request.method == 'GET') {
            request.response.write(
              jsonEncode([
                {
                  'id': 1,
                  'title': 'Piranesi',
                  'author': 'Susanna Clarke',
                  'total_pages': 245,
                  'pages_read': 10,
                },
              ]),
            );
          } else {
            expect(jsonDecode(await utf8.decoder.bind(request).join()), {
              'pages_read': 163,
            });
            request.response.statusCode = HttpStatus.noContent;
          }
        }
        await request.response.close();
      });
      final client = HttpOverrides.runWithHttpOverrides(
        () => HttpClient(),
        _RealHttpOverrides(),
      );
      final api = ShelfApi(
        Uri.parse('http://127.0.0.1:${server.port}'),
        client: client,
      );
      try {
        await api.login('reader', 'test-password-only');
        final books = await api.books();
        expect(books.single.title, 'Piranesi');
        await api.progress(books.single, 163);
        expect(requests, [
          'POST /api/v1/login',
          'GET /api/v1/books',
          'POST /api/v1/books/1/progress',
        ]);
      } finally {
        api.close();
        await subscription.cancel();
        await server.close(force: true);
      }
    },
  );
}

class _RealHttpOverrides extends HttpOverrides {}
