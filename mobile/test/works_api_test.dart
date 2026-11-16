import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/api.dart';

void main() {
  test('Work.fromJson accepts server WorkJson shape', () {
    final work = Work.fromJson({
      'id': 3,
      'epub_book_id': 10,
      'audio_book_id': 11,
      'readaloud_book_id': 12,
      'status': 'ready',
      'stage': 'SYNC_CHAPTERS',
      'progress': 1,
      'error': null,
    });
    expect(work.id, 3);
    expect(work.epubBookId, 10);
    expect(work.audioBookId, 11);
    expect(work.readaloudBookId, 12);
    expect(work.isReady, isTrue);
  });

  test('lists, creates, gets, and starts works over the API', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    final requests = <String>[];
    final bodies = <Map<String, dynamic>>[];
    final subscription = server.listen((request) async {
      requests.add('${request.method} ${request.uri.path}');
      request.response.headers.contentType = ContentType.json;
      final path = request.uri.path;
      if (path.endsWith('/login')) {
        request.response.write(jsonEncode({'token': 'works-session'}));
      } else {
        expect(
          request.headers.value(HttpHeaders.authorizationHeader),
          'Bearer works-session',
        );
        if (request.method == 'POST' && path.endsWith('/works')) {
          final body = jsonDecode(
            await utf8.decoder.bind(request).join(),
          ) as Map<String, dynamic>;
          bodies.add(body);
          request.response.statusCode = HttpStatus.created;
          request.response.write(
            jsonEncode({
              'id': 7,
              'epub_book_id': body['epub_book_id'],
              'audio_book_id': body['audio_book_id'],
              'readaloud_book_id': null,
              'status': 'queued',
              'stage': 'SPLIT_TRACKS',
              'progress': 0,
              'error': null,
            }),
          );
        } else if (request.method == 'POST' && path.endsWith('/align')) {
          request.response.write(
            jsonEncode({
              'id': 7,
              'epub_book_id': 1,
              'audio_book_id': 2,
              'readaloud_book_id': null,
              'status': 'processing',
              'stage': 'TRANSCRIBE',
              'progress': 0.1,
              'error': null,
            }),
          );
        } else if (request.method == 'GET' && path.endsWith('/works/7')) {
          request.response.write(
            jsonEncode({
              'id': 7,
              'epub_book_id': 1,
              'audio_book_id': 2,
              'readaloud_book_id': null,
              'status': 'processing',
              'stage': 'TRANSCRIBE',
              'progress': 0.4,
              'error': null,
            }),
          );
        } else if (request.method == 'GET' && path.endsWith('/works')) {
          request.response.write(
            jsonEncode([
              {
                'id': 7,
                'epub_book_id': 1,
                'audio_book_id': 2,
                'readaloud_book_id': 9,
                'status': 'ready',
                'stage': 'SYNC_CHAPTERS',
                'progress': 1,
                'error': null,
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
      final listed = await api.works();
      expect(listed.single.readaloudBookId, 9);
      final created = await api.createWork(
        epubBookId: 1,
        audioBookId: 2,
        title: 'Shared Title',
      );
      expect(created.id, 7);
      expect(created.isBusy, isTrue);
      expect(bodies.single, {
        'epub_book_id': 1,
        'audio_book_id': 2,
        'start': true,
        'title': 'Shared Title',
      });
      final one = await api.work(7);
      expect(one.progress, 0.4);
      final started = await api.startWorkAlign(7);
      expect(started.status, 'processing');
      expect(requests, [
        'POST /api/v1/login',
        'GET /api/v1/works',
        'POST /api/v1/works',
        'GET /api/v1/works/7',
        'POST /api/v1/works/7/align',
      ]);
    } finally {
      api.close();
      await subscription.cancel();
      await server.close(force: true);
    }
  });
}

class _RealHttpOverrides extends HttpOverrides {}
