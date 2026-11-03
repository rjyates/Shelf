import 'dart:convert';
import 'dart:io';

/// FIFO JSON queue for Shelf write APIs that fail while offline.
class SyncOutbox {
  SyncOutbox(this.root);
  final Directory root;

  File get _file => File('${root.path}/sync_outbox.json');

  Future<List<Map<String, dynamic>>> _load() async {
    if (!await _file.exists()) return [];
    try {
      final data = jsonDecode(await _file.readAsString());
      if (data is! List) return [];
      return data
          .whereType<Map>()
          .map((item) => Map<String, dynamic>.from(item))
          .toList();
    } on FormatException {
      return [];
    }
  }

  Future<void> _save(List<Map<String, dynamic>> items) async {
    await root.create(recursive: true);
    await _file.writeAsString(jsonEncode(items), flush: true);
  }

  Future<int> get length async => (await _load()).length;

  Future<void> enqueue({
    required String method,
    required String path,
    Map<String, dynamic>? body,
  }) async {
    final items = await _load();
    items.add({
      'method': method,
      'path': path,
      'body': body,
      'enqueuedAt': DateTime.now().toUtc().toIso8601String(),
    });
    await _save(items);
  }

  /// Sends queued writes in order; stops on the first failure and keeps the rest.
  Future<int> flush(
    Future<void> Function(
      String method,
      String path,
      Map<String, dynamic>? body,
    )
    send,
  ) async {
    final items = await _load();
    var sent = 0;
    while (sent < items.length) {
      final item = items[sent];
      final method = item['method'];
      final path = item['path'];
      if (method is! String || path is! String) {
        sent += 1;
        continue;
      }
      final rawBody = item['body'];
      final body = rawBody is Map ? Map<String, dynamic>.from(rawBody) : null;
      try {
        await send(method, path, body);
      } catch (_) {
        break;
      }
      sent += 1;
    }
    if (sent > 0) await _save(items.sublist(sent));
    return sent;
  }
}
