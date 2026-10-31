import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/api.dart';
import 'package:shelf_mobile/main.dart';

void main() {
  test('rejects malformed server data before rendering progress', () {
    final valid = <String, dynamic>{
      'id': 1,
      'title': 'Piranesi',
      'author': 'Susanna Clarke',
      'total_pages': 245,
      'pages_read': 163,
    };
    for (final change in [
      {'total_pages': 0},
      {'pages_read': 246},
      {'pages_read': -1},
      {'title': null},
    ]) {
      expect(
        () => Book.fromJson({...valid, ...change}),
        throwsA(isA<ShelfException>()),
      );
    }
  });
  testWidgets('catalogue remains usable at narrow width and double text size', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(320, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(
      MaterialApp(
        home: MediaQuery(
          data: const MediaQueryData(
            size: Size(320, 800),
            textScaler: TextScaler.linear(2),
          ),
          child: LibraryScreen(api: _CatalogueApi()),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Piranesi'), findsOneWidget);
    expect(find.text('Add book'), findsOneWidget);
    expect(find.byTooltip('Import EPUB'), findsNothing);
    expect(tester.takeException(), isNull);
  });
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

  test('does not follow a redirect while signing in', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    final subscription = server.listen((request) async {
      request.response.statusCode = HttpStatus.found;
      request.response.headers.set(
        HttpHeaders.locationHeader,
        'http://example.invalid',
      );
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
      await expectLater(
        api.login('reader', 'test-password-only'),
        throwsA(
          isA<ShelfException>().having(
            (e) => e.message,
            'message',
            contains('redirects are not followed'),
          ),
        ),
      );
    } finally {
      api.close();
      await subscription.cancel();
      await server.close(force: true);
    }
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
          if (request.uri.path.endsWith('/import')) {
            expect(
              request.headers.contentType?.mimeType,
              'application/epub+zip',
            );
            expect(
              await request.fold<List<int>>(
                [],
                (bytes, chunk) => bytes..addAll(chunk),
              ),
              [80, 75, 3, 4],
            );
            request.response.statusCode = HttpStatus.created;
            request.response.write(jsonEncode({'id': 2, 'duplicate': false}));
          } else if (request.method == 'GET') {
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
        expect(await api.importEpub([80, 75, 3, 4]), isFalse);
        expect(requests, [
          'POST /api/v1/login',
          'GET /api/v1/books',
          'POST /api/v1/books/1/progress',
          'POST /api/v1/books/import',
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

class _CatalogueApi extends ShelfApi {
  _CatalogueApi() : super(Uri.parse('https://example.invalid'));
  @override
  Future<List<Book>> books() async => [
    Book(
      id: 1,
      title: 'Piranesi',
      author: 'Susanna Clarke',
      totalPages: 245,
      pagesRead: 163,
    ),
  ];
}
