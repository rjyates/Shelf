import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/api.dart';

void main() {
  test('Book.fromJson accepts optional series fields', () {
    final book = Book.fromJson({
      'id': 1,
      'title': 'Piranesi',
      'author': 'Susanna Clarke',
      'total_pages': 245,
      'pages_read': 10,
      'series': 'Clarke',
      'series_index': 1.5,
    });
    expect(book.series, 'Clarke');
    expect(book.seriesIndex, 1.5);
  });

  test('Book.fromJson keeps series null when missing or wrong type', () {
    final missing = Book.fromJson({
      'id': 1,
      'title': 'Piranesi',
      'author': 'Susanna Clarke',
      'total_pages': 245,
      'pages_read': 10,
    });
    expect(missing.series, isNull);
    expect(missing.seriesIndex, isNull);

    final bad = Book.fromJson({
      'id': 1,
      'title': 'Piranesi',
      'author': 'Susanna Clarke',
      'total_pages': 245,
      'pages_read': 10,
      'series': 3,
      'series_index': 'x',
    });
    expect(bad.series, isNull);
    expect(bad.seriesIndex, isNull);
  });

  test('BooksQuery encodes only non-empty filters', () {
    expect(const BooksQuery().toQueryString(), '');
    expect(
      const BooksQuery(
        author: 'Clarke',
        series: 'Piranesi',
        limit: 20,
      ).toQueryString(),
      '?limit=20&author=Clarke&series=Piranesi',
    );
    expect(
      const BooksQuery(collectionId: 4, q: 'door').toQueryString(),
      '?collection_id=4&q=door',
    );
  });

  test('parses authors, collections, scopes, and scan payloads', () {
    final author = AuthorInfo.fromJson({'name': 'Clarke', 'book_count': 2});
    expect(author.bookCount, 2);

    final collection = CollectionInfo.fromJson({
      'id': 9,
      'name': 'Favorites',
      'book_ids': [1, 2],
      'book_count': 2,
    });
    expect(collection.bookIds, [1, 2]);

    final scope = ScopeInfo.fromJson({
      'id': 3,
      'name': 'Clarke open',
      'filter': {'author': 'Clarke', 'status': 'open'},
    });
    expect(scope.filter.author, 'Clarke');
    expect(scope.filter.toBooksQuery().author, 'Clarke');

    final scan = LibraryScanResult.fromJson({
      'scanned': 5,
      'imported': 2,
      'duplicates': 1,
      'failed': [
        {'path': '/bad.epub', 'error': 'corrupt'},
      ],
    });
    expect(scan.imported, 2);
    expect(scan.failed.single['path'], '/bad.epub');
  });

  test('updateBook sends series fields and books uses query string', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    final paths = <String>[];
    final bodies = <Map<String, dynamic>>[];
    final subscription = server.listen((request) async {
      paths.add('${request.method} ${request.uri}');
      request.response.headers.contentType = ContentType.json;
      if (request.uri.path.endsWith('/login')) {
        request.response.write(jsonEncode({'token': 'm11-session'}));
      } else {
        expect(
          request.headers.value(HttpHeaders.authorizationHeader),
          'Bearer m11-session',
        );
        if (request.method == 'PATCH' &&
            request.uri.path.endsWith('/books/1')) {
          bodies.add(
            jsonDecode(await utf8.decoder.bind(request).join())
                as Map<String, dynamic>,
          );
          request.response.statusCode = HttpStatus.noContent;
        } else if (request.method == 'GET' &&
            request.uri.path.endsWith('/books')) {
          request.response.write(jsonEncode([]));
        } else if (request.method == 'GET' &&
            request.uri.path.endsWith('/authors')) {
          request.response.write(
            jsonEncode([
              {'name': 'Clarke', 'book_count': 1},
            ]),
          );
        } else if (request.method == 'POST' &&
            request.uri.path.endsWith('/library/scan')) {
          request.response.write(
            jsonEncode({
              'scanned': 3,
              'imported': 1,
              'duplicates': 0,
              'failed': <Object>[],
            }),
          );
        } else if (request.method == 'POST' &&
            request.uri.path.endsWith('/collections')) {
          bodies.add(
            jsonDecode(await utf8.decoder.bind(request).join())
                as Map<String, dynamic>,
          );
          request.response.statusCode = HttpStatus.created;
          request.response.write(jsonEncode({'id': 8}));
        } else if (request.method == 'PATCH' &&
            request.uri.path.endsWith('/collections/8')) {
          bodies.add(
            jsonDecode(await utf8.decoder.bind(request).join())
                as Map<String, dynamic>,
          );
          request.response.statusCode = HttpStatus.noContent;
        } else if (request.method == 'GET' &&
            request.uri.path.endsWith('/scopes')) {
          request.response.write(
            jsonEncode([
              {
                'id': 2,
                'name': 'Clarke',
                'filter': {'author': 'Clarke'},
              },
            ]),
          );
        } else {
          request.response.statusCode = HttpStatus.notFound;
          request.response.write(jsonEncode({'error': 'missing'}));
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
      await api.updateBook(
        1,
        title: 'Piranesi',
        author: 'Susanna Clarke',
        series: 'Clarke',
        seriesIndex: 1,
      );
      await api.books(const BooksQuery(author: 'Clarke', series: 'Piranesi'));
      expect(await api.authors(), hasLength(1));
      final scan = await api.scanLibrary();
      expect(scan.scanned, 3);
      final collectionId = await api.createCollection('Favorites');
      await api.updateCollection(collectionId, addBookIds: [1]);
      expect(await api.listScopes(), hasLength(1));
      expect(bodies[0], {
        'title': 'Piranesi',
        'author': 'Susanna Clarke',
        'series': 'Clarke',
        'series_index': 1.0,
      });
      expect(bodies[1], {'name': 'Favorites'});
      expect(bodies[2], {
        'add_book_ids': [1],
      });
      expect(paths, [
        'POST /api/v1/login',
        'PATCH /api/v1/books/1',
        'GET /api/v1/books?author=Clarke&series=Piranesi',
        'GET /api/v1/authors',
        'POST /api/v1/library/scan',
        'POST /api/v1/collections',
        'PATCH /api/v1/collections/8',
        'GET /api/v1/scopes',
      ]);
    } finally {
      api.close();
      await subscription.cancel();
      await server.close(force: true);
    }
  });
  test('metadata search apply autofill and sync hit expected paths', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    final paths = <String>[];
    final subscription = server.listen((request) async {
      paths.add('${request.method} ${request.uri.path}');
      request.response.headers.contentType = ContentType.json;
      if (request.uri.path.endsWith('/login')) {
        request.response.write(jsonEncode({'token': 'm11-session'}));
      } else if (request.uri.path.endsWith('/metadata/search')) {
        request.response.write(
          jsonEncode({
            'results': [
              {
                'provider': 'openLibrary',
                'external_id': '/works/OL1',
                'title': 'Dune',
                'authors': ['Frank Herbert'],
              },
            ],
          }),
        );
      } else if (request.uri.path.endsWith('/metadata/apply')) {
        request.response.write(
          jsonEncode({
            'applied': ['description'],
            'metadata_score': 40,
          }),
        );
      } else if (request.uri.path.endsWith('/metadata/autofill')) {
        request.response.write(
          jsonEncode({'applied': <Object>[], 'metadata_score': 10}),
        );
      } else if (request.uri.path.endsWith('/metadata/fetch/run')) {
        request.response.write(jsonEncode({'queued': 2}));
      } else {
        request.response.statusCode = HttpStatus.notFound;
        request.response.write(jsonEncode({'error': 'missing'}));
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
      final results = await api.searchMetadata(
        title: 'Dune',
        author: 'Herbert',
      );
      expect(results.single['title'], 'Dune');
      await api.applyMetadataCandidate(3, results.single, ['description']);
      await api.autofillBookMetadata(3);
      expect(await api.syncLibraryMetadata(), 2);
      expect(paths, [
        'POST /api/v1/login',
        'POST /api/v1/metadata/search',
        'POST /api/v1/books/3/metadata/apply',
        'POST /api/v1/books/3/metadata/autofill',
        'POST /api/v1/metadata/fetch/run',
      ]);
    } finally {
      api.close();
      await subscription.cancel();
      await server.close(force: true);
    }
  });
}

class _RealHttpOverrides extends HttpOverrides {}
