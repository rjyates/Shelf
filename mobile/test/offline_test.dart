import 'dart:convert';
import 'dart:io';

import 'package:connectivity_plus/connectivity_plus.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shelf_mobile/api.dart';
import 'package:shelf_mobile/connectivity.dart';
import 'package:shelf_mobile/epub_store.dart';
import 'package:shelf_mobile/reader.dart';
import 'package:shelf_mobile/sync_outbox.dart';

void main() {
  late Directory temp;

  setUp(() async {
    temp = await Directory.systemTemp.createTemp('shelf-offline-');
  });

  tearDown(() async {
    if (await temp.exists()) await temp.delete(recursive: true);
  });

  test('epub store writes bytes keyed by book id', () async {
    final store = EpubStore(temp);
    expect(await store.has(7), isFalse);
    await store.save(7, [80, 75, 3, 4]);
    expect(await store.has(7), isTrue);
    expect(await store.fileFor(7).readAsBytes(), [80, 75, 3, 4]);
  });

  test('pdf store uses a .pdf extension beside epubs', () async {
    final store = EpubStore(temp);
    await store.save(9, [37, 80, 68, 70], extension: 'pdf');
    expect(await store.has(9, extension: 'pdf'), isTrue);
    expect(await store.has(9), isFalse);
    expect(store.fileFor(9, extension: 'pdf').path.endsWith('9.pdf'), isTrue);
  });

  test('cbz store uses a .cbz extension', () async {
    final store = EpubStore(temp);
    expect(EpubStore.extensionFor(isCbz: true), 'cbz');
    await store.save(3, [80, 75, 3, 4], extension: 'cbz');
    expect(await store.has(3, extension: 'cbz'), isTrue);
    expect(store.fileFor(3, extension: 'cbz').path.endsWith('3.cbz'), isTrue);
  });

  test('audiobook formats map to cache extensions', () {
    expect(EpubStore.extensionFor(format: 'm4b'), 'm4b');
    expect(EpubStore.extensionFor(format: 'mp3'), 'mp3');
    expect(EpubStore.extensionFor(format: 'opus'), 'opus');
    expect(EpubStore.extensionFor(format: 'flac'), 'flac');
    expect(EpubStore.extensionFor(format: 'audiozip'), 'zip');
  });

  test('digest keys prevent stale bytes when a book id is reused', () async {
    final store = EpubStore(temp);
    const oldDigest =
        '1111111111111111111111111111111111111111111111111111111111111111';
    const newDigest =
        '2222222222222222222222222222222222222222222222222222222222222222';
    await store.save(2, [1, 2, 3], extension: 'cbz', digest: oldDigest);
    expect(await store.has(2, extension: 'cbz', digest: oldDigest), isTrue);
    expect(await store.has(2, extension: 'cbz', digest: newDigest), isFalse);
    await store.save(2, [9, 9, 9], extension: 'cbz', digest: newDigest);
    expect(await store.has(2, extension: 'cbz', digest: oldDigest), isFalse);
    expect(
      await store.fileFor(2, extension: 'cbz', digest: newDigest).readAsBytes(),
      [9, 9, 9],
    );
  });

  test('outbox enqueues and flushes in order, keeping failures', () async {
    final outbox = SyncOutbox(temp);
    await outbox.enqueue(
      method: 'POST',
      path: 'books/1/progress',
      body: {'pages_read': 10},
    );
    await outbox.enqueue(
      method: 'POST',
      path: 'books/1/location',
      body: {'position': 'epubcfi(/6/2)', 'percentage': 0.2},
    );
    expect(await outbox.length, 2);

    final sent = <String>[];
    final flushed = await outbox.flush((method, path, body) async {
      sent.add('$method $path');
      if (path.endsWith('/location')) {
        throw ShelfException(
          'Could not reach your server. Check the address and connection.',
        );
      }
    });
    expect(flushed, 1);
    expect(sent, ['POST books/1/progress', 'POST books/1/location']);
    expect(await outbox.length, 1);

    final rest = await outbox.flush((method, path, body) async {
      sent.add('retry $method $path ${jsonEncode(body)}');
    });
    expect(rest, 1);
    expect(await outbox.length, 0);
    expect(sent.last, contains('books/1/location'));
  });

  test('ShelfHost outbox messages normalize API paths', () {
    final item = parseShelfHostOutbox(
      jsonEncode({
        'type': 'outbox',
        'method': 'POST',
        'path': '/api/v1/books/3/location',
        'body': {'position': 'epubcfi(/6/2)', 'percentage': 0.5},
      }),
    );
    expect(item?['path'], 'books/3/location');
    expect(item?['method'], 'POST');
    expect((item?['body'] as Map)['percentage'], 0.5);
    expect(parseShelfHostOutbox('close'), isNull);
    expect(
      parseShelfHostOutbox('{"type":"outbox","method":"GET","path":"books"}'),
      isNull,
    );
  });

  test('queue failures include transport and HTTP 5xx', () {
    expect(
      ShelfApi.isQueueFailure(
        ShelfException(
          'Could not reach your server. Check the address and connection.',
        ),
      ),
      isTrue,
    );
    expect(
      ShelfApi.isQueueFailure(ShelfException('boom', statusCode: 503)),
      isTrue,
    );
    expect(
      ShelfApi.isQueueFailure(ShelfException('bad', statusCode: 400)),
      isFalse,
    );
    expect(connectivityLooksOnline([ConnectivityResult.none]), isFalse);
    expect(connectivityLooksOnline([ConnectivityResult.wifi]), isTrue);
  });
}
